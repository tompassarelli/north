#!/usr/bin/env bb
;; Offline cohort report: exact evaluation evidence enters comparisons; discovery
;; and incomplete receipts remain visible with explicit exclusion reasons.
(require '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def scratch (.toFile (java.nio.file.Files/createTempDirectory
                       "north-learning-report-"
                       (make-array java.nio.file.attribute.FileAttribute 0))))
(def coord (io/file scratch "coordination.log"))
(def telem (io/file scratch "telemetry.log"))
(def digest-a (apply str (repeat 64 "a")))
(def digest-b (apply str (repeat 64 "b")))
(def digest-c (apply str (repeat 64 "c")))
(def checks (atom []))

(defn check! [label pass?] (swap! checks conj [label (boolean pass?)]))
(defn fact! [file subject predicate object]
  (spit file (str (pr-str {:op "assert" :l subject :p predicate :r object}) "\n") :append true))

(defn verified-thread! [thread]
  (fact! coord thread "title" (str "Thread " thread))
  (fact! coord thread "done_when" "focused probe passes")
  (fact! coord thread "bar_evidence" "focused probe passes → exit 0")
  (fact! coord thread "outcome" "landed"))

(defn learning-run!
  [{:keys [run thread episode arm axis arm-id evidence-mode environment-coverage
           tokens duration struggle authority-surface]}]
  (doseq [[predicate object]
          [["kind" "run"] ["agent" (str "agent-" episode)] ["thread" thread]
           ["at" "2026-08-01T00:00:00Z"] ["outcome" "ran"]
           ["process_outcome" "ran"] ["delivery_outcome" "unverified"]
           ["delivery_reason" "provider process completed without external delivery proof"]
           ["tokens" (str tokens)] ["duration_ms" (str duration)] ["num_turns" "3"]
           ["learning_assignment_version" "north-learning-assignment:v1"]
           ["learning_policy_version" "north-learning-policy:v1"]
           ["learning_policy_sha256" digest-a] ["learning_mode" "learning"]
           ["learning_evidence_mode" evidence-mode]
           ["learning_experiment_id" "ordinary-ops-v1"]
           ["learning_episode_id" episode]
           ["learning_task_signature_sha256" digest-b]
           ["learning_task_signature_coverage" "exact"] ["learning_risk" "p1"]
           ["learning_arm" arm] ["learning_axis" axis] ["learning_arm_id" arm-id]
           ["learning_propensity" "0.250000000000"]
           ["learning_explore_propensity" "0.250000000000"]
           ["learning_narrowing_reason"
            (if (= arm "control") "assignment:control" (str "explore:" axis ":" arm-id))]
           ["learning_baseline_sha256" digest-a] ["learning_options_sha256" digest-b]
           ["learning_assignment_sha256" digest-c]
           ["prompt_receipt_version" "north-prompt-receipt:v1"]
           ["prompt_receipt_sha256" digest-a] ["prompt_wire_sha256" digest-b]
           ["prompt_receipt_coverage" "exact"]
           ["environment_receipt_version" "north-environment-receipt:v1"]
           ["environment_receipt_sha256" digest-b]
           ["environment_receipt_coverage" environment-coverage]
           ["available_skill_catalog_sha256" digest-c]
           ["activated_resource_closure_sha256" digest-a]
           ["run_envelope_version" "north-run-envelope:v1"]
           ["run_envelope_sha256" digest-c]]]
    (fact! telem run predicate object))
  (if authority-surface
    (do
      (fact! telem run "execution_source" "north-managed")
      (fact! telem run "effective_authority_provider" "openai")
      (fact! telem run "effective_authority_capability" "filesystem.read")
      (fact! telem run "effective_authority_capability" "filesystem.search")
      (when (#{"graph" "text"} authority-surface)
        (fact! telem run "effective_authority_capability" "filesystem.write")
        (fact! telem run "effective_authority_capability" "shell"))
      (when (= "graph" authority-surface)
        (fact! telem run "effective_authority_capability" "graph-authoring.fram")
        (fact! telem run "mcp_activity_source" "codex-app-server:item-completed")
        (fact! telem run "mcp_activity_coverage" "exact")
        (fact! telem run "mcp_actual_calls" "2")
        (fact! telem run "mcp_actual_tool"
               "{\"server\":\"fram\",\"tool\":\"tell\",\"count\":2}")))
    (do
      (fact! telem run "execution_source" "provider-native")
      (fact! telem run "authoring_authority_surface" "unknown")
      (fact! telem run "authoring_authority_surface_coverage" "unknown")))
  (when struggle
    (fact! telem run "struggle" "no_progress")
    (fact! telem run "error_count" "2"))
  ;; This is deliberately absent from the report projection.
  (fact! telem run "raw_prompt" "PRIVATE PROMPT SENTINEL"))

(try
  (doseq [thread ["@thread-control" "@thread-explore" "@thread-incomplete"]]
    (verified-thread! thread))
  (learning-run! {:run "@run:10000000-0000-4000-8000-000000000001"
                  :thread "thread-control" :episode "episode-control"
                  :arm "control" :axis "control" :arm-id "control"
                  :evidence-mode "evaluation" :environment-coverage "exact"
                  :tokens 100 :duration 1000 :authority-surface "text"})
  (learning-run! {:run "@run:10000000-0000-4000-8000-000000000002"
                  :thread "thread-explore" :episode "episode-explore"
                  :arm "explore" :axis "prompt" :arm-id "compact-v1"
                  :evidence-mode "evaluation" :environment-coverage "exact"
                  :tokens 80 :duration 900 :struggle true :authority-surface "graph"})
  (learning-run! {:run "@run:10000000-0000-4000-8000-000000000003"
                  :thread "thread-incomplete" :episode "episode-discovery"
                  :arm "control" :axis "control" :arm-id "control"
                  :evidence-mode "discovery" :environment-coverage "unknown"
                  :tokens 120 :duration 1100})

  (let [result (process/shell
                {:out :string :err :string :continue true
                 :extra-env {"FRAM_LOG" (.getPath coord)
                             "FRAM_TELEMETRY_LOG" (.getPath telem)}}
                "bb" (str root "/cli/routing-report.clj")
                "report" "learning" "--json")
        data (when (zero? (:exit result))
               (json/parse-string (str/trim (:out result)) true))]
    (check! "learning report exits zero" (zero? (:exit result)))
    (check! "only exact evaluation observations enter cohorts"
            (and (= 3 (:runs data)) (= 2 (:eligibleRuns data))
                 (= 1 (:excludedRuns data)) (= 2 (count (:cohorts data)))))
    (check! "control and one-axis treatment become an evaluation-ready comparison"
            (= {:controlRuns 1 :exploratoryRuns 1 :axes ["prompt"]
                :comparable true :reason "evaluation-ready"}
               (select-keys (first (:comparisonGroups data))
                            [:controlRuns :exploratoryRuns :axes :comparable :reason])))
    (check! "unknown environment and discovery are explicit exclusions"
            (and (= 1 (get-in data [:exclusions :not-evaluation]))
                 (= 1 (get-in data [:exclusions :environment-receipt-not-exact]))
                 (= 1 (get-in data [:exclusions :receipt-envelope-incomplete-or-invalid]))))
    (check! "cohort metrics preserve exact wall/token evidence and struggle"
            (let [prompt (first (filter #(= "compact-v1" (:armId %)) (:cohorts data)))]
              (and (= 80 (:tokens prompt)) (= "exact" (:tokenEvidence prompt))
                   (= 900 (:wallMilliseconds prompt))
                   (= 1 (:struggleRuns prompt)) (= 1 (:barEvidencedRuns prompt)))))
    (check! "authoring report separates exact authority from observed graph activation"
            (let [authoring (:authoringObservability data)
                  surfaces (into {} (map (juxt :surface identity)
                                         (:authoritySurfaces authoring)))]
              (and (= 2 (:exactAuthorityRuns authoring))
                   (= 1 (:authorityExcludedRuns authoring))
                   (= 1 (get-in surfaces ["graph" :runs]))
                   (= 1 (get-in surfaces ["text" :runs]))
                   (= 1 (get-in authoring [:activation :graphInvocationRuns]))
                   (= 2 (get-in authoring [:activation :graphMutationInvocations]))
                   (nil? (get-in authoring [:activation :textInvocationRuns]))
                   (= "blocked" (get-in authoring [:exploration :status])))))
    (check! "report emits content identities but never raw prompt material"
            (and (str/includes? (:out result) digest-a)
                 (not (str/includes? (:out result) "PRIVATE PROMPT SENTINEL")))))

  (finally
    (doseq [file (reverse (file-seq scratch))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label pass?] results]
    (println (format "  [%s] %s" (if pass? "PASS" "FAIL") label)))
  (println (format "\nlearning report: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
