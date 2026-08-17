#!/usr/bin/env bb
;; Whole-run lifecycle regression for one independently scheduled maintenance
;; task. The fixture coordinator answers in canonical FRAMRPC v2 through the
;; locked Beagle Store wire namespace; canonical coordination state is never started or
;; mutated.
(require '[babashka.classpath :as classpath]
         '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
                (System/getenv "BEAGLE_STORE_HOME")
                "/home/tom/code/beagle/main/store"))))

(classpath/add-classpath (str fram "/out"))
(require '[store.rpc :as wire]
         '[store.types :as t])

(def maintenance-host (str root "/cli/coordination-maintenance-task-host.clj"))
(def checks (atom []))

(defn check [label value detail]
  (swap! checks conj [label (boolean value) detail]))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn await-port [port]
  (loop [attempt 0]
    (let [connected?
          (try
            (with-open [socket (java.net.Socket.)]
              (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 50)
              true)
            (catch Throwable _ false))]
      (cond
        connected? true
        (< attempt 100) (do (Thread/sleep 20) (recur (inc attempt)))
        :else false))))

(defn common-env [tmp port log lock timeout-ms]
  {"HOME" (.getCanonicalPath (io/file tmp "home"))
   "BEAGLE_STORE_PORT" (str port)
   "BEAGLE_STORE_LOG" (.getCanonicalPath log)
   "NORTH_AGENT_LOGS_DIR" (.getCanonicalPath (io/file tmp "agent-logs"))
   "NORTH_WORKER_HEARTBEAT" (.getCanonicalPath (io/file tmp "heartbeat"))
   "NORTH_MAINTENANCE_TASK_LOCK_PATH" (.getCanonicalPath lock)
   "NORTH_MAINTENANCE_TASK_TIMEOUT_MS" (str timeout-ms)
   "NORTH_MAINTENANCE_TASK_RETRY_MS" "50"
   "NORTH_COORD_CONNECT_TIMEOUT_MS" "50"
   "NORTH_FRAMRPC_READ_TIMEOUT_MS" "10000"})

(defn start-task
  ([environment] (start-task environment true))
  ([environment dry?]
   (apply proc/process
          {:dir root :out :string :err :string :extra-env environment}
          "bb" maintenance-host "stale-concerns"
          (when dry? ["--dry-run"]))))

(defn start-default-lock-task [environment dry?]
  (apply proc/process
         {:dir root :out :string :err :string :extra-env environment}
         "env" "-u" "NORTH_MAINTENANCE_TASK_LOCK_PATH"
         "bb" maintenance-host "stale-concerns"
         (when dry? ["--dry-run"])))

(defn read-exact! [input bytes offset length]
  (loop [position offset remaining length]
    (if (zero? remaining)
      true
      (let [read-count (.read input bytes position remaining)]
        (if (neg? read-count)
          false
          (recur (+ position read-count) (- remaining read-count)))))))

(defn read-request-frame!
  "Read one bounded FRAMRPC v2 request frame. The declared body length lives at
   header offset 14 and is never trusted past the shared 1 MiB bound."
  [input]
  (let [header (byte-array wire/rpc-v2-header-bytes)]
    (when-not (read-exact! input header 0 wire/rpc-v2-header-bytes)
      (throw (ex-info "FRAMRPC request ended inside its header"
                      {:type :rpc-truncated})))
    (let [buffer (doto (java.nio.ByteBuffer/wrap header)
                   (.order java.nio.ByteOrder/LITTLE_ENDIAN)
                   (.position 14))
          body-length (Integer/toUnsignedLong (.getInt buffer))]
      (when (> body-length wire/rpc-v2-max-body-bytes)
        (throw (ex-info "FRAMRPC request exceeds the body limit"
                        {:type :rpc-frame-too-large :body-length body-length})))
      (let [body (byte-array (int body-length))
            frame (byte-array (+ wire/rpc-v2-header-bytes (int body-length)))]
        (when-not (read-exact! input body 0 (int body-length))
          (throw (ex-info "FRAMRPC request ended inside its body"
                          {:type :rpc-truncated})))
        (System/arraycopy header 0 frame 0 wire/rpc-v2-header-bytes)
        (System/arraycopy body 0 frame wire/rpc-v2-header-bytes (int body-length))
        (wire/decode-rpc-frame-v2! frame)))))

(def fixture-served-version 0)

(defn typed-payload
  "Canonical empty payload per operation. nil means the fixture does not serve
   that operation, and the caller answers with a typed error instead."
  [operation]
  (case operation
    :rpc/version wire/rpc-unit
    :rpc/status (wire/rpc-status! :ready 0 :rpc/jvm
                                  (wire/rpc-record! :rpc/result-cache [0 0 0 0]))
    :rpc/scan (wire/rpc-triples! [])
    :rpc/query (wire/rpc-query-rows! [])
    nil))

