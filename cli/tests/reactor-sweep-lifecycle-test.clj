#!/usr/bin/env bb
;; Whole-run lifecycle regression for the production north-reactor sweep-once
;; entrypoint. Fixtures are isolated: an empty temporary log, a strict
;; empty-corpus wire stub, and a planted blackhole socket. Canonical Fram is
;; never started or mutated.
(require '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def reactor (str root "/cli/north-reactor.clj"))
(def checks (atom []))

(defn check [label ok detail]
  (swap! checks conj [label ok detail]))

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
   "NORTH_REACTOR_HEARTBEAT" (.getCanonicalPath (io/file tmp "heartbeat"))
   "NORTH_REACTOR_SWEEP_LOCK_PATH" (.getCanonicalPath lock)
   "NORTH_REACTOR_SWEEP_TIMEOUT_MS" (str timeout-ms)
   "NORTH_REACTOR_SWEEP_RETRY_MS" "50"
   "NORTH_COORD_CONNECT_TIMEOUT_MS" "50"
   "NORTH_COORD_READ_TIMEOUT_MS" "10000"})

(defn start-sweep
  ([environment] (start-sweep environment true))
  ([environment dry?]
   (apply proc/process
          {:dir root :out :string :err :string :extra-env environment}
          "bb" reactor "sweep-once" (when dry? ["--dry-run"]))))

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
      :show {:ok []}
      {:ok true :version 0})))

(defn start-coordinator [port _log]
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
                                    (str (pr-str
                                          (empty-coordinator-response
                                           (edn/read-string line)))
                                         "\n"))
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
       (future-cancel acceptor))}))

(defn start-blackhole [port]
  (let [server (java.net.ServerSocket. port)
        sockets (atom [])
        stopped (atom false)
        acceptor
        (future
          (while (not @stopped)
            (try
              (let [socket (.accept server)]
                (swap! sockets conj socket))
              (catch java.net.SocketException _ nil))))]
    {:sockets sockets
     :stop (fn []
             (reset! stopped true)
             (try (.close server) (catch Throwable _ nil))
             (doseq [socket @sockets]
               (try (.close ^java.net.Socket socket) (catch Throwable _ nil)))
             (future-cancel acceptor))}))

(defn pid-alive? [pid]
  (boolean
   (when pid
     (some-> (java.lang.ProcessHandle/of (long pid))
             (.orElse nil)
             (.isAlive)))))

(defn await-file [file timeout-ms]
  (let [deadline (+ (System/nanoTime) (* 1000000 timeout-ms))]
    (loop []
      (cond
        (.isFile ^java.io.File file) true
        (>= (System/nanoTime) deadline) false
        :else (do (Thread/sleep 10) (recur))))))

(defn await-pid-gone [pid timeout-ms]
  (let [deadline (+ (System/nanoTime) (* 1000000 timeout-ms))]
    (loop []
      (cond
        (not (pid-alive? pid)) true
        (>= (System/nanoTime) deadline) false
        :else (do (Thread/sleep 10) (recur))))))

(defn read-pid [file]
  (try (parse-long (str/trim (slurp file))) (catch Throwable _ nil)))

(def tmp (.toFile
          (java.nio.file.Files/createTempDirectory
           "north-reactor-sweep-lifecycle-"
           (make-array java.nio.file.attribute.FileAttribute 0))))

