#!/usr/bin/env bb
;; Real Fram socket gate for north.delivery-evidence-internal/record! under
;; concurrent write traffic (thread 019f9f12-b5fa).
;;
;; The bug: run-bound evidence publication raced every unrelated coordinator
;; write through the global version. The repair serializes only writers for the
;; SAME run/bar with a coordinator lease and commits through its atomic fence.
;; Unrelated churn cannot reject a valid proof, simultaneous same-bar writes
;; remain exactly-once, and a transport failure is distinct from task failure.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file *file*)) "../..")))
(def fram
  (.getCanonicalPath
   (io/file (or (System/getenv "FRAM_PATH")
                "/home/tom/code/beagle/main/branch-core"))))
(when-not (.isFile (io/file fram "bin/fram-server"))
  (throw
   (ex-info
    "Beagle branch-core engine not found; set FRAM_PATH to Beagle's branch-core directory"
    {:fram fram})))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/terminal-projection.clj"))
(load-file (str root "/cli/delivery-evidence-internal.clj"))

;; This disposable harness owns one scratch coordinator. A managed parent may
;; export the live telemetry partition, but no @run:* operation in this process
;; may escape to it.
(alter-var-root #'north.coord/telemetry-partition-enabled?
                (constantly (fn [] false)))

(defn scratch-coordinator-env [port dir log]
  {"FRAM_LOG" (.getPath log)
   "FRAM_SPACE_ID" "north-coordination"
   "FRAM_TELEMETRY_LOG" (.getPath (io/file dir "telemetry.framlog"))
   "NORTH_PORT" (str port)
   "NORTH_TELEMETRY_PARTITION" "0"
   "NORTH_TELEMETRY_PORT" (str port)})

(defn free-port []
  (with-open [socket (java.net.ServerSocket. 0)]
    (.getLocalPort socket)))

(defn port-open? [port]
  (try
    (with-open [socket (java.net.Socket. "127.0.0.1" (int port))]
      true)
    (catch Exception _ false)))

(defn eventually [f]
  (loop [remaining 200]
    (cond
      (try (f) (catch Exception _ false)) true
      (zero? remaining) false
      :else (do (Thread/sleep 25) (recur (dec remaining))))))