(defn empty-coordinator-response [request]
  (if-let [payload (typed-payload (t/rpcrequest-op request))]
    {:payload payload}
    {:error (wire/rpc-error!
             :rpc/unsupported-operation false
             "fixture coordinator serves only the lifecycle read operations"
             nil)}))

(defn response-frame
  "Build the v2 response frame. SpaceId and op must echo the request or the
   client rejects the answer as a response/request identity mismatch."
  [frame request {:keys [payload error]}]
  (wire/rpc-response-frame
   (t/rpcframev2-request-id frame)
   (wire/rpc-response!
    (t/rpcrequest-space request)
    (t/rpcrequest-op request)
    fixture-served-version
    (when (and (nil? error) (t/rpcrequest-page request))
      (wire/rpc-page-response! 0 nil true))
    error
    payload)))

(defn start-coordinator
  ([port] (start-coordinator port empty-coordinator-response))
  ([port response-for]
   (let [server (java.net.ServerSocket. port)
         sockets (atom [])
         handlers (atom [])
         stopped (atom false)
         acceptor
         (future
           (while (not @stopped)
             (try
               (let [socket (.accept server)
                     handler
                     (future
                       (swap! sockets conj socket)
                       (try
                         ;; The daemon owns one request per socket.
                         (with-open [socket socket]
                           (let [frame (read-request-frame! (.getInputStream socket))
                                 request (t/rpcframev2-request frame)
                                 output (.getOutputStream socket)]
                             (.write output
                                     (wire/encode-rpc-frame-v2!
                                      (response-frame frame request
                                                      (response-for request))))
                             (.flush output)))
                         (catch Throwable _ nil)))]
                 (swap! handlers conj handler))
               (catch java.net.SocketException _ nil))))]
     {:stop
      (fn []
        (reset! stopped true)
        (try (.close server) (catch Throwable _ nil))
        (doseq [socket @sockets]
          (try (.close ^java.net.Socket socket) (catch Throwable _ nil)))
        (doseq [handler @handlers] (future-cancel handler))
        (future-cancel acceptor))})))

(defn start-blackhole [port]
  (let [server (java.net.ServerSocket. port)
        sockets (atom [])
        stopped (atom false)
        acceptor
        (future
          (while (not @stopped)
            (try
              (swap! sockets conj (.accept server))
              (catch java.net.SocketException _ nil))))]
    {:sockets sockets
     :stop
     (fn []
       (reset! stopped true)
       (try (.close server) (catch Throwable _ nil))
       (doseq [socket @sockets]
         (try (.close ^java.net.Socket socket) (catch Throwable _ nil)))
       (future-cancel acceptor))}))

(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-maintenance-task-lifecycle-"
    (make-array java.nio.file.attribute.FileAttribute 0))))