(try
  (doto (io/file tmp "home") .mkdirs)
  (doto (io/file tmp "agent-logs") .mkdirs)

  ;; The sweep begins while the coordinator is down, observes connection
  ;; refusal, and completes after the isolated coordinator comes online.
  (let [port (free-port)
        log (io/file tmp "reconnect.log")
        lock (io/file tmp "reconnect.lock")
        environment (common-env tmp port log lock 5000)
        _ (spit log "")
        started (System/nanoTime)
        sweep (start-sweep environment)
        _ (Thread/sleep 1200)
        daemon (start-coordinator port log)]
    (try
      (when-not (await-port port)
        ((:stop daemon))
        (throw (ex-info "throwaway coordinator did not start" {})))
      (let [result @sweep
            elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))
            output (str (:out result) (:err result))]
        (check "disconnect/reconnect completes inside the whole-run deadline"
               (and (zero? (:exit result)) (< elapsed-ms 5000)
                    (str/includes? output "coordinator unavailable")
                    (str/includes? output "terminal=completed")
                    (re-find #"attempts=(?:[2-9]|[1-9][0-9]+)\b" output))
               output))
      (finally
        ((:stop daemon)))))

  ;; Core liveness work completes before the daily audit. A TERM-resistant audit
  ;; with a child and grandchild must lose to the aggregate deadline, leave the
  ;; real core heartbeat behind, and be fully gone before terminal output. The
  ;; attention phase is after the audit and therefore must never start.
  (let [port (free-port)
        log (doto (io/file tmp "hung-audit.log") (spit ""))
        lock (io/file tmp "hung-audit.lock")
        heartbeat (io/file tmp "heartbeat")
        audit (io/file tmp "hung-clock-audit.sh")
        parent-pid-file (io/file tmp "audit-parent.pid")
        child-pid-file (io/file tmp "audit-child.pid")
        grandchild-pid-file (io/file tmp "audit-grandchild.pid")
        _ (spit audit
                (str "#!/bin/sh\n"
                     "set -eu\n"
                     "printf '%s\\n' \"$$\" > \"$NORTH_TEST_AUDIT_PARENT_PID\"\n"
                     "sh -c 'trap \"\" TERM; "
                     "printf \"%s\\\\n\" \"$$\" > \"$NORTH_TEST_AUDIT_CHILD_PID\"; "
                     "sleep 1000 & grandchild=$!; "
                     "printf \"%s\\\\n\" \"$grandchild\" > \"$NORTH_TEST_AUDIT_GRANDCHILD_PID\"; "
                     "wait' &\n"
                     "trap '' TERM\n"
                     "wait\n"))
        _ (.setExecutable audit true)
        environment
        (merge
         (common-env tmp port log lock 6000)
         {"NORTH_REACTOR_CLOCK_AUDIT_BIN" (.getCanonicalPath audit)
          "NORTH_REACTOR_CLOCK_AUDIT_TIMEOUT_MS" "30000"
          "NORTH_TEST_AUDIT_PARENT_PID" (.getCanonicalPath parent-pid-file)
          "NORTH_TEST_AUDIT_CHILD_PID" (.getCanonicalPath child-pid-file)
          "NORTH_TEST_AUDIT_GRANDCHILD_PID" (.getCanonicalPath grandchild-pid-file)})
        daemon (start-coordinator port log)]
    (try
      (when-not (await-port port)
        ((:stop daemon))
        (throw (ex-info "hung-audit coordinator did not start" {})))
      (.delete heartbeat)
      (let [started (System/nanoTime)
            sweep (start-sweep environment false)
            result @sweep
            elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))
            output (str (:out result) (:err result))
            pid-files-ready?
            (every? #(await-file % 1000)
                    [parent-pid-file child-pid-file grandchild-pid-file])
            pids (mapv read-pid
                       [parent-pid-file child-pid-file grandchild-pid-file])
            all-gone? (and pid-files-ready?
                           (every? #(await-pid-gone % 3000) pids))
            heartbeat-record
            (try (edn/read-string (slurp heartbeat)) (catch Throwable _ nil))]
        (check "hung audit loses to aggregate deadline with explicit clean deferral"
               (and (zero? (:exit result))
                    (< elapsed-ms 10000)
                    (str/includes? output "terminal=deferred reason=deadline")
                    (str/includes? output "stage=clock-audit")
                    (str/includes? output "child_cleanup=1/1 surviving=0"))
               output)
        (check "registered audit parent, child, and grandchild are gone before terminal"
               (and pid-files-ready? (every? some? pids) all-gone?)
               (str "pids=" (pr-str pids) " output=" output))
        (check "completed core sweep publishes heartbeat despite audit deferral"
               (and (map? heartbeat-record)
                    (string? (:at heartbeat-record))
                    (map? (:details heartbeat-record))
                    (map? (get-in heartbeat-record [:details :worktrees])))
               (str "heartbeat=" (pr-str heartbeat-record) " output=" output))
        (check "aggregate deadline starts no post-audit phase"
               (and (not (str/includes? output "attention reconcile"))
                    (not (str/includes? output "[sweep] rebuild window")))
               output))
      (finally
        ((:stop daemon)))))

  ;; A server that accepts but never answers defeats the coordinator's normal
  ;; read path. The whole-run deadline must still terminate the process cleanly.
  ;; While that run holds the lock, a second invocation must not reach the socket.
  (let [port (free-port)
        log (doto (io/file tmp "blocked.log") (spit ""))
        lock (io/file tmp "blocked.lock")
        blackhole (start-blackhole port)
        environment (common-env tmp port log lock 800)
        started (System/nanoTime)
        first-sweep (start-sweep environment)]
    (try
      (loop [attempt 0]
        (when (and (empty? @(:sockets blackhole)) (< attempt 100))
          (Thread/sleep 10)
          (recur (inc attempt))))
      (let [second-started (System/nanoTime)
            second-result @(start-sweep environment)
            second-elapsed-ms (long (/ (- (System/nanoTime) second-started) 1000000))
            second-output (str (:out second-result) (:err second-result))
            accepted-before-first-exit (count @(:sockets blackhole))
            first-result @first-sweep
            first-elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))
            first-output (str (:out first-result) (:err first-result))]
        (check "concurrent sweep is deferred without reaching coordinator"
               (and (zero? (:exit second-result)) (< second-elapsed-ms 500)
                    (= 1 accepted-before-first-exit)
                    (str/includes? second-output
                                   "terminal=deferred reason=already-running"))
               second-output)
        (check "blocked coordinator has bounded clean terminal result"
               (and (zero? (:exit first-result)) (< first-elapsed-ms 2000)
                    (str/includes? first-output
                                   "terminal=deferred reason=deadline")
                    (str/includes? first-output
                                   "action=retry-on-next-scheduled-run"))
               first-output))
      (finally ((:stop blackhole)))))

  ;; A bad lifecycle setting is a real operator/configuration failure, not a
  ;; transient coordinator condition, and must remain actionable + nonzero.
  (let [port (free-port)
        log (doto (io/file tmp "invalid.log") (spit ""))
        lock (io/file tmp "invalid.lock")
        result @(start-sweep
                 (assoc (common-env tmp port log lock 100)
                        "NORTH_REACTOR_SWEEP_TIMEOUT_MS" "300000"))
        output (str (:out result) (:err result))]
    (check "invalid timeout is a nonzero actionable terminal failure"
           (and (= 1 (:exit result))
                (str/includes? output "terminal=failed")
                (str/includes? output "check-sweep-lifecycle-configuration"))
           output))

  (finally
    (doseq [file (reverse (file-seq tmp))]
      (try (io/delete-file file true) (catch Throwable _ nil)))))

(let [priority-test (str root "/cli/tests/reactor-priority-test.clj")
      result (proc/shell {:out :string :err :string :continue true}
                         "bb" priority-test)
      output (str (:out result) (:err result))]
  (check "rebuild queue-priority contract"
         (and (zero? (:exit result))
              (str/includes? output "reactor priority: 3 / 3 PASS"))
         output))

(let [heal-test (str root "/cli/tests/reactor-heal-projection-test.clj")
      result (proc/shell {:out :string :err :string :continue true}
                         "bb" heal-test)
      output (str (:out result) (:err result))]
  (check "coordination-only auto-heal contract"
         (and (zero? (:exit result))
              (str/includes? output "reactor heal projection: 2 / 2 PASS"))
         output))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nreactor sweep lifecycle: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
