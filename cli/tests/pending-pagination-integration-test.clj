#!/usr/bin/env bb
;; Real Fram + production replay-loop proof that a pending backlog larger than
;; the retired 4096 hard ceiling drains through bounded first pages.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file")))
            "../..")))
(def fram
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (str (System/getProperty "user.home") "/code/fram")))
(System/setProperty "north.live-feed.lib" "1")
(let [test-file (System/getProperty "babashka.file")
      live-feed-file (str root "/cli/north-live-feed.clj")]
  (System/setProperty "babashka.file" live-feed-file)
  (try
    (load-file live-feed-file)
    (finally
      (System/setProperty "babashka.file" test-file))))

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))
(defn throws-type? [expected f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (= expected (:type (ex-data error))))))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket
                (java.net.InetSocketAddress. "127.0.0.1" (int port))
                100)
      true)
    (catch Exception _ false)))

(defn eventually [f]
  (loop [remaining 300]
    (cond
      (try (f) (catch Throwable _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 20) (recur (dec remaining))))))

(defn stop-process! [process]
  (try (proc/destroy-tree process) (catch Throwable _ nil))
  (let [java-process ^Process (:proc process)]
    (when-not (.waitFor java-process 5 java.util.concurrent.TimeUnit/SECONDS)
      (.destroyForcibly java-process)
      (.waitFor java-process 5 java.util.concurrent.TimeUnit/SECONDS))))

(defn start-one-shot-server [response]
  (let [server (java.net.ServerSocket. 0)
        worker
        (future
          (try
            (with-open [socket (.accept server)
                        reader (io/reader (.getInputStream socket))
                        writer (io/writer (.getOutputStream socket))]
              (.readLine ^java.io.BufferedReader reader)
              (.write writer response)
              (.write writer "\n")
              (.flush writer))
            (finally
              (.close server))))]
    {:port (.getLocalPort server) :worker worker}))

(def backlog-size 4097)
(def recipient "page-recipient")
(defn message-id [index]
  (format "@msg:page-%05d" index))
(defn fact-line [tx subject predicate object]
  (pr-str {:tx tx :op "assert"
           :l subject :p predicate :r object :frame "fixture"}))

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

(let [port (free-port)
      tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-pending-pages"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "facts.log")
      _ (spit log
              (str
               (str/join
                "\n"
                (mapv
                 (fn [index]
                   (fact-line
                    (inc index) (message-id index) "to" recipient))
                 (range backlog-size)))
               "\n"))
      canonical-log (.getCanonicalPath log)
      daemon
      (proc/process
       {:dir fram
        :out :string
        :err :string
        :extra-env {"FRAM_REQUIRE_LOG_FENCE" "1"}}
       "bb" "-cp" "out" "coord_daemon.clj"
       "serve-flat" (str port) canonical-log)
      page-sizes (atom [])
      original-page north.message-audience/pending-message-page]
  (try
    (check! "throwaway paged coordinator starts"
            (eventually #(port-open? port)))
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
                     (:more initial))))
      (replay-pending!
       port recipient #{recipient}
       (java.util.concurrent.LinkedBlockingQueue.)
       30000 10000)
      (let [remaining
            (north.message-audience/pending-message-page
             port recipient #{recipient})
            acked
            (:ok
             (north.coord/send-op
              port
              {:op :query
               :query
               {:find "acked"
                :rules
                [{:head {:rel "acked" :args [{:var "e"}]}
                  :body [{:rel "fact"
                          :args [{:var "e"} "acked_by" recipient]}]}]}}))]
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
      (stop-process! daemon))))

;; North independently enforces the page protocol at its own client boundary.
(let [{:keys [port worker]}
      (start-one-shot-server
       (apply str
              (repeat
               (inc north.coord/query-page-response-byte-limit)
               "x")))]
  (check! "North query-page client rejects one byte over the Fram page bound"
          (with-redefs [north.coord/expected-log
                        (constantly "/tmp/query-page-wire.log")]
            (throws-type?
             :coordinator-response-too-large
             #(north.coord/query-page
               port {:find "x" :rules []} 1 nil))))
  @worker)

(let [{:keys [port worker]}
      (start-one-shot-server (pr-str {:error "unknown op"}))]
  (check! "North query-page fails closed against an older coordinator"
          (with-redefs [north.coord/expected-log
                        (constantly "/tmp/query-page-wire.log")]
            (throws-type?
             :query-page-unsupported
             #(north.coord/query-page
               port {:find "x" :rules []} 1 nil))))
  @worker)

(let [failures (remove second @checks)]
  (doseq [[label ok] @checks]
    (println (if ok "  [PASS] " "  [FAIL] ") label))
  (if (seq failures)
    (do
      (println "\npending pagination:" (count failures) "FAILED")
      (System/exit 1))
    (println "\npending pagination:"
             (count @checks) "/" (count @checks) "PASS")))
