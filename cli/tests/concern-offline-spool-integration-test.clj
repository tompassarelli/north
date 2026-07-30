#!/usr/bin/env bb
;; Phase 1 bar: concern declare remains live-compatible and gains one bounded,
;; private, immutable local durability path for coordinator transport ambiguity.
(require '[babashka.process :as p]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-root
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def subject-root
  (.getCanonicalPath
   (io/file (or (System/getenv "NORTH_TEST_SUBJECT_ROOT") test-root))))
(load-file (str test-root "/cli/concern-spool.clj"))

(def fails (atom 0))
(defn check [label observation]
  (println (str "  " (if observation "PASS" "FAIL") " — " label))
  (when-not observation (swap! fails inc))
  observation)

(defn temp-directory [prefix]
  (.toFile
   (java.nio.file.Files/createTempDirectory
    prefix
    (make-array java.nio.file.attribute.FileAttribute 0))))

(defn delete-tree! [file]
  (doseq [entry (reverse (file-seq file))]
    (io/delete-file entry true)))

(defn free-port []
  (or
   (some
    (fn [port]
      (try
        (with-open [socket (java.net.ServerSocket. port)]
          port)
        (catch Exception _ nil)))
    (range 17660 17740))
   (throw (ex-info "no test port available" {}))))

(defn start-blackhole [port]
  (let [server (java.net.ServerSocket. port)
        sockets (atom [])
        acceptor
        (future
          (try
            (loop []
              (let [socket (.accept server)]
                (swap! sockets conj socket)
                (future
                  (try
                    (.read (.getInputStream socket))
                    (Thread/sleep 10000)
                    (catch Throwable _ nil)))
                (recur)))
            (catch Throwable _ nil)))]
    {:server server :sockets sockets :acceptor acceptor}))

(defn stop-server! [{:keys [server sockets acceptor]}]
  (try (.close server) (catch Throwable _ nil))
  (doseq [socket @sockets]
    (try (.close socket) (catch Throwable _ nil)))
  (future-cancel acceptor))

(defn start-responder [port response-for]
  (let [server (java.net.ServerSocket. port)
        acceptor
        (future
          (try
            (loop []
              (with-open [socket (.accept server)
                          reader
                          (io/reader (.getInputStream socket))
                          writer
                          (io/writer (.getOutputStream socket))]
                (let [envelope (edn/read-string (.readLine reader))
                      response (response-for (:request envelope))]
                  (.write writer (str (pr-str response) "\n"))
                  (.flush writer)))
              (recur))
            (catch Throwable _ nil)))]
    {:server server :sockets (atom []) :acceptor acceptor}))

(defn coordinator-op [port log request]
  (with-open [socket (java.net.Socket. "127.0.0.1" (int port))
              reader (io/reader (.getInputStream socket))
              writer (io/writer (.getOutputStream socket))]
    (.write
     writer
     (str
      (pr-str
       {:op :for-log
        :expected-log (.getCanonicalPath log)
        :request request})
      "\n"))
    (.flush writer)
    (edn/read-string (.readLine reader))))

(defn await-port [port process]
  (loop [attempt 0]
    (cond
      (>= attempt 100)
      false

      (not= ::running (deref process 0 ::running))
      false

      :else
      (let [open?
            (try
              (with-open [socket (java.net.Socket.)]
                (.connect
                 socket
                 (java.net.InetSocketAddress. "127.0.0.1" (int port))
                 50)
                true)
              (catch Exception _ false))]
        (if open?
          true
          (do
            (Thread/sleep 50)
            (recur (inc attempt))))))))

