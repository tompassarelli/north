#!/usr/bin/env bb
(require '[babashka.classpath :as cp]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file")))
            "../..")))
(def store
  (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
      (System/getenv "BEAGLE_STORE_HOME")
      "/home/tom/code/beagle/main/store"))
(cp/add-classpath (str root "/out:" store "/out"))
(load-file (str root "/cli/learning-compare.clj"))

(def checks (atom []))
(defn check [label value]
  (swap! checks conj [label (boolean value)]))
(defn sha [value] (north.terminal-projection/sha256 value))

(defn assignment-facts
  [{:keys [axis arm-id assignment-id episode-id evidence-mode
           propensity explore-propensity baseline-sha256 options-sha256]
    :or {axis "control"
         arm-id "control"
         assignment-id (sha "assignment-control")
         episode-id "episode-control"
         evidence-mode "evaluation"
         propensity "0.800000000000"
         explore-propensity "0.200000000000"
         baseline-sha256 (sha "baseline")
         options-sha256 (sha "options")}}]
  {"learning_assignment_version" "north-learning-assignment:v1"
   "learning_policy_version" "north-learning-policy:v1"
   "learning_policy_sha256" (sha "policy")
   "learning_mode" "learning"
   "learning_evidence_mode" evidence-mode
   "learning_experiment_id" "exp-fixture"
   "learning_episode_id" episode-id
   "learning_task_signature_sha256" (sha "exact-task")
   "learning_task_signature_coverage" "exact"
   "learning_risk" "p1"
   "learning_arm" (if (= axis "control") "control" "explore")
   "learning_axis" axis
   "learning_arm_id" arm-id
   "learning_propensity" propensity
   "learning_explore_propensity" explore-propensity
   "learning_narrowing_reason" (if (= axis "control")
                                  "control:eligible"
                                  (str "explore:" axis ":" arm-id))
   "learning_baseline_sha256" baseline-sha256
   "learning_options_sha256" options-sha256
   "learning_assignment_sha256" assignment-id})

(def exact-receipts
  {"prompt_receipt_version" "north-prompt-receipt:v1"
   "prompt_receipt_sha256" (sha "prompt-receipt")
   "prompt_wire_sha256" (sha "prompt-wire")
   "prompt_receipt_coverage" "exact"
   "environment_receipt_version" "north-environment-receipt:v1"
   "environment_receipt_sha256" (sha "environment-receipt")
   "environment_receipt_coverage" "exact"
   "available_skill_catalog_sha256" (sha "available-skills")
   "activated_resource_closure_sha256" (sha "activated-resources")
   "run_envelope_version" "north-run-envelope:v1"
   "run_envelope_sha256" (sha "run-envelope")
   "routing_admission_receipt_version" "1"
   "routing_request_sha256" (sha "routing-request")
   "routing_policy_sha256" (sha "routing-policy")
   "routing_applied_task_grade" "medium"
   "routing_applied_topology" "single"
   "routing_applied_tier" "standard"
   "routing_applied_reasoning" "medium"
   "routing_applied_posture" "balanced"})

