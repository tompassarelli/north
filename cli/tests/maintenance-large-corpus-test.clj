#!/usr/bin/env bb
;; Production-shape lane maintenance over a large isolated corpus. The fixture carries
;; enough live triples and lanes to make the retired per-lane query-page path
;; repeat whole-corpus Datalog fixpoints through the four-minute sweep budget.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(def fram-source
  (or (System/getenv "FRAM_TEST_CHECKOUT")
      (System/getenv "FRAM_PATH")))
(when-not (seq fram-source)
  (throw
   (ex-info
    "Fram fixture required; set FRAM_TEST_CHECKOUT to a pinned checkout or package libexec/fram"
    {})))
(def fram
  (.getCanonicalPath (io/file fram-source)))
(when-not (and (.isFile (io/file fram "coord_daemon.clj"))
               (.isDirectory (io/file fram "out")))
  (throw (ex-info "Fram fixture lacks coord_daemon.clj or out/" {:fram fram})))
(def maintenance-host (str root "/cli/coordination-maintenance-task-host.clj"))
(def checks (atom []))
(def control-subject-count 1352)

(defn check [label ok detail]
  (swap! checks conj [label (boolean ok) detail]))

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)] (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket.)]
      (.connect socket (java.net.InetSocketAddress. "127.0.0.1" (int port)) 100)
      true)
    (catch Throwable _ false)))

(defn await-up [port]
  (loop [attempt 0]
    (cond
      (port-open? port) true
      ;; Cold log ingestion is deliberately outside the timed sweep assertion.
      ;; Give the interpreted test coordinator room to fold the full fixture.
      (>= attempt 3600) false
      :else (do (Thread/sleep 50) (recur (inc attempt))))))

(defn fact-line [tx subject predicate object]
  (pr-str {:tx tx :op "assert" :l subject :p predicate
           :r object :frame "maintenance-large-corpus-fixture"}))

(defn write-large-log! [file]
  (with-open [writer (io/writer file)]
    (let [tx (atom 0)
          emit! (fn [subject predicate object]
                  (.write writer (fact-line (swap! tx inc) subject predicate object))
                  (.write writer "\n"))]
      ;; 2,210 subjects x 100 values = 221,000 live facts, sharing the
      ;; predicate/object vocabulary as high-volume telemetry does in practice.
      (doseq [subject-index (range 2210)
              value-index (range 100)]
        (emit! (format "@noise:%04d" subject-index)
               "noise"
               (format "value-%03d" value-index)))
      ;; Allocation, reservation, and other worktree-control records carry a
      ;; worktree fact but are not lane entities. The janitor must exclude this
      ;; production-scale population in its seed query, before classification
      ;; can emit one KEEP line per subject.
      (doseq [control-index (range control-subject-count)]
        (let [[prefix kind]
              (case (mod control-index 3)
                0 ["worktree-allocation" "worktree_allocation"]
                1 ["worktree-reservation" "worktree_reservation"]
                ["worktree-control" "control"])
              subject (format "@%s:%04d" prefix control-index)]
          (emit! subject "kind" kind)
          (emit! subject "worktree"
                 (format "/tmp/non-lane-worktree-control-%04d" control-index))))
      ;; Forty-eight old lanes reproduce the incident's multiplicative shape:
      ;; every lane needs the run-candidate lookup. Half have a committed run and
      ;; must remain protected; half are unresolved and must remain reapable.
      (doseq [lane-index (range 48)]
        (let [handle (format "large-corpus-%02d" lane-index)
              lane (str "@agent:" handle)]
          (emit! lane "kind" "lane")
          (emit! lane "spawned_at" "2026-01-01T00:00:00Z")
          (emit! lane "worktree" (format "/tmp/large-corpus-lane-%02d" lane-index))
          (when (odd? lane-index)
            (let [run (str "@run:" handle)]
              (emit! run "agent" handle)
              (emit! run "at" "2026-01-01T00:01:00Z")
              (emit! run "outcome" "ran")
              (emit! run "kind" "run")))))
      {:live-facts @tx :control-subjects control-subject-count})))