(defn operation-files [directory]
  (if-not (.isDirectory directory)
    []
    (->> (.listFiles directory)
         (filter #(str/ends-with? (.getName %) ".op.edn"))
         (sort-by #(.getName %))
         vec)))

(defn temp-files [directory]
  (if-not (.isDirectory directory)
    []
    (->> (.listFiles directory)
         (filter #(str/ends-with? (.getName %) ".tmp"))
         vec)))

(defn run-declare
  [root port log spool repo extra-env & extra-args]
  (let [env
        (merge
         {"NORTH_BB" "bb"
          "NORTH_PORT" (str port)
          "FRAM_LOG" (.getCanonicalPath log)
          "NORTH_TELEMETRY_PARTITION" "0"
          "NORTH_CONCERN_SPOOL_DIR" (.getCanonicalPath spool)
          "NORTH_CONCERN_DECLARE_TRANSPORT_TIMEOUT_MS" "300"}
         extra-env)
        process
        (apply
         p/process
         {:dir root :out :string :err :string :extra-env env}
         (str root "/bin/concern")
         "declare" "offline-fixture" (.getCanonicalPath repo)
         "durable concern fixture" "src/z.clj,src/a.clj"
         extra-args)
        started (System/nanoTime)
        result (deref process 2200 ::timeout)
        elapsed-ms (quot (- (System/nanoTime) started) 1000000)]
    (when (= ::timeout result)
      (p/destroy-tree process))
    (if (= ::timeout result)
      {:timeout true :elapsed-ms elapsed-ms :exit nil :out "" :err ""}
      (assoc result :timeout false :elapsed-ms elapsed-ms))))

(defn private-mode [file]
  (java.nio.file.Files/getPosixFilePermissions
   (.toPath file)
   (make-array java.nio.file.LinkOption 0)))

(def directory-mode
  #{java.nio.file.attribute.PosixFilePermission/OWNER_READ
    java.nio.file.attribute.PosixFilePermission/OWNER_WRITE
    java.nio.file.attribute.PosixFilePermission/OWNER_EXECUTE})
(def file-mode
  #{java.nio.file.attribute.PosixFilePermission/OWNER_READ
    java.nio.file.attribute.PosixFilePermission/OWNER_WRITE})

(defn blackhole-probe [root]
  (let [tmp (temp-directory "north-concern-offline-blackhole")
        repo (doto (io/file tmp "repo") .mkdirs)
        spool (io/file tmp "spool")
        log (io/file tmp "coordination.log")
        sentinel "canonical-log-sentinel\n"
        port (free-port)
        server (start-blackhole port)]
    (spit log sentinel)
    (try
      (let [result (run-declare root port log spool repo {})
            files (operation-files spool)
            operation
            (when (= 1 (count files))
              (north.concern-spool/read-operation-file! (first files)))
            facts (:facts operation)
            last-fact (peek facts)]
        (check "coordinator blackhole returns durable-local in under two seconds"
               (and (not (:timeout result))
                    (zero? (:exit result))
                    (< (:elapsed-ms result) 2000)
                    (str/includes? (:out result) "durable-local")
                    (str/includes? (:out result) "visibility=pending")))
        (check "fallback never appends directly to the exact target log"
               (= sentinel (slurp log)))
        (check "fallback publishes exactly one complete operation and no temp"
               (and (= 1 (count files))
                    (empty? (temp-files spool))
                    operation))
        (check "state directory and immutable operation are private"
               (and (= 1 (count files))
                    (.isDirectory spool)
                    (= directory-mode (private-mode spool))
                    (= file-mode (private-mode (first files)))))
        (check "operation binds identity, exact log, ordered facts and digests"
               (and (= "north-concern-operation-v1" (:schema-version operation))
                    (= "concern-declare" (:operation-type operation))
                    (= (.getCanonicalPath log) (:target-log operation))
                    (= (:concern-id operation)
                       (get-in operation [:precondition :subject]))
                    (= (:facts-sha256 operation)
                       (get-in operation [:precondition :projection-sha256]))
                    (re-matches #"[0-9a-f]{64}" (:sha256 operation))
                    (= (range (count facts)) (map :ordinal facts))
                    (= ["kind" "concern" "single"]
                       [(:predicate last-fact)
                        (:object last-fact)
                        (:cardinality last-fact)])
                    (= (:terminal-commit-marker operation)
                       (select-keys last-fact [:ordinal :predicate :object]))))
        result)
      (finally
        (stop-server! server)
        (delete-tree! tmp)))))

(when (= "parent-red" (first *command-line-args*))
  (blackhole-probe subject-root)
  (if (zero? @fails) (System/exit 0) (System/exit 1)))

(blackhole-probe subject-root)

;; Parallel publishers share one bounded capacity turn. Every success must be a
;; separately valid immutable operation; no writer may observe a torn peer file.
(let [tmp (temp-directory "north-concern-offline-parallel")
      repo (doto (io/file tmp "repo") .mkdirs)
      spool (io/file tmp "spool")
      log (doto (io/file tmp "coordination.log") (spit "parallel-sentinel\n"))
      port (free-port)
      server (start-blackhole port)]
  (try
    (let [runs
          (->> (range 6)
               (mapv
                (fn [index]
                  (future
                    (run-declare
                     subject-root port log spool repo
                     {"NORTH_CONCERN_SPOOL_MAX_FILES" "8"}
                     "--about" (str "@parallel-thread-" index)))))
               (mapv deref))
          files (operation-files spool)
          operations
          (mapv north.concern-spool/read-operation-file! files)]
      (check "parallel fallback publications all finish inside the bound"
             (every?
              #(and (not (:timeout %))
                    (zero? (:exit %))
                    (< (:elapsed-ms %) 2000))
              runs))
      (check "parallel publication yields six unique complete operations"
             (and (= 6 (count files))
                  (= 6 (count (set (map :operation-id operations))))
                  (= 6 (count (set (map :concern-id operations))))
                  (empty? (temp-files spool)))))
    (finally
      (stop-server! server)
      (delete-tree! tmp))))

;; Full capacity is a typed failure, never a false durable-local receipt.
(let [tmp (temp-directory "north-concern-offline-full")
      repo (doto (io/file tmp "repo") .mkdirs)
      spool (io/file tmp "spool")
      log (doto (io/file tmp "coordination.log") (spit "full-sentinel\n"))
      port (free-port)
      server (start-blackhole port)]
  (try
    (let [env {"NORTH_CONCERN_SPOOL_MAX_FILES" "1"}
          first-result (run-declare subject-root port log spool repo env)
          second-result (run-declare subject-root port log spool repo env)]
      (check "first operation fills the one-record fixture"
             (and (zero? (:exit first-result))
                  (= 1 (count (operation-files spool)))))
      (check "full spool exits nonzero without a false success receipt"
             (and (= 4 (:exit second-result))
                  (str/includes? (:err second-result) "spool is full")
                  (not (str/includes? (:out second-result) "durable-local"))
                  (= 1 (count (operation-files spool))))))
    (finally
      (stop-server! server)
      (delete-tree! tmp))))

;; A non-canonical ref cannot be made authoritative while its meaning is
;; unreadable, and an explicit coordinator reject never becomes local success.
(let [tmp (temp-directory "north-concern-offline-alias")
      repo (doto (io/file tmp "repo") .mkdirs)
      spool (io/file tmp "spool")
      log (doto (io/file tmp "coordination.log") (spit "alias-sentinel\n"))
      port (free-port)
      server (start-blackhole port)]
  (try
    (let [result
          (run-declare
           subject-root port log spool repo {} "--about" "ambiguous-alias")]
      (check "ambiguous about resolution never spools"
             (and (= 2 (:exit result))
                  (empty? (operation-files spool))
                  (not (str/includes? (:out result) "durable-local")))))
    (finally
      (stop-server! server)
      (delete-tree! tmp))))

;; A parsed coordinator response that rejects the canonical batch is semantic,
;; not transport ambiguity. It must remain a hard failure with no local record.
(let [tmp (temp-directory "north-concern-offline-reject")
      repo (doto (io/file tmp "repo") .mkdirs)
      spool (io/file tmp "spool")
      log (doto (io/file tmp "coordination.log") (spit "reject-sentinel\n"))
      port (free-port)
      server
      (start-responder
       port
       (fn [request]
         (case (:op request)
           :resolved
           {:value nil
            :members 0
            :ambiguous? false
            :values []
            :version 0}

           :assert
           {:ok 1}

           :assert-batch
           {:reject :semantic}

           {:error "unexpected test operation"})))]
  (try
    (let [result (run-declare subject-root port log spool repo {})]
      (check "explicit semantic rejection never becomes durable-local"
             (and (not (:timeout result))
                  (not (zero? (:exit result)))
                  (str/includes?
                   (:err result)
                   "explicitly rejected concern declaration")
                  (empty? (operation-files spool))
                  (not (str/includes? (:out result) "durable-local")))))
    (finally
      (stop-server! server)
      (delete-tree! tmp))))

;; One real strict coordinator proves the normal path still publishes the same
;; concern fact projection and creates no local operation.
(let [tmp (temp-directory "north-concern-offline-live")
      repo (doto (io/file tmp "repo") .mkdirs)
      spool (io/file tmp "spool")
      log (doto (io/file tmp "coordination.log") (spit ""))
      telemetry-log (doto (io/file tmp "telemetry.log") (spit ""))
      port (free-port)
      fram "/home/tom/code/fram/main"
      env
      {"FRAM_LOG" (.getCanonicalPath log)
       "FRAM_TELEMETRY_LOG" (.getCanonicalPath telemetry-log)
       "NORTH_TELEMETRY_PARTITION" "0"
       "NORTH_TELEMETRY_PORT" (str port)
       "FRAM_REQUIRE_LOG_FENCE" "1"}
      daemon
      (p/process
       {:dir fram :out :string :err :string :extra-env env}
       "bb" "-cp" "out" "coord_daemon.clj" "serve-flat"
       (str port) (.getCanonicalPath log))]
  (try
    (check "strict live coordinator fixture starts" (await-port port daemon))
    (let [result
          (run-declare
           subject-root port log spool repo
           {"NORTH_CONCERN_DECLARE_TRANSPORT_TIMEOUT_MS" "1500"})
          concern-id
          (second
           (re-find #"(@concern-[0-9]+-[0-9a-f]{4})" (:out result)))
          values
          (fn [predicate]
            (set
             (:values
              (coordinator-op
               port log
               {:op :resolved :te concern-id :p predicate}))))
          live-ok?
          (and (not (:timeout result))
               (zero? (:exit result))
               concern-id
               (not (str/includes? (:out result) "durable-local"))
               (empty? (operation-files spool)))]
      (when-not live-ok?
        (println
         "    live diagnostic"
         (pr-str
          {:result (select-keys result [:timeout :elapsed-ms :exit :out :err])
           :operation-files (mapv #(.getName %) (operation-files spool))})))
      (check "live declaration remains coordinator-visible, not pending" live-ok?)
      (check "live batch preserves the complete concern fact semantics"
             (and (= #{"concern"} (values "kind"))
                  (= #{"building"} (values "reached"))
                  (= #{"@offline-fixture"} (values "agent"))
                  (= #{"@offline-fixture"} (values "driver"))
                  (= #{(.getCanonicalPath repo)} (values "repo"))
                  (= #{"durable concern fixture"} (values "intent"))
                  (= #{"src/a.clj" "src/z.clj"} (values "touches")))))
    (finally
      (try (p/destroy-tree daemon) (catch Throwable _ nil))
      (delete-tree! tmp))))

;; Deterministic pre-rename failure cleans its exclusive temp. A subsequently
;; observed orphan temp is safe to remove because the capacity lock proves no
;; publisher can still own it.
(let [tmp (temp-directory "north-concern-offline-crash")
      spool (.toPath (io/file tmp "spool"))
      sample
      (fn []
        (north.concern-spool/build-operation
         {:operation-id (str (java.util.UUID/randomUUID))
          :concern-id
          (str "@concern-" (System/currentTimeMillis) "-"
               (format "%04x" (rand-int 65536)))
          :target-log (.getCanonicalPath (io/file tmp "coordination.log"))
          :created-at (str (java.time.Instant/now))
          :facts
          [{:predicate "title" :object "crash fixture" :cardinality "single"}
           {:predicate "kind" :object "concern" :cardinality "single"}]}))]
  (try
    (with-redefs [north.concern-spool/state-directory (fn [] spool)]
      (try
        (binding [north.concern-spool/*publish-stage!*
                  (fn [stage _]
                    (when (= :file-fsynced stage)
                      (throw (ex-info "injected crash" {}))))]
          (north.concern-spool/publish-operation! (sample)))
        (catch Exception _ nil))
      (check "pre-rename crash leaves no operation or temp"
             (and (empty? (operation-files (.toFile spool)))
                  (empty? (temp-files (.toFile spool)))))
      (spit (.toFile (.resolve spool ".orphan.tmp")) "orphan")
      (north.concern-spool/publish-operation! (sample))
      (check "next exclusive publication recovers an orphan temp"
             (and (= 1 (count (operation-files (.toFile spool))))
                  (empty? (temp-files (.toFile spool))))))
    (finally
      (delete-tree! tmp))))

(if (zero? @fails)
  (do
    (println "\nconcern offline spool Phase 1: ALL PASS")
    (System/exit 0))
  (do
    (println (str "\nconcern offline spool Phase 1: " @fails " FAIL"))
    (System/exit 1)))
