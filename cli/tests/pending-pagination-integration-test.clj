#!/usr/bin/env bb
;; Real Beagle Store + production replay-loop proof that a pending backlog larger than
;; the retired 4096 hard ceiling drains through bounded first pages.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file")))
            "../..")))
(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      "/home/tom/code/beagle/main/store"))
(when-not (.isFile (io/file store "bin/beagle-store-server"))
  (throw (ex-info "current Beagle store engine is required" {:store store})))
(load-file (str store "/database.clj"))
(require '[database :as database]
         '[store.types :as t])
(defn pagination-process-env
  [overrides]
  (merge (dissoc (into {} (System/getenv)) "BEAGLE_STORE_TELEMETRY_LOG")
         overrides))
(def inbox-peek (str root "/cli/inbox-peek.clj"))
(System/setProperty "north.live-feed.lib" "1")
(let [test-file (System/getProperty "babashka.file")
      live-feed-file (str root "/cli/north-live-feed.clj")]
  (System/setProperty "babashka.file" live-feed-file)
  (try
    (load-file live-feed-file)
    (finally
      (System/setProperty "babashka.file" test-file))))
(System/setProperty "north.inbox-peek.lib" "1")
(let [test-file (System/getProperty "babashka.file")]
  (System/setProperty "babashka.file" inbox-peek)
  (try
    (load-file inbox-peek)
    (finally
      (System/setProperty "babashka.file" test-file))))

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))
(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))
(defn throws-type? [expected f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (= expected (:type (ex-data error))))))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket
                (java.net.InetSocketAddress. "127.0.0.1" (int port))
                100)
      true)
    (catch Exception _ false)))

(defn await-coordinator! [port]
  (loop [attempt 0]
    (let [ready? (try
                   (let [status (north.coord/status port)]
                     (and (= :ready (:state status))
                          (= "north-coordination" (:space-id status))))
                   (catch Exception _ false))]
      (cond
        ready? true
        (>= attempt 750) false
        :else (do (Thread/sleep 100) (recur (inc attempt)))))))