(let [port (free-port)
      dir (.toFile
           (java.nio.file.Files/createTempDirectory
            "north-evidence-contention"
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file dir "coordination.framlog")
      subprocess-env (scratch-coordinator-env port dir log)
      daemon
      (proc/process
       {:dir fram :out :string :err :string
        :extra-env {"FRAM_SERVER_RUNTIME" "jvm-dev"
                    "FRAM_SERVER_QUIET" "1"
                    "FRAM_SERVER_XMX" "1g"}}
       (str fram "/bin/fram-server") "serve" (str port)
       (.getCanonicalPath log) "north-coordination")
      checks (atom [])
      check! (fn [label value]
               (swap! checks conj [label (boolean value)]))]
  (alter-var-root #'north.coord/expected-log
                  (constantly (fn [] (.getCanonicalPath log))))
  (try
    (check! "current Fram server starts"
            (eventually
             #(let [status (north.coord/status port)]
                (and (= :ready (:state status))
                     (= "north-coordination" (:space-id status))))))
    (when (= "1" (System/getenv
                   "NORTH_TEST_FORCE_DELIVERY_EVIDENCE_SETUP_FAILURE"))
      (throw
       (ex-info "forced delivery-evidence contention harness setup failure"
                {:type :forced-harness-setup-failure})))

    ;; --- Set up one thread with N*M active done_when bars and reserve one run
    ;; against it, exactly as a managed lane's provider process would. ---
    (let [thread "@thread:evidence-contention"
          writer-count 4
          records-per-writer 4
          bars-by-writer
          (mapv
           (fn [writer-index]
             (mapv
              #(str "bar-" writer-index "-" %)
              (range records-per-writer)))
           (range writer-count))
          bars (vec (mapcat identity bars-by-writer))
          run "@run:evidence-contention"
          reporter "@agent:evidence-contention"
          capability (str/join (repeat 64 "a"))
          capability-sha256 (north.terminal-projection/sha256 capability)]
      (north.coord/append! port thread "title" "evidence contention fixture")
      (doseq [bar bars]
        (north.coord/append! port thread "done_when" bar))
      (north.delivery-evidence-internal/reserve!
       port {"run" run "thread" thread "reporter" reporter
             "capabilitySha256" capability-sha256})

      ;; --- Flood the coordinator with UNRELATED global-version churn from a
      ;; separate writer while every bar is recorded CONCURRENTLY, each from
      ;; its OWN `bb` subprocess — exactly the shape production traffic takes
      ;; (one CLI invocation per `north evidence record`, launched by many
      ;; lanes at once), so no in-process budget atom is shared across bars.
      ;; Before the fix this could exhaust the global CAS budget and refuse a
      ;; perfectly valid, live-reservation write. ---
      (let [running? (atom true)
            churn-writes (atom 0)
            writer
            (future
              (while @running?
                (north.coord/append!
                 port "@unrelated-evidence-churn" "noise"
                 (str (swap! churn-writes inc)))
                (Thread/sleep 15)))
            writer-path (str root "/cli/delivery-evidence-internal.clj")
            submit
            (fn []
              (->> bars-by-writer
                   (pmap
                    (fn [writer-bars]
                      (mapv
                       (fn [bar]
                         (let [request
                               (json/generate-string
                                {"run" run "thread" thread "reporter" reporter
                                 "capability" capability
                                 "bar" bar "observed" (str "exit 0 " bar)})
                               outcome
                               (proc/process
                                {:in request :out :string :err :string
                                 :extra-env subprocess-env}
                                "bb" writer-path (str port) "record")
                               done @outcome]
                           {:bar bar :exit (:exit done) :err (:err done)}))
                       writer-bars)))
                   (doall)
                   (mapcat identity)
                   vec))
            submissions
            (try
              [(submit) (submit)]
              (finally
                (reset! running? false)
                @writer))
            results (vec (mapcat identity submissions))]
        (check! "unrelated churn actually raced the commits"
                (>= @churn-writes 5))
        (check! "N writers x M records plus exact replays all acknowledge"
                (every? #(zero? (:exit %)) results))
        (when-let [failed (seq (filter #(not (zero? (:exit %))) results))]
          (println "  [FAILURES]" (mapv :err failed)))

        (let [stored
              (mapv
               #(json/parse-string %)
               (north.coord/many port run "run_bar_evidence"))
              stored-bars (mapv #(get % "bar") stored)
              projected
              (north.coord/many port thread "bar_evidence")]
          (check! "all N*M evidence records are stored with zero loss"
                  (= (set bars) (set stored-bars)))
          (check! "exact replay is idempotent: zero run-record duplicates"
                  (and (= (count bars) (count stored))
                       (every? #(= 1 %)
                               (vals (frequencies stored-bars)))))
          (check! "exact replay is idempotent: zero thread-projection duplicates"
                  (= (count bars) (count projected))))))

    ;; Same-run/bar first writers are the race the lease must serialize. Every
    ;; subprocess validates before publishing; without the run-scoped lease,
    ;; rival raw records (different recordedAt values) can both land.
    (let [thread "@thread:evidence-same-bar"
          run "@run:evidence-same-bar"
          reporter "@agent:evidence-same-bar"
          capability (str/join (repeat 64 "c"))
          capability-sha256 (north.terminal-projection/sha256 capability)
          bar "one concurrent bar"
          writer-path (str root "/cli/delivery-evidence-internal.clj")]
      (north.coord/append! port thread "title" "same bar fixture")
      (north.coord/append! port thread "done_when" bar)
      (north.delivery-evidence-internal/reserve!
       port {"run" run "thread" thread "reporter" reporter
             "capabilitySha256" capability-sha256})
      (let [request
            (json/generate-string
             {"run" run "thread" thread "reporter" reporter
              "capability" capability
              "bar" bar "observed" "exit 0"})
            results
            (->> (range 8)
                 (pmap
                  (fn [_]
                    @(proc/process
                      {:in request :out :string :err :string
                       :extra-env subprocess-env}
                      "bb" writer-path (str port) "record")))
                 doall)
            stored (north.coord/many port run "run_bar_evidence")]
        (check! "simultaneous first writers all acknowledge"
                (every? #(zero? (:exit %)) results))
        (check! "simultaneous same-bar first writers commit exactly once"
                (= 1 (count stored)))
        (let [wrong-capability
              (try
                (north.delivery-evidence-internal/record!
                 port {"run" run "thread" thread "reporter" reporter
                       "capability" (str/join (repeat 64 "e"))
                       "bar" bar "observed" "exit 0"})
                nil
                (catch Exception error error))
              wrong-reporter
              (try
                (north.delivery-evidence-internal/record!
                 port {"run" run "thread" thread
                       "reporter" "@agent:evidence-impostor"
                       "capability" capability
                       "bar" bar "observed" "exit 0"})
                nil
                (catch Exception error error))]
          (check! "invalid capability and reporter provenance stay rejected"
                  (and (some? wrong-capability)
                       (str/includes?
                        (.getMessage wrong-capability)
                        "run evidence capability mismatch")
                       (some? wrong-reporter)
                       (str/includes?
                        (.getMessage wrong-reporter)
                        "run reservation reporter mismatch")
                       (= 1 (count
                             (north.coord/many
                              port run "run_bar_evidence"))))))))

    ;; A thread contract mutation in the validation/write window is not
    ;; acknowledged as proof. The coordinator-fenced write may already be
    ;; durable, but post-write context confirmation keeps that orphan record
    ;; from becoming this invocation's delivery claim; terminal publication
    ;; independently requires the unchanged accepted contract.
    (let [thread "@thread:evidence-contract-race"
          run "@run:evidence-contract-race"
          reporter "@agent:evidence-contract-race"
          capability (str/join (repeat 64 "d"))
          capability-sha256 (north.terminal-projection/sha256 capability)
          mutated (atom false)]
      (north.coord/append! port thread "title" "contract race fixture")
      (north.coord/append! port thread "done_when" "original bar")
      (north.delivery-evidence-internal/reserve!
       port {"run" run "thread" thread "reporter" reporter
             "capabilitySha256" capability-sha256})
      (let [caught
            (with-redefs
             [north.coord/transact!
              (let [original north.coord/transact!]
                (fn
                  ([target-port actions]
                   (original target-port actions))
                  ([target-port actions options]
                   (when (and (:fence options)
                              (some #(and (= run (:subject %))
                                          (= "run_bar_evidence"
                                             (:predicate %)))
                                    actions)
                              (compare-and-set! mutated false true))
                     (north.coord/append!
                      target-port thread "done_when" "replacement bar")
                     (north.coord/retract!
                      target-port thread "done_when" "original bar"))
                   (original target-port actions options))))]
              (try
                (north.delivery-evidence-internal/record!
                 port {"run" run "thread" thread "reporter" reporter
                       "capability" capability
                       "bar" "original bar" "observed" "exit 0"})
                nil
                (catch Exception error error)))
            active-after
            (north.terminal-projection/canonical-done-when
             (north.delivery-evidence-internal/facts-of port thread))]
        (check! "contract mutation actually lands in the validation/write window"
                (true? @mutated))
        (check! "a raced done-bar contract is rejected, never acknowledged as proof"
                (and (some? caught)
                     (str/includes?
                      (.getMessage caught)
                      "accepted done_when contract changed during the run")))
        (when-not caught
          (println "  [CONTRACT-RACE]" active-after))))

    ;; A proof transport outage is not a verdict about the task and must not
    ;; retain the old retry-the-task language.
    (let [thread "@thread:evidence-contention-exhaustion"
          run "@run:evidence-contention-exhaustion"
          reporter "@agent:evidence-contention-exhaustion"
          capability (str/join (repeat 64 "b"))
          capability-sha256 (north.terminal-projection/sha256 capability)]
      (north.coord/append! port thread "title" "evidence exhaustion fixture")
      (north.coord/append! port thread "done_when" "only-bar")
      (north.delivery-evidence-internal/reserve!
       port {"run" run "thread" thread "reporter" reporter
             "capabilitySha256" capability-sha256})
      ;; Deterministic non-convergence, not a race against real timing: every
      ;; assert-at-version attempt for THIS run is forced to conflict, so the
      ;; retry budget is GUARANTEED to exhaust rather than depending on a
      ;; background writer happening to land in a narrow window.
      (with-redefs
       [north.coord/acquire-lease!
        (let [original north.coord/acquire-lease!]
          (fn [target-port resource holder ttl-ms]
            (if (= (north.delivery-evidence-internal/evidence-lease-resource
                    run "only-bar")
                   resource)
              {:reject :held}
              (original target-port resource holder ttl-ms))))
        north.delivery-evidence-internal/evidence-lease-wait-budget-ms 50]
        (let [caught
              (try
                (north.delivery-evidence-internal/record!
                 port {"run" run "thread" thread "reporter" reporter
                       "capability" capability
                       "bar" "only-bar" "observed" "exit 0"})
                nil
                (catch Exception error error))]
          (check! "an unavailable proof transport throws"
                  (some? caught))
          (check! "the refusal names proof transport without task retry language"
                  (and (some? caught)
                       (str/includes?
                        (.getMessage caught)
                        "PROOF_TRANSPORT_FAILURE:")
                       (str/includes?
                        (.getMessage caught)
                        "do not repeat the task")
                       (not (str/includes?
                             (.getMessage caught)
                             "RETRYABLE:")))))))

    (finally
      (proc/destroy-tree daemon)
      (doseq [[label ok?] @checks]
        (println (if ok? "  [OK]" "  [FAIL]") label))))
  (System/exit
   (if (every? second @checks) 0 1)))