(def tmp (.toFile
          (java.nio.file.Files/createTempDirectory
           "north-maintenance-large-corpus-"
           (make-array java.nio.file.attribute.FileAttribute 0))))

(try
  (let [port (free-port)
        log (io/file tmp "facts.log")
        fixture (write-large-log! log)
        live-facts (:live-facts fixture)
        control-subjects (:control-subjects fixture)
        home (doto (io/file tmp "home") .mkdirs)
        agent-logs (doto (io/file tmp "agent-logs") .mkdirs)
        daemon
        (proc/process
         {:dir fram :out :string :err :string
          :env
          (merge
           (dissoc (into {} (System/getenv)) "FRAM_TELEMETRY_LOG")
           {"FRAM_REQUIRE_LOG_FENCE" "1"
            "NORTH_TELEMETRY_PARTITION" "0"})}
         "bb" "-cp" "out" "coord_daemon.clj"
         "serve-flat" (str port) (.getCanonicalPath log))]
    (try
      (when-not (await-up port)
        (try (proc/destroy-tree daemon) (catch Throwable _ nil))
        (throw (ex-info "large-corpus coordinator did not start"
                        {:stdout (deref (:out daemon) 1000 "<still running>")
                         :stderr (deref (:err daemon) 1000 "<still running>")})))
      (let [started (System/nanoTime)
            result
            (proc/shell
             {:dir root :out :string :err :string :continue true
              :extra-env
              {"HOME" (.getCanonicalPath home)
               "FRAM_PORT" (str port)
               "FRAM_LOG" (.getCanonicalPath log)
               "NORTH_TELEMETRY_PARTITION" "0"
               "FRAM_TELEMETRY_LOG" ""
               "NORTH_AGENT_LOGS_DIR" (.getCanonicalPath agent-logs)
               "NORTH_WORKER_HEARTBEAT" (.getCanonicalPath (io/file tmp "heartbeat"))
               "NORTH_MAINTENANCE_TASK_LOCK_PATH" (.getCanonicalPath (io/file tmp "task.lock"))
               ;; Completion, not the timeout terminal, is the regression bar.
               "NORTH_MAINTENANCE_TASK_TIMEOUT_MS" "60000"
               "NORTH_COORD_READ_TIMEOUT_MS" "10000"}}
             "bb" maintenance-host "stale-lanes" "--dry-run")
            elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))
            output (str (:out result) (:err result))
            output-bytes (alength (.getBytes output "UTF-8"))]
        (check "fixture exceeds the observed 221k-fact production scale"
               (> live-facts 221000) (str "live-facts=" live-facts))
        (check "fixture includes the observed non-lane worktree control scale"
               (>= control-subjects 1352)
               (str "control-subjects=" control-subjects))
        (check "large-corpus sweep completes instead of spending the four-minute budget"
               (and (zero? (:exit result))
                    (< elapsed-ms 60000)
                    (str/includes? output "terminal=completed")
                    (not (str/includes? output "terminal=deferred")))
               (str "elapsed-ms=" elapsed-ms "\n" output))
        (check "non-lane worktree controls produce no invalid-subject output flood"
               (and (not (str/includes? output "invalid managed-lane subject"))
                    (< output-bytes 65536))
               (str "output-bytes=" output-bytes "\n" output))
        (check "canonical lane subjects still enter lifecycle classification"
               (and (str/includes? output "@agent:large-corpus-00")
                    (not (str/includes? output "invalid managed-lane subject")))
               output)
        (check "unresolved stale lanes remain reapable"
               (and (str/includes? output ":lanes 24")
                    (str/includes? output "WOULD reap lane @agent:large-corpus-00"))
               output)
        (check "committed-run lanes remain protected from reaping"
               (and (not (str/includes? output "WOULD reap lane @agent:large-corpus-01"))
                    (not (str/includes? output "WOULD reap lane @agent:large-corpus-47")))
               output))
      (finally
        (try (proc/destroy-tree daemon) (catch Throwable _ nil)))))
  (finally
    (doseq [file (reverse (file-seq tmp))]
      (try (io/delete-file file true) (catch Throwable _ nil)))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when-not ok (println (str "        " detail))))
  (println (format "\nmaintenance large corpus: %d / %d PASS"
                   passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