(defn proof-facts [run]
  (let [suffix (str/replace (subs run 5) ":" "-")
        agent (str "lane-" suffix)
        reporter (str "@agent:" agent)
        thread (str "@thread:" suffix)
        bars ["tests pass"]
        record (sorted-map
                "bar" "tests pass"
                "observed" "24/24"
                "recordedAt" "2026-08-01T00:01:00Z"
                "reporter" reporter
                "run" run
                "thread" thread
                "version" "north:run-bar-evidence:v1")
        record-raw (json/generate-string record)
        delivery (json/generate-string
                  (array-map
                   "version" "north:done-bars:v2"
                   "run" run
                   "thread" thread
                   "reporter" reporter
                   "contractOrigin" "accepted"
                   "baselineDoneWhen" bars
                   "doneWhen" bars
                   "matches" [(array-map "bar" "tests pass"
                                         "evidence" [record])]))
        reservation (sorted-map
                     "run_capability_sha256" (sha "capability")
                     "run_reservation_agent" reporter
                     "run_reservation_contract_origin" "accepted"
                     "run_reservation_done_when" (json/generate-string bars)
                     "run_reservation_thread" thread
                     "run_reservation_version" "north:run-reservation:v1"
                     "run_reserved_at" "2026-08-01T00:00:00Z")]
    (merge
     {"kind" "run"
      "thread" thread
      "agent" agent
      "outcome" "ran"
      "process_outcome" "ran"
      "delivery_outcome" "reported"
      "delivery_reason" "complete_run_scoped_done_bar_evidence_self_reported"
      "delivery_evidence" delivery
      "delivery_evidence_sha256" (sha delivery)
      "run_bar_evidence" record-raw
      "run_reservation_manifest_sha256"
      (north.terminal-projection/run-reservation-manifest-sha256 reservation)}
     reservation)))

(defn complete-run
  [run {:keys [duration tokens token-status retry-of retry-attempt
               reviewer-duration reviewer-tokens reviewer-token-status]
        :or {duration "100" tokens "10" token-status "exact"}
        :as options}]
  (cond->
   (merge (assignment-facts options)
          exact-receipts
          (proof-facts run)
          {"duration_ms" duration
           "usage_total_status" token-status})
    tokens (assoc "tokens" tokens)
    reviewer-duration
    (assoc "shadow_reviewer_duration_ms" reviewer-duration)
    reviewer-tokens (assoc "shadow_reviewer_tokens" reviewer-tokens)
    reviewer-token-status
    (assoc "shadow_reviewer_usage_status" reviewer-token-status)
    retry-of (assoc "retry_of_run" retry-of)
    retry-attempt (assoc "retry_attempt" (str retry-attempt))))

(defn rows-for [run facts]
  (mapv (fn [[predicate value]] [run predicate value]) facts))