(try
  (doto (io/file tmp "home") .mkdirs)
  (doto (io/file tmp "agent-logs") .mkdirs)

  (let [port (free-port)
        log (doto (io/file tmp "reconnect.log") (spit ""))
        environment (common-env tmp port log (io/file tmp "reconnect.lock") 5000)
        started (System/nanoTime)
        task (start-task environment)
        _ (Thread/sleep 1200)
        coordinator (start-coordinator port)]
    (try
      (when-not (await-port port)
        (throw (ex-info "fixture coordinator did not start" {})))
      (let [result @task
            elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))
            output (str (:out result) (:err result))]
        (check "disconnect/reconnect completes inside the task deadline"
               (and (zero? (:exit result))
                    (< elapsed-ms 5000)
                    (str/includes? output "coordinator unavailable")
                    (str/includes? output "terminal=completed")
                    (re-find #"attempts=(?:[2-9]|[1-9][0-9]+)\b" output))
               output))
      (finally ((:stop coordinator)))))

  ;; A typed :query-time-limit is retryable, so one answer is absorbed inside the
  ;; FRAMRPC client and proves nothing about the host. Serving exactly
  ;; client-retry-budget of them escapes the same-question budget ONCE, so the
  ;; task completes only because the outer coordinator retry re-asked.
  (let [port (free-port)
        ;; Must stay equal to coord.clj's :max-attempts for the escape to occur.
        client-retry-budget 3
        log (doto (io/file tmp "query-limit.log") (spit ""))
        queries (atom 0)
        response-for
        (fn [request]
          (if (and (= :rpc/query (t/rpcrequest-op request))
                   (<= (swap! queries inc) client-retry-budget))
            {:error (wire/rpc-error!
                     :query-time-limit true
                     "query evaluation stopped: query-time-limit" nil)}
            (empty-coordinator-response request)))
        environment (common-env tmp port log (io/file tmp "query-limit.lock") 5000)
        coordinator (start-coordinator port response-for)]
    (try
      (let [result @(start-task environment)
            output (str (:out result) (:err result))]
        (check "indexed query timeout retries as a coordinator condition"
               (and (zero? (:exit result))
                    (> @queries client-retry-budget)
                    (str/includes? output "coordinator unavailable")
                    (str/includes? output "terminal=completed"))
               output))
      (finally ((:stop coordinator)))))

  (let [port (free-port)
        log (doto (io/file tmp "blocked.log") (spit ""))
        lock (io/file tmp "blocked.lock")
        blackhole (start-blackhole port)
        environment (common-env tmp port log lock 800)
        first (start-task environment false)]
    (try
      (loop [attempt 0]
        (when (and (empty? @(:sockets blackhole)) (< attempt 100))
          (Thread/sleep 10)
          (recur (inc attempt))))
      (let [second @(start-task environment false)
            accepted (count @(:sockets blackhole))
            first-result @first
            first-output (str (:out first-result) (:err first-result))
            second-output (str (:out second) (:err second))]
        (check "same task is single-flight without a second coordinator call"
               (and (zero? (:exit second))
                    (= 1 accepted)
                    (str/includes? second-output "reason=already-running"))
               second-output)
        (check "blocked coordinator reaches a bounded deferred result"
               (and (zero? (:exit first-result))
                    (str/includes? first-output "reason=deadline")
                    (str/includes? first-output "retry-on-next-scheduled-run"))
               first-output))
      (finally ((:stop blackhole)))))

  (let [port (free-port)
        log (doto (io/file tmp "split-lock.log") (spit ""))
        runtime-dir (doto (io/file tmp "runtime") .mkdirs)
        blackhole (start-blackhole port)
        environment
        (assoc (common-env tmp port log (io/file tmp "ignored.lock") 1200)
               "XDG_RUNTIME_DIR" (.getCanonicalPath runtime-dir))
        production (start-default-lock-task environment false)]
    (try
      (loop [attempt 0]
        (when (and (< (count @(:sockets blackhole)) 1) (< attempt 100))
          (Thread/sleep 10)
          (recur (inc attempt))))
      (let [dry-run (start-default-lock-task environment true)]
        (loop [attempt 0]
          (when (and (< (count @(:sockets blackhole)) 2) (< attempt 100))
            (Thread/sleep 10)
            (recur (inc attempt))))
        (let [duplicate-dry @(start-default-lock-task environment true)
              duplicate-production @(start-default-lock-task environment false)
              production-result @production
              dry-result @dry-run
              duplicate-dry-output (str (:out duplicate-dry) (:err duplicate-dry))
              duplicate-production-output
              (str (:out duplicate-production) (:err duplicate-production))]
          (check "production and dry-run have distinct task locks"
                 (and (= 2 (count @(:sockets blackhole)))
                      (zero? (:exit production-result))
                      (zero? (:exit dry-result)))
                 (str (:out production-result) (:err production-result)
                      (:out dry-result) (:err dry-result)))
          (check "duplicate dry-run uses its deterministic task lock"
                 (and (zero? (:exit duplicate-dry))
                      (str/includes? duplicate-dry-output "reason=already-running")
                      (str/includes? duplicate-dry-output
                                     "north-stale-concerns-dry-run.lock"))
                 duplicate-dry-output)
          (check "duplicate production uses its deterministic task lock"
                 (and (zero? (:exit duplicate-production))
                      (str/includes? duplicate-production-output "reason=already-running")
                      (str/includes? duplicate-production-output
                                     "north-stale-concerns.lock"))
                 duplicate-production-output)))
      (finally ((:stop blackhole)))))

  (let [port (free-port)
        log (doto (io/file tmp "invalid.log") (spit ""))
        result
        @(start-task
          (assoc (common-env tmp port log (io/file tmp "invalid.lock") 100)
                 "NORTH_MAINTENANCE_TASK_TIMEOUT_MS" "300000"))
        output (str (:out result) (:err result))]
    (check "invalid lifecycle configuration is nonzero and actionable"
           (and (= 1 (:exit result))
                (str/includes? output "terminal=failed")
                (str/includes? output "check-task-configuration"))
           output))

  (finally
    (doseq [file (reverse (file-seq tmp))]
      (try (io/delete-file file true) (catch Throwable _ nil)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nmaintenance task lifecycle: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