(defn listener-pids [port]
  (->> (:out
        (proc/shell
         {:continue true :out :string :err :string}
         "ss" "-tlnpH" (str "sport = :" port)))
       (re-seq #"pid=([0-9]+)")
       (map (comp parse-long second))
       set))

(defn stop-process! [process]
  (try (proc/destroy-tree process) (catch Throwable _ nil))
  (let [java-process ^Process (:proc process)]
    (when-not (.waitFor java-process 5 java.util.concurrent.TimeUnit/SECONDS)
      (.destroyForcibly java-process)
      (.waitFor java-process 5 java.util.concurrent.TimeUnit/SECONDS))))

(def backlog-size 4097)
(def recipient "page-recipient")
(defn message-id [index]
  (format "@msg:page-%05d" index))
(defn populate-log! [file]
  (let [path (.getCanonicalPath file)]
    (database/create-triple-log! path "north-coordination" {:deflate? true})
    (let [db (database/open-database! path "north-coordination")
          operations
          (mapcat
           (fn [index]
             (let [message (message-id index)]
               [{:action :assert
                 :proposition (t/triple message "from" "page-sender")}
                {:action :assert
                 :proposition (t/triple message "subject"
                                        (format "page-subject-%05d" index))}
                {:action :assert
                 :proposition (t/triple message "body"
                                        (format "page-body-%05d" index))}
                {:action :assert
                 :proposition (t/triple message "to" recipient)}]))
           (range backlog-size))]
      (doseq [batch (partition-all 2048 operations)]
        (database/commit! db {:operations (vec batch)})))
    path))
(defn output-indices [output]
  (mapv (comp parse-long second)
        (re-seq #"page-subject-([0-9]{5})" output)))
(defn message-index [message]
  (some->> message
           (re-matches #"@msg:page-([0-9]{5})")
           second
           parse-long))

(defn permissions [file]
  (java.nio.file.attribute.PosixFilePermissions/toString
   (java.nio.file.Files/getPosixFilePermissions
    (.toPath (io/file file))
    (make-array java.nio.file.LinkOption 0))))

(check! "direct addresses are canonical, deduplicated, and recipient-inclusive"
        (= ["page-recipient" "reviewer"]
           (north.message-audience/bounded-direct-addresses
            "page-recipient" ["reviewer" "reviewer"])))
(check! "malformed and oversized direct addresses fail before query construction"
        (and
         (throws-type?
          :invalid-direct-address
          #(north.message-audience/bounded-direct-addresses
            "page-recipient" ["bad/address"]))
         (throws-type?
          :invalid-direct-address
          #(north.message-audience/bounded-direct-addresses
            "page-recipient"
            [(apply str
                    (repeat
                     (inc north.message-audience/max-direct-address-bytes)
                     "x"))]))))
(check! "duplicate-heavy direct input is bounded by scanned elements"
        (throws-type?
         :direct-address-limit-exceeded
         #(north.message-audience/bounded-direct-addresses
           "page-recipient"
           (repeat
            (inc north.message-audience/max-direct-addresses)
            "reviewer"))))
(check! "query-page cursors accept only canonical recursive Terms"
        (let [cursor (t/triple "@cursor" :page 1)]
          (and (north.coord/valid-query-page-cursor? nil)
               (north.coord/valid-query-page-cursor? cursor)
               (not (north.coord/valid-query-page-cursor? "cursor"))
               (not (north.coord/valid-query-page-cursor?
                     ["@cursor" :page 1])))))

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-pending-pages"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "coordination.storelog")
      _ (populate-log! log)
      canonical-log (.getCanonicalPath log)
      daemon
      (proc/process
       {:dir store
        :out :string
        :err :string
        :env (pagination-process-env
               "BEAGLE_STORE_SERVER_QUIET" "1"
               "BEAGLE_STORE_SERVER_XMX" "2g"})}
       (str store "/bin/beagle-store-server") "serve" (str port)
       canonical-log "north-coordination")
      page-sizes (atom [])
      original-page north.message-audience/pending-message-page]
  (try
    (check! "throwaway current Beagle Store server is ready"
            (await-coordinator! port))
    (let [status (north.coord/status port)
          daemon-pid (.pid ^Process (:proc daemon))]
      (check! "paged fixture is bound to the canonical SpaceId"
              (and (= :ready (:state status))
                   (= "north-coordination" (:space-id status))))
      (check! "throwaway paged coordinator owns its kernel-selected port"
              (contains? (listener-pids port) daemon-pid)))
    ;; The PostToolUse path must not scan/materialize the whole relation before
    ;; its first byte. Run the real helper twice under the exact 2s outer
    ;; hook deadline. Distinct subjects prove the persisted deletion-safe cursor
    ;; and acknowledgements make forward progress across turns.
    (with-redefs [north.coord/expected-log (constantly canonical-log)]
      (let [_warm-page
            ;; Beagle Store's first relational query builds its local index. Production
            ;; coordinators are long-lived; warm that engine boundary outside
            ;; the hook deadline so this bar measures North's bounded replay
            ;; path rather than one-time daemon index construction.
            (north.message-audience/pending-message-page
             port recipient #{recipient} 1 nil)
            runtime (io/file tmp "hook-runtime")
          _ (.mkdirs runtime)
          invoke
          (fn []
            (let [started (System/nanoTime)
                  result
                  (proc/shell
                   {:continue true
                    :out :string
                    :err :string
                    :env
                    (pagination-process-env
                     {"BEAGLE_STORE_LOG" canonical-log
                      "XDG_RUNTIME_DIR" (.getCanonicalPath runtime)})}
                   "timeout" "--signal=TERM" "--kill-after=0.1s" "2s"
                   "bb" inbox-peek (str port) recipient)]
              (assoc result
                     :elapsed-ms
                     (/ (- (System/nanoTime) started) 1000000.0))))
          first-turn (invoke)
          second-turn (invoke)
          first-ids (output-indices (:out first-turn))
          second-ids (output-indices (:out second-turn))
          actor-key (managed-actor-key recipient)
          space-key (canonical-space-key port)
          state-root (io/file runtime "north-inbox-peek")
          state-file (io/file state-root actor-key)
          lock-file (io/file state-root (str actor-key ".lock"))
          state (when (.isFile state-file)
                  (edn/read-string (slurp state-file)))]
      (when (or (empty? first-ids) (empty? second-ids) (nil? state))
        (binding [*out* *err*]
          (println "pending hook diagnostics"
                   (pr-str
                    {:first (select-keys first-turn [:exit :elapsed-ms :out :err])
                     :second (select-keys second-turn [:exit :elapsed-ms :out :err])
                     :state-files (mapv #(.getName ^java.io.File %)
                                        (or (seq (.listFiles state-root)) []))
                     :state-summary (when state
                                      {:ids (count (:ids state))
                                       :first-id (first (:ids state))
                                       :cursor (:cursor state)})}))))
      (when (nil? state)
        (throw (ex-info "bounded hook did not persist its continuation spool"
                        {:type :missing-inbox-spool})))
      (check! ">4096 backlog emits nonempty hook context inside the first deadline"
              (and (zero? (:exit first-turn))
                   ;; GNU timeout is the child deadline. Allow a small parent-side
                   ;; process launch/reap allowance so scheduler noise cannot turn
                   ;; a correctly killed child into a flaky elapsed-time verdict.
                   (< (:elapsed-ms first-turn) 2250.0)
                   (seq first-ids)
                   (= 0 (first first-ids))))
      (check! "a second bounded hook turn advances instead of rescanning the prefix"
              (and (zero? (:exit second-turn))
                   (< (:elapsed-ms second-turn) 2250.0)
                   (seq first-ids)
                   (seq second-ids)
                   (> (first second-ids) (last first-ids))
                   (= (count second-ids) (count (distinct second-ids)))
                   (empty? (set/intersection (set first-ids)
                                             (set second-ids)))))
      (check! "each hook turn honors the 3-message and 24KiB output bounds"
              (and (<= (count first-ids) delivery-limit)
                   (<= (count second-ids) delivery-limit)
                   (<= (utf8-bytes (:out first-turn)) output-byte-limit)
                   (<= (utf8-bytes (:out second-turn)) output-byte-limit)))
      (check! "hook state uses the canonical managed actor key with no temp residue"
              (and (.isFile state-file)
                   (.isFile lock-file)
                   (empty?
                    (filter #(str/ends-with? (.getName ^java.io.File %) ".tmp")
                            (or (seq (.listFiles state-root)) [])))))
      (check! "hook directory, spool, and lock permissions are private"
              (and (.isDirectory state-root)
                   (.isFile state-file)
                   (.isFile lock-file)
                   (= "rwx------" (permissions state-root))
                   (= "rw-------" (permissions state-file))
                   (= "rw-------" (permissions lock-file))))
      (check! "spool is a strict bounded engine page, not a synthesized cursor"
              (and (= state-keys (set (keys state)))
                   (= spool-schema (:schema state))
                   (= actor-key (:actor-key state))
                   (= space-key (:space-key state))
                   (integer? (:snapshot-version state))
                   (<= (:snapshot-version state)
                       (north.coord/cur-ver port))
                   (<= (count (:ids state)) spool-page-limit)
                   (every? valid-message-id? (:ids state))
                   (= (message-id (inc (last second-ids)))
                      (first (:ids state)))
                   (or (nil? (:cursor state))
                       (valid-cursor? (:cursor state)))))

      ;; A crash after the graph ack but before the spool rewrite leaves a stale
      ;; settled prefix. Reintroduce one exact settled ID: the next turn must
      ;; consult the graph, skip it without duplicate output, and keep advancing.
      (let [settled (message-id (first first-ids))
            crash-state
            (assoc state
                   :snapshot-version (north.coord/cur-ver port)
                   :created-at-ms (System/currentTimeMillis)
                   :ids (into [settled] (:ids state)))
            _ (atomic-write! (.toPath state-file) crash-state)
            crash-turn (invoke)
            crash-ids (output-indices (:out crash-turn))]
        (check! "settled crash residue is re-read from the graph and never re-emitted"
                (and (zero? (:exit crash-turn))
                     (seq crash-ids)
                     (not (some #{(first first-ids)} crash-ids))
                     (> (first crash-ids) (last second-ids)))))

      ;; A foreign live consumer owns the graph claim, not this cache. The hook
      ;; drops only its hint; after release, the same exact ID remains deliverable.
      (let [current (edn/read-string (slurp state-file))
            foreign-id (first (:ids current))
            single (assoc current
                          :snapshot-version (north.coord/cur-ver port)
                          :created-at-ms (System/currentTimeMillis)
                          :ids [foreign-id]
                          :cursor nil)
            claim (north.message-audience/claim-delivery!
                   port foreign-id recipient)
            _ (atomic-write! (.toPath state-file) single)
            blocked-turn (invoke)
            blocked-ack
            (set (north.coord/many port foreign-id "acked_by"))]
        (check! "foreign graph claim prevents cached output and acknowledgement"
                (and claim
                     (zero? (:exit blocked-turn))
                     (str/blank? (:out blocked-turn))
                     (not (contains? blocked-ack recipient))))
        (north.message-audience/release-delivery-claim! port claim)
        (atomic-write! (.toPath state-file)
                       (assoc single
                              :snapshot-version (north.coord/cur-ver port)
                              :created-at-ms (System/currentTimeMillis)))
        (let [released-turn (invoke)]
          (check! "released foreign claim leaves graph mail available to the next turn"
                  (and (zero? (:exit released-turn))
                       (str/includes?
                        (:out released-turn)
                        (format "page-subject-%05d"
                                (message-index foreign-id)))
                       (= #{recipient}
                          (set (north.coord/many port foreign-id "acked_by")))))))

      ;; Corrupt, stale, and cross-corpus files are discarded rather than used as
      ;; cursor or delivery authority. State deletion is itself directory-fsynced.
      (let [base {:schema spool-schema
                  :actor-key actor-key
                  :space-key space-key
                  :snapshot-version (north.coord/cur-ver port)
                  :created-at-ms (System/currentTimeMillis)
                  :ids [(message-id 4000)]
                  :cursor nil}]
        (atomic-write! (.toPath state-file)
                       (assoc base :created-at-ms
                              (- (System/currentTimeMillis)
                                 spool-max-age-ms 1)))
        (check! "stale spool is discarded without becoming graph authority"
                (and (nil? (read-spool port (.toPath state-file)
                                       actor-key space-key
                                       (System/currentTimeMillis)))
                     (not (.exists state-file))))
        (spit state-file "{")
        (check! "corrupt spool is discarded without a cursor guess"
                (and (nil? (read-spool port (.toPath state-file)
                                       actor-key space-key
                                       (System/currentTimeMillis)))
                     (not (.exists state-file))))
        (java.nio.file.Files/write
         (.toPath state-file)
         (byte-array [(byte -1)])
         (into-array java.nio.file.OpenOption
                     [java.nio.file.StandardOpenOption/CREATE_NEW
                      java.nio.file.StandardOpenOption/WRITE]))
        (check! "non-UTF-8 spool bytes are rejected rather than replacement-decoded"
                (and (nil? (read-spool port (.toPath state-file)
                                       actor-key space-key
                                       (System/currentTimeMillis)))
                     (not (.exists state-file))))
        (atomic-write! (.toPath state-file)
                       (assoc base :space-key (apply str (repeat 64 "f"))))
        (check! "foreign-corpus spool is discarded before any cached ID is read"
                (and (nil? (read-spool port (.toPath state-file)
                                       actor-key space-key
                                       (System/currentTimeMillis)))
                     (not (.exists state-file)))))

      ;; A second hook never waits out the foreground deadline behind a live
      ;; sibling. The kernel releases the lock when the holder channel closes.
      (let [entered (promise)
            release (promise)
            holder
            (future
              (with-state-lock
                (.toPath lock-file)
                (+ (System/nanoTime) 1000000000)
                #(do (deliver entered true) @release)))
            _ @entered
            ran (atom false)
            started (System/nanoTime)
            result
            (with-state-lock
              (.toPath lock-file)
              (+ (System/nanoTime) 30000000)
              #(reset! ran true))
            elapsed-ms (/ (- (System/nanoTime) started) 1000000.0)]
        (deliver release true)
        @holder
        (check! "concurrent spool lock contention is bounded and side-effect free"
                (and (nil? result) (not @ran) (< elapsed-ms 150.0))))))
    (with-redefs
      [north.coord/expected-log (constantly canonical-log)
       north.message-audience/pending-message-page
       (fn
         ([p r addresses]
          (let [page (original-page p r addresses)]
            (swap! page-sizes conj (count (:messages page)))
            page))
         ([p r addresses limit after]
          (let [page (original-page p r addresses limit after)]
            (swap! page-sizes conj (count (:messages page)))
            page)))
       deliver-message!
       (fn [p r message _control _claim-ttl _ack-timeout]
         (let [result (north.coord/append! p message "acked_by" r)]
           (when (:reject result)
             (throw (ex-info "fixture acknowledgement rejected" result)))
           :acked))]
      (let [initial
            (north.message-audience/pending-message-page
             port recipient #{recipient})]
        (check! "first real pending page is bounded"
                (and (= north.message-audience/pending-page-limit
                        (count (:messages initial)))
                     (not (:done? initial)))))
      (replay-pending!
       port recipient #{recipient}
       (java.util.concurrent.LinkedBlockingQueue.)
       30000 10000)
      (let [remaining
            (north.message-audience/pending-message-page
             port recipient #{recipient})
            acked
            (north.coord/query-rows
             port
             {:find "acked"
              :rules
              [{:head {:rel "acked" :args [{:var "e"}]}
                :body [{:rel "triple"
                        :args [{:var "e"} "acked_by" recipient]}]}]})]
        (check! "production replay settles all 4097 pending messages"
                (and (empty? (:messages remaining))
                     (= backlog-size (count acked))))
        (check! "every replay query stays within the fixed page size"
                (and (> (count @page-sizes) 16)
                     (every?
                      #(<= % north.message-audience/pending-page-limit)
                      @page-sizes)))
        (check! "replay reaches a final empty first page"
                (zero? (last @page-sizes)))))
    (finally
      (stop-process! daemon)
      (cleanup-scratch (.getCanonicalPath tmp)))))

(let [failures (remove second @checks)]
  (doseq [[label ok] @checks]
    (println (if ok "  [PASS] " "  [FAIL] ") label))
  (if (seq failures)
    (do
      (println "\npending pagination:" (count failures) "FAILED")
      (System/exit 1))
    (println "\npending pagination:"
             (count @checks) "/" (count @checks) "PASS")))
