#!/usr/bin/env bb
(require '[clojure.java.io :as io] '[clojure.string :as str])
(load-file "cli/shipped-report.clj")

(def checks (atom []))
(defn check! [label ok?] (swap! checks conj [label (boolean ok?)]))
(def now (java.time.Instant/parse "2026-08-03T12:00:00Z"))
(def runs {"@run:terminal" [["kind" "run"] ["at" "2026-08-03T11:00:00Z"] ["process_outcome" "ran"] ["delivery_outcome" "reported"] ["thread" "@thread-one"] ["thread_provenance" "exact"] ["provider" "anthropic"] ["duration_ms" "120000"]
                            ["routing_applied_topology" "worker"] ["routing_applied_task_grade" "senior"] ["routing_applied_tier" "standard"] ["routing_applied_reasoning" "high"] ["routing_applied_posture" "deliver"]
                            ["learning_mode" "learning"] ["learning_arm" "explore"] ["learning_axis" "effort"] ["learning_arm_id" "high"] ["learning_experiment_id" "exp-fixture"]
                            ["estimate_classification" "over"] ["estimate_ratio" "1.5"] ["estimate_delta_ms" "60000"]]
           "@run:retry" [["kind" "run"] ["at" "2026-08-03T10:00:00Z"] ["process_outcome" "ran"] ["delivery_outcome" "unverified"] ["thread" "@thread-one"] ["provider" "openai"] ["composition_id" "implementer"] ["duration_ms" "180000"] ["retry_of_run" "@run:infrastructure"] ["retry_attempt" "1"]]
           "@run:infrastructure" [["kind" "run"] ["at" "2026-08-03T09:00:00Z"] ["process_outcome" "provider_error"] ["delivery_outcome" "blocked"] ["thread" "@thread-one"] ["provider" "openai"] ["duration_ms" "90000"]]
           "@run:capability" [["kind" "run"] ["at" "2026-08-03T08:00:00Z"] ["process_outcome" "blocked_preflight"] ["delivery_outcome" "blocked"] ["thread" "@thread-two"] ["provider" "openai"] ["duration_ms" "60000"]]
           "@run:unresolved" [["kind" "run"] ["at" "2026-08-03T07:00:00Z"] ["provider" "openai"]]
           "@run:old" [["kind" "run"] ["at" "2026-07-20T12:00:00Z"] ["process_outcome" "ran"] ["thread" "@thread-one"] ["provider" "openai"]]})
(def threads {"@thread-one" [["title" "Ship fixture"] ["outcome" "landed in abcdef1"] ["progress" "commit 7654321"]]
              "@thread-two" [["title" "Unverified fixture"] ["outcome" "landed"]]})
(with-redefs [north.shipped-report/run-subjects (fn [_] (vec (keys runs)))
              north.shipped-report/exact-facts-many
              (fn [_ _ subjects]
                (into {}
                      (map (fn [subject]
                             [subject
                              (north.shipped-report/rows->facts
                               (or (get runs subject) (get threads subject)))])
                           subjects)))]
  (let [rows (north.shipped-report/report-rows 7977 (.minus now (java.time.Duration/ofHours 24)) now)
        rendered (north.shipped-report/render rows (.minus now (java.time.Duration/ofHours 24)) now)]
    (check! "windowed run projection excludes old facts" (= 5 (count rows)))
    (check! "render labels its requested window without a volatile clock" (str/starts-with? rendered "RUNS — past 24h\n"))
    (check! "render groups native and managed harnesses" (and (str/includes? rendered "native-claude") (str/includes? rendered "managed-codex")))
    (check! "render carries canonical staffing learning and estimate facts" (and (str/includes? rendered "staffing: worker / senior / standard / high / deliver") (str/includes? rendered "learning: learning/explore/effort/high · exp-fixture") (str/includes? rendered "estimate: over · 1.5x · +1m")))
    (check! "render keeps terminal infrastructure and capability outcomes distinct" (and (str/includes? rendered "provider_error · Ship fixture") (str/includes? rendered "blocked_preflight · Unverified fixture")))
    (check! "render carries retry lineage and unresolved attribution without inference" (and (str/includes? rendered "retry: 1 of @run:infrastructure") (str/includes? rendered "unresolved · (unattributed)") (str/includes? rendered "run: @run:unresolved · thread: unavailable")))
    (check! "render includes title, outcome, and fact-referenced commits" (and (str/includes? rendered "Ship fixture") (str/includes? rendered "outcome: landed in abcdef1") (str/includes? rendered "commits: 7654321, abcdef1")))))
(doseq [[label ok?] @checks] (println (if ok? "PASS" "FAIL") label))
(System/exit (if (every? second @checks) 0 1))
