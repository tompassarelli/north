#!/usr/bin/env bb
;; Whole-run lifecycle regression for one independently scheduled maintenance
;; task. Fixtures use a private wire stub; canonical coordination state is never
;; started or mutated.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
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
   "FRAM_PORT" (str port)
   "FRAM_LOG" (.getCanonicalPath log)
   "NORTH_AGENT_LOGS_DIR" (.getCanonicalPath (io/file tmp "agent-logs"))
   "NORTH_WORKER_HEARTBEAT" (.getCanonicalPath (io/file tmp "heartbeat"))
   "NORTH_MAINTENANCE_TASK_LOCK_PATH" (.getCanonicalPath lock)
   "NORTH_MAINTENANCE_TASK_TIMEOUT_MS" (str timeout-ms)
   "NORTH_MAINTENANCE_TASK_RETRY_MS" "50"
   "NORTH_COORD_CONNECT_TIMEOUT_MS" "50"
   "NORTH_COORD_READ_TIMEOUT_MS" "10000"})

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

(defn empty-coordinator-response [envelope]
  (let [request (:request envelope)]
    (case (:op request)
      :version {:version 0}
      :resolved {:value nil :members 0 :ambiguous? false :values [] :version 0}
      :query (if (:query-max-rows request)
               {:ok [] :version 0 :engine "index"}
               {:ok []})
      :query-page {:ok [] :more false :next nil :version 0 :engine "scan"}
      :facts {:facts [] :version 0}
      :show {:rows [] :version 0}
      {:ok true :version 0})))

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
                         (with-open [socket socket
                                     reader (io/reader socket)
                                     writer (io/writer socket)]
                           (when-let [line (.readLine ^java.io.BufferedReader reader)]
                             (.write ^java.io.Writer writer
                                     (str (pr-str (response-for (edn/read-string line))) "\n"))
                             (.flush ^java.io.Writer writer)))
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

  (let [port (free-port)
        log (doto (io/file tmp "query-limit.log") (spit ""))
        stopped-once? (atom false)
        response-for
        (fn [envelope]
          (let [request (:request envelope)]
            (if (and (= :query (:op request))
                     (:query-max-rows request)
                     (compare-and-set! stopped-once? false true))
              {:error ["query evaluation stopped: query-time-limit"]
               :code :query-time-limit :version 0 :engine "index"}
              (empty-coordinator-response envelope))))
        environment (common-env tmp port log (io/file tmp "query-limit.lock") 5000)
        coordinator (start-coordinator port response-for)]
    (try
      (let [result @(start-task environment)
            output (str (:out result) (:err result))]
        (check "indexed query timeout retries as a coordinator condition"
               (and (zero? (:exit result))
                    @stopped-once?
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