(defn all-observations [document]
  (vec (concat
        (mapcat #(get % "observations") (get document "cohorts"))
        (get document "uncohortedExclusions"))))

(defn observation-of [document run]
  (first (filter #(= run (get % "runId")) (all-observations document))))

(let [assignment-id (sha "retry-assignment")
      initial "@run:retry-initial"
      retry "@run:retry-final"
      unknown "@run:unknown-tokens"
      variant "@run:prompt-variant"
      max-safe "@run:max-safe"
      rows (vec
            (concat
             (rows-for initial
                       (complete-run initial
                                     {:assignment-id assignment-id
                                      :episode-id "episode-retry"
                                      :duration "150" :tokens "15"}))
             (rows-for retry
                       (complete-run retry
                                     {:assignment-id assignment-id
                                      :episode-id "episode-retry"
                                      :duration "200" :tokens "20"
                                      :reviewer-duration "40"
                                      :reviewer-tokens "3"
                                      :reviewer-token-status "exact"
                                      :retry-of initial :retry-attempt 1}))
             (rows-for unknown
                       (complete-run unknown
                                     {:assignment-id (sha "unknown-assignment")
                                      :episode-id "episode-unknown"
                                      :duration "300" :tokens nil
                                      :token-status "partial"
                                      :reviewer-duration "60"
                                      :reviewer-tokens "999"
                                      :reviewer-token-status "partial"}))
             (rows-for variant
                       (complete-run variant
                                     {:axis "prompt" :arm-id "variant-a"
                                      :assignment-id (sha "prompt-assignment")
                                      :episode-id "episode-prompt"
                                      :duration "400" :tokens "40"
                                      :propensity "0.100000000000"}))
             (rows-for max-safe
                       (complete-run max-safe
                                     {:axis "prompt" :arm-id "max-safe"
                                      :assignment-id (sha "max-safe-assignment")
                                      :episode-id "episode-max-safe"
                                      :duration "9007199254740991"
                                      :tokens "9007199254740991"}))))
      document (north.learning-compare/comparison-document
                "exp-fixture" 42 rows)
      reversed-document (north.learning-compare/comparison-document
                         "exp-fixture" 42 (vec (reverse rows)))
      [control max-safe-cohort prompt] (get document "cohorts")
      control-metrics (get control "metrics")
      retry-observation (observation-of document retry)
      unknown-observation (observation-of document unknown)]
  (check "row order cannot change deterministic JSON"
         (= (json/generate-string document)
            (json/generate-string reversed-document)))
  (let [request (atom nil)
        queried
        (with-redefs
         [north.coord/bounded-query-in-domain
          (fn [port domain query max-rows]
            (reset! request {:port port :domain domain
                             :query query :max-rows max-rows})
            {:rows rows :served-version 42})]
         (north.learning-compare/compare-experiment 7977 "exp-fixture"))]
    (check "command reads one bounded telemetry snapshot for the experiment"
           (and (= document queried)
                (= 7977 (:port @request))
                (= :telemetry (:domain @request))
                (= north.learning-compare/max-comparison-facts
                   (:max-rows @request))
                (= "exp-fixture"
                   (get-in @request [:query :rules 0 :body 0 :args 2])))))
  (check "comparison declares descriptive-only interpretation"
         (and (= "descriptive_only" (get document "interpretation"))
              (= "Observed cohorts only; no causal estimate is produced."
                 (get document "notice"))))
  (check "cohorts are exact task/axis/arm/baseline/options and control sorts first"
         (and (= [(sha "exact-task") "control" "control"
                  (sha "baseline") (sha "options")]
                 [(get control "taskSignature")
                  (get control "axis")
                  (get control "armId")
                  (get control "baselineSha256")
                  (get control "optionsSha256")])
              (= [(sha "exact-task") "prompt" "variant-a"
                  (sha "baseline") (sha "options")]
                 [(get prompt "taskSignature")
                  (get prompt "axis")
                  (get prompt "armId")
                  (get prompt "baselineSha256")
                  (get prompt "optionsSha256")])))
  (check "valid maximum-safe counts aggregate without integer overflow"
         (= {"mean" 9.007199254740991E15
             "populationN" 1 "knownN" 1 "unknownN" 0}
            (get-in max-safe-cohort ["metrics" "tokens"])))
  (check "retry chain contributes only its final attempt"
         (and (= 2 (get-in control ["population" "included"]))
              (= ["retry_superseded"]
                 (get (observation-of document initial) "exclusionReasons"))
              (true? (get retry-observation "included"))
              (true? (get retry-observation "selectedAttempt"))))
  (check "duration and tokens use the same decided population"
         (and (= {"mean" 250.0 "populationN" 2 "knownN" 2 "unknownN" 0}
                 (get control-metrics "durationMs"))
              (= {"mean" nil "populationN" 2 "knownN" 1 "unknownN" 1}
                 (get control-metrics "tokens"))))
  (check "reviewer usage remains separate from primary usage"
         (and (= {"mean" 50.0 "populationN" 2 "knownN" 2 "unknownN" 0}
                 (get control-metrics "reviewerDurationMs"))
              (= {"mean" nil "populationN" 2 "knownN" 1 "unknownN" 1}
                 (get control-metrics "reviewerTokens"))
              (= 200 (get retry-observation "durationMs"))
              (= 20 (get retry-observation "tokens"))
              (= 40 (get retry-observation "reviewerDurationMs"))
              (= 3 (get retry-observation "reviewerTokens"))))
  (check "unknown token evidence remains unknown"
         (and (nil? (get unknown-observation "tokens"))
              (= "partial" (get unknown-observation "tokenStatus"))
              (nil? (get unknown-observation "reviewerTokens"))
              (= "partial"
                 (get unknown-observation "reviewerUsageStatus"))))
  (check "stored propensities survive and unstored factors remain explicit"
         (= {"assigned" 0.8 "explore" 0.2 "axis" nil "arm" nil}
            (get retry-observation "propensity")))
  (check "text projection labels the non-causal and unknown contracts"
         (let [rendered (north.learning-compare/render-comparison document)]
           (and (str/includes? rendered
                               "Descriptive only — observed cohorts are not causal estimates.")
                (str/includes? rendered "tokens: unknown (known 1, unknown 1)")
                (str/includes? rendered
                               "reviewer tokens: unknown (known 1, unknown 1)")
                (str/includes? rendered (str "BASELINE " (sha "baseline")))
                (str/includes? rendered (str "OPTIONS " (sha "options")))
                (str/includes? rendered "axis-p=unknown · arm-p=unknown")))))

(let [baseline-a (sha "baseline-a")
      baseline-b (sha "baseline-b")
      run-a "@run:baseline-a"
      run-b "@run:baseline-b"
      rows (vec
            (concat
             (rows-for run-a
                       (complete-run run-a
                                     {:assignment-id (sha "baseline-a-assignment")
                                      :episode-id "episode-baseline-a"
                                      :baseline-sha256 baseline-a}))
             (rows-for run-b
                       (complete-run run-b
                                     {:assignment-id (sha "baseline-b-assignment")
                                      :episode-id "episode-baseline-b"
                                      :baseline-sha256 baseline-b}))))
      document (north.learning-compare/comparison-document
                "exp-fixture" 42 rows)
      cohorts (get document "cohorts")]
  (check "different control baselines cannot merge into one evidence cohort"
         (and (= 2 (count cohorts))
              (= #{baseline-a baseline-b}
                 (set (map #(get % "baselineSha256") cohorts)))
              (every? #(= 1 (get-in % ["population" "included"])) cohorts))))

(let [reviewer-off-options (sha "reviewer-off-options")
      reviewer-on-options (sha "reviewer-on-options")
      off-run "@run:reviewer-off-options"
      on-run "@run:reviewer-on-options"
      rows (vec
            (concat
             (rows-for off-run
                       (complete-run off-run
                                     {:assignment-id (sha "reviewer-off-assignment")
                                      :episode-id "episode-reviewer-off"
                                      :options-sha256 reviewer-off-options}))
             (rows-for on-run
                       (complete-run on-run
                                     {:assignment-id (sha "reviewer-on-assignment")
                                      :episode-id "episode-reviewer-on"
                                      :options-sha256 reviewer-on-options}))))
      document (north.learning-compare/comparison-document
                "exp-fixture" 42 rows)
      cohorts (get document "cohorts")]
  (check "exact eligible-option receipts stratify otherwise identical controls"
         (and (= 2 (count cohorts))
              (= #{reviewer-off-options reviewer-on-options}
                 (set (map #(get % "optionsSha256") cohorts)))
              (every? #(= 1 (get-in % ["population" "included"])) cohorts))))

(let [discovery "@run:discovery"
      missing "@run:missing-predecessor"
      mismatch-root "@run:mismatch-root"
      mismatch-retry "@run:mismatch-retry"
      malformed "@run:malformed-delivery"
      malformed-count "@run:malformed-count"
      partial-task "@run:partial-task-signature"
      incomplete "@run:preflight"
      rows (vec
            (concat
             (rows-for discovery
                       (complete-run discovery
                                     {:evidence-mode "discovery"
                                      :assignment-id (sha "discovery")
                                      :episode-id "episode-discovery"}))
             (rows-for missing
                       (complete-run missing
                                     {:assignment-id (sha "missing")
                                      :episode-id "episode-missing"
                                      :retry-of "@run:absent" :retry-attempt 1}))
             (rows-for mismatch-root
                       (complete-run mismatch-root
                                     {:assignment-id (sha "mismatch-root")
                                      :episode-id "episode-mismatch"}))
             (rows-for mismatch-retry
                       (complete-run mismatch-retry
                                     {:axis "prompt" :arm-id "variant-b"
                                      :assignment-id (sha "mismatch-retry")
                                      :episode-id "episode-mismatch"
                                      :retry-of mismatch-root :retry-attempt 1}))
             (rows-for malformed
                       (let [delivery
                             (json/generate-string
                              {"matches" [{"evidence" ["not-a-record"]}]})]
                         (assoc
                          (complete-run malformed
                                        {:assignment-id (sha "malformed")
                                         :episode-id "episode-malformed"})
                          "delivery_evidence" delivery
                          "delivery_evidence_sha256" (sha delivery))))
             (rows-for malformed-count
                       (complete-run malformed-count
                                     {:assignment-id (sha "malformed-count")
                                      :episode-id "episode-malformed-count"
                                      :duration "+1" :tokens "01"
                                      :reviewer-duration "+2"
                                      :reviewer-tokens "02"
                                      :reviewer-token-status "exact"}))
             (rows-for partial-task
                       (assoc
                        (complete-run partial-task
                                      {:assignment-id (sha "partial-task")
                                       :episode-id "episode-partial-task"})
                        "learning_task_signature_coverage" "partial"))
             (rows-for incomplete
                       {"learning_experiment_id" "exp-fixture"
                        "learning_evidence_mode" "discovery"})))
      document (north.learning-compare/comparison-document
                "exp-fixture" 43 rows)]
  (check "discovery attempts remain visible but cannot enter evaluation"
         (= ["not_evaluation"]
            (get (observation-of document discovery) "exclusionReasons")))
  (check "a retry with no predecessor is explicitly invalid"
         (= ["retry_chain_invalid"]
            (get (observation-of document missing) "exclusionReasons")))
  (check "cross-cohort retries cannot be deduplicated into either arm"
         (and (= ["retry_chain_cohort_mismatch"]
                 (get (observation-of document mismatch-root)
                      "exclusionReasons"))
              (= ["retry_chain_cohort_mismatch"]
                 (get (observation-of document mismatch-retry)
                      "exclusionReasons"))))
  (check "malformed delivery proof excludes one attempt without aborting comparison"
         (let [reasons (get (observation-of document malformed)
                            "exclusionReasons")]
           (and (some #{"delivery_outcome_not_reported"} reasons)
                (some #{"done_bar_contract_not_exact"} reasons))))
  (check "noncanonical count literals remain unknown rather than becoming measurements"
         (let [observed (observation-of document malformed-count)]
           (and (nil? (get observed "durationMs"))
                (nil? (get observed "tokens"))
                (nil? (get observed "reviewerDurationMs"))
                (nil? (get observed "reviewerTokens")))))
  (check "nonexact task identities remain visible outside exact cohorts"
         (let [observed (observation-of document partial-task)]
           (and (some #{"task_signature_not_exact"}
                      (get observed "exclusionReasons"))
                (some #(= partial-task (get % "runId"))
                      (get document "uncohortedExclusions")))))
  (check "assignment-bearing preflight rows remain named exclusions"
         (let [observed (observation-of document incomplete)]
           (and (false? (get observed "included"))
                (some #{"assignment_invalid"} (get observed "exclusionReasons"))
                (some #{"run_not_terminal"} (get observed "exclusionReasons"))
                (some #{"not_evaluation"} (get observed "exclusionReasons")))))
  (check "document retains deterministic aggregate exclusion counts"
         (every? #(pos? (get-in document ["exclusionCounts" %] 0))
                 ["not_evaluation" "retry_chain_invalid"
                  "retry_chain_cohort_mismatch" "assignment_invalid"])))

(doseq [[label ok?] @checks]
  (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
(let [failed (remove second @checks)]
  (println (format "\nlearning compare: %d/%d passed"
                   (- (count @checks) (count failed)) (count @checks)))
  (when (seq failed) (System/exit 1)))
