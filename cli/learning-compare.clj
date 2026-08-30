#!/usr/bin/env bb
;; Deterministic, read-only comparison of immutable learning/run facts. This is
;; a descriptive projection: it neither mutates Beagle Store nor infers causal effects.
(ns north.learning-compare
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.set :as set]
            [clojure.string :as str]))

(def cli-dir
  (.getParent (io/file (or *file* (System/getProperty "babashka.file")))))
(load-file (str cli-dir "/coord.clj"))
(load-file (str cli-dir "/terminal-projection.clj"))

(def schema-version "north-learning-comparison:v2")
(def max-comparison-facts 262144)
(def max-safe-integer 9007199254740991)
(def identifier-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$")
(def digest-pattern #"^[a-f0-9]{64}$")
(def run-pattern #"^@run[-:][A-Za-z0-9][A-Za-z0-9._:-]*$")
(def nonnegative-integer-pattern #"^(?:0|[1-9][0-9]*)$")

(def assignment-predicates
  ["learning_assignment_version"
   "learning_policy_version"
   "learning_policy_sha256"
   "learning_mode"
   "learning_evidence_mode"
   "learning_experiment_id"
   "learning_episode_id"
   "learning_task_signature_sha256"
   "learning_task_signature_coverage"
   "learning_risk"
   "learning_arm"
   "learning_axis"
   "learning_arm_id"
   "learning_propensity"
   "learning_explore_propensity"
   "learning_narrowing_reason"
   "learning_baseline_sha256"
   "learning_options_sha256"
   "learning_assignment_sha256"])

(def exclusion-order
  ["assignment_invalid"
   "run_not_terminal"
   "terminal_outcome_not_eligible"
   "delivery_outcome_not_reported"
   "not_evaluation"
   "task_signature_not_exact"
   "prompt_receipt_not_exact"
   "environment_receipt_not_exact"
   "run_envelope_not_exact"
   "routing_receipt_not_exact"
   "done_bar_contract_not_exact"
   "done_bar_evidence_incomplete"
   "retry_chain_invalid"
   "retry_chain_cohort_mismatch"
   "retry_superseded"
   "execution_observation_invalid"
   "execution_mode_unknown"
   "execution_mode_mixed"])

(def exclusion-rank (zipmap exclusion-order (range)))

(defn usage [] "usage: north learning compare <experiment-id> [--json]")

(defn- fact-values [facts predicate]
  (let [value (get facts predicate ::absent)]
    (cond
      (= ::absent value) #{}
      (set? value) value
      (and (sequential? value) (not (string? value))) (set value)
      :else #{value})))

(defn- one [facts predicate]
  (let [values (fact-values facts predicate)]
    (when (= 1 (count values)) (first values))))

(defn- present? [facts predicate]
  (boolean (seq (fact-values facts predicate))))

(defn- parse-nonnegative-long [value]
  (when (and (string? value)
             (re-matches nonnegative-integer-pattern value))
    (try
      (let [parsed (parse-long value)]
        (when (and parsed (<= parsed max-safe-integer)) parsed))
      (catch Exception _ nil))))

(defn- parse-positive-long [value]
  (when-let [parsed (parse-nonnegative-long value)]
    (when (pos? parsed) parsed)))

(defn- parse-probability [value]
  (try
    (let [parsed (Double/parseDouble (str value))]
      (when (and (Double/isFinite parsed) (<= 0.0 parsed 1.0)) parsed))
    (catch Exception _ nil)))

(defn- exact-digest? [value]
  (boolean (and (string? value) (re-matches digest-pattern value))))

(defn- exact-identifier? [value]
  (boolean (and (string? value) (re-matches identifier-pattern value))))

(def execution-observation-version "agent-execution-observation/v1")
(def execution-modes #{"standard" "fast"})
(def observation-token-pattern #"^[a-z0-9][a-z0-9._:/-]*$")

(defn- exact-fields? [value expected]
  (and (map? value) (= expected (set (keys value)))))

(defn- safe-count? [value]
  (and (integer? value) (<= 0 value max-safe-integer)))

(defn- exact-observation-token? [value]
  (boolean (and (string? value) (re-matches observation-token-pattern value))))

(defn- execution-observation [facts]
  (let [raw (one facts "execution_observation")
        parsed (when (string? raw)
                 (try (json/parse-string raw)
                      (catch Exception _ nil)))
        coverage (get parsed "coverage")
        segments (get parsed "segments")
        evidence (get parsed "evidence")
        source (get parsed "source")]
    (cond
      (not (and (= execution-observation-version (get parsed "version"))
                (exact-observation-token? source)))
      {:status "invalid"}

      (= "unknown" coverage)
      (if (and (exact-fields? parsed #{"version" "coverage" "source" "turn_unit"
                                      "tool_call_unit" "evidence" "segments"})
               (= "unknown" (get parsed "turn_unit"))
               (= "unknown" (get parsed "tool_call_unit"))
               (map? evidence) (empty? evidence)
               (sequential? segments) (empty? segments))
        {:status "unknown" :coverage coverage :source source}
        {:status "invalid"})

      (= "exact" coverage)
      (if-not (and (exact-fields? parsed
                                  #{"version" "coverage" "source" "turn_unit"
                                    "tool_call_unit" "evidence" "segments"})
                   (= "assistant-turn" (get parsed "turn_unit"))
                   (= "admitted-tool-call" (get parsed "tool_call_unit"))
                   (exact-fields? evidence #{"provider" "attempt_sha256" "session_sha256"})
                   (exact-observation-token? (get evidence "provider"))
                   (exact-digest? (get evidence "attempt_sha256"))
                   (exact-digest? (get evidence "session_sha256"))
                   (sequential? segments) (seq segments))
        {:status "invalid"}
        (loop [remaining segments preceding nil turns 0 tools 0 modes #{} seen-turns #{}]
          (if-let [segment (first remaining)]
            (let [mode (get segment "mode")
                  segment-turns (get segment "turn_count")
                  segment-tools (get segment "tool_call_count")
                  turn-keys (get segment "turn_sha256")
                  next-turns (+ turns (if (integer? segment-turns) segment-turns 0))
                  next-tools (+ tools (if (integer? segment-tools) segment-tools 0))]
              (if-not (and (exact-fields? segment
                                          #{"mode" "turn_count" "tool_call_count"
                                            "turn_sha256"})
                           (execution-modes mode)
                           (not= preceding mode)
                           (safe-count? segment-turns) (pos? segment-turns)
                           (safe-count? segment-tools)
                           (safe-count? next-turns)
                           (safe-count? next-tools)
                           (sequential? turn-keys)
                           (= segment-turns (count turn-keys))
                           (= (count turn-keys) (count (set turn-keys)))
                           (empty? (set/intersection seen-turns (set turn-keys)))
                           (every? exact-digest? turn-keys))
                {:status "invalid"}
                (recur (next remaining) mode next-turns next-tools
                       (conj modes mode) (into seen-turns turn-keys))))
            (let [mode (when (= 1 (count modes)) (first modes))]
              {:status (or mode "mixed")
               :coverage coverage
               :source source
               :evidence evidence
               :mode mode
               :turn-unit (get parsed "turn_unit")
               :tool-call-unit (get parsed "tool_call_unit")
               :turn-count turns
               :tool-call-count tools
               :segments (vec segments)}))))

      :else {:status "invalid"})))

(defn- execution-exclusion-reasons [execution]
  (case (:status execution)
    "invalid" ["execution_observation_invalid"]
    "unknown" ["execution_mode_unknown"]
    "mixed" ["execution_mode_mixed"]
    []))

(defn- exact-versioned-receipt?
  [facts version-predicate expected-version coverage-predicate digest-predicates]
  (and (= expected-version (one facts version-predicate))
       (= "exact" (one facts coverage-predicate))
       (every? #(exact-digest? (one facts %)) digest-predicates)))

(defn- assignment-valid? [facts]
  (let [arm (one facts "learning_arm")
        axis (one facts "learning_axis")
        arm-id (one facts "learning_arm_id")]
    (and (every? #(some? (one facts %)) assignment-predicates)
         (= "north-learning-assignment:v1"
            (one facts "learning_assignment_version"))
         (= "north-learning-policy:v1" (one facts "learning_policy_version"))
         (#{"frozen" "learning"} (one facts "learning_mode"))
         (#{"discovery" "evaluation"}
          (one facts "learning_evidence_mode"))
         (#{"exact" "partial" "unknown"}
          (one facts "learning_task_signature_coverage"))
         (#{"p0" "p1" "p2" "p3" "unknown"}
          (one facts "learning_risk"))
         (#{"control" "explore"} arm)
         (#{"control" "model-tier" "effort" "prompt" "authoring" "history"}
          axis)
         (or (and (= "control" arm) (= "control" axis) (= "control" arm-id))
             (and (= "explore" arm) (not= "control" axis)
                  (not= "control" arm-id)))
         (every? #(exact-digest? (one facts %))
                 ["learning_policy_sha256"
                  "learning_task_signature_sha256"
                  "learning_baseline_sha256"
                  "learning_options_sha256"
                  "learning_assignment_sha256"])
         (every? #(exact-identifier? (one facts %))
                 ["learning_experiment_id" "learning_episode_id"
                  "learning_arm_id" "learning_narrowing_reason"])
         (some? (parse-probability (one facts "learning_propensity")))
         (some? (parse-probability
                 (one facts "learning_explore_propensity"))))))

(defn- routing-receipt-exact? [facts]
  (and (= "1" (one facts "routing_admission_receipt_version"))
       (every? #(exact-digest? (one facts %))
               ["routing_request_sha256" "routing_policy_sha256"])
       (every? #(exact-identifier? (one facts %))
               ["routing_applied_task_grade" "routing_applied_topology"
                "routing_applied_tier" "routing_applied_reasoning"
                "routing_applied_posture"])))

(defn- done-bar-reasons [run facts]
  (if-not (north.terminal-projection/run-reservation-valid? facts)
    ["done_bar_contract_not_exact"]
    (let [bars (north.terminal-projection/run-reservation-done-when facts)
          thread (one facts "run_reservation_thread")
          reporter (one facts "run_reservation_agent")
          evidence (north.terminal-projection/run-evidence-state
                    facts run thread reporter)
          observed-bars (set (map #(get % "bar") (:records evidence)))
          delivery (try (json/parse-string (one facts "delivery_evidence"))
                        (catch Exception _ nil))
          cited-records
          (when (and (map? delivery)
                     (vector? (get delivery "matches"))
                     (every? #(and (map? %)
                                   (vector? (get % "evidence"))
                                   (every? map? (get % "evidence")))
                             (get delivery "matches")))
            (set (mapcat (fn [match]
                           (map #(json/generate-string (into (sorted-map) %))
                                (get match "evidence" [])))
                         (get delivery "matches" []))))
          expected-reporter (when-let [agent (one facts "agent")]
                              (str "@agent:" agent))
          expected-thread (one facts "thread")]
      (cond
        (empty? bars) ["done_bar_contract_not_exact"]
        (or (not= expected-reporter reporter)
            (not= expected-thread thread)
            (not= run (get delivery "run"))
            (not= expected-thread (get delivery "thread"))
            (not= expected-reporter (get delivery "reporter"))
            (not= (one facts "run_reservation_contract_origin")
                  (get delivery "contractOrigin"))
            (not= bars (get delivery "baselineDoneWhen"))
            (not= bars (get delivery "doneWhen")))
        ["done_bar_contract_not_exact"]
        (or (nil? cited-records)
            (not (:valid? evidence))
            (not= (set bars) observed-bars)
            (not= (:raws evidence) cited-records))
        ["done_bar_evidence_incomplete"]
        :else []))))

(defn- ordered-reasons [reasons]
  (->> reasons distinct
       (sort-by (fn [reason] [(get exclusion-rank reason Integer/MAX_VALUE)
                              reason]))
       vec))

(defn- eligibility-reasons [run facts]
  (ordered-reasons
   (concat
    (when-not (assignment-valid? facts) ["assignment_invalid"])
    (when-not (north.terminal-projection/committed-run? facts)
      ["run_not_terminal"])
    (when-not (= "ran"
                 (north.terminal-projection/committed-run-process-outcome
                  facts))
      ["terminal_outcome_not_eligible"])
    (when-not (and (= "reported" (one facts "delivery_outcome"))
                   (= "complete_run_scoped_done_bar_evidence_self_reported"
                      (one facts "delivery_reason"))
                   (north.terminal-projection/delivery-projection-valid? facts))
      ["delivery_outcome_not_reported"])
    (when-not (= "evaluation" (one facts "learning_evidence_mode"))
      ["not_evaluation"])
    (when-not (= "exact" (one facts "learning_task_signature_coverage"))
      ["task_signature_not_exact"])
    (when-not (exact-versioned-receipt?
               facts "prompt_receipt_version" "north-prompt-receipt:v1"
               "prompt_receipt_coverage"
               ["prompt_receipt_sha256" "prompt_wire_sha256"])
      ["prompt_receipt_not_exact"])
    (when-not (exact-versioned-receipt?
               facts "environment_receipt_version"
               "north-environment-receipt:v1"
               "environment_receipt_coverage"
               ["environment_receipt_sha256"
                "available_skill_catalog_sha256"
                "activated_resource_closure_sha256"])
      ["environment_receipt_not_exact"])
    (when-not (and (= "north-run-envelope:v1"
                        (one facts "run_envelope_version"))
                   (exact-digest? (one facts "run_envelope_sha256")))
      ["run_envelope_not_exact"])
    (when-not (routing-receipt-exact? facts)
      ["routing_receipt_not_exact"])
    (done-bar-reasons run facts))))

(defn- exact-cohort [facts execution-mode]
  (let [task (one facts "learning_task_signature_sha256")
        axis (one facts "learning_axis")
        arm (one facts "learning_arm_id")
        baseline (one facts "learning_baseline_sha256")
        options (one facts "learning_options_sha256")]
    (when (and (exact-digest? task)
               (exact-digest? baseline)
               (exact-digest? options)
               (= "exact" (one facts "learning_task_signature_coverage"))
               (#{"control" "model-tier" "effort" "prompt" "authoring" "history"}
                axis)
               (exact-identifier? arm)
               (execution-modes execution-mode))
      [task axis arm execution-mode baseline options])))

(defn- observation [run facts]
  (let [retry-of (one facts "retry_of_run")
        retry-link-present? (present? facts "retry_of_run")
        execution (execution-observation facts)
        token-status (or (one facts "usage_total_status") "unknown")
        reviewer-token-status
        (or (one facts "shadow_reviewer_usage_status") "unknown")]
    {:run run
     :facts facts
     :cohort (exact-cohort facts (:mode execution))
     :execution execution
     :episode-id (one facts "learning_episode_id")
     :assignment-id (one facts "learning_assignment_sha256")
     :retry-of retry-of
     :retry-attempt (parse-positive-long (one facts "retry_attempt"))
     :retry-link-invalid
     (or (and retry-link-present?
              (not (and (string? retry-of) (re-matches run-pattern retry-of))))
         (and retry-of (nil? (parse-positive-long (one facts "retry_attempt"))))
         (and (not retry-of) (present? facts "retry_attempt")))
     :outcome (one facts "outcome")
     :duration-ms (parse-nonnegative-long (one facts "duration_ms"))
     :token-status token-status
     :tokens (when (= "exact" token-status)
               (parse-nonnegative-long (one facts "tokens")))
     :reviewer-duration-ms
     (parse-nonnegative-long (one facts "shadow_reviewer_duration_ms"))
     :reviewer-token-status reviewer-token-status
     :reviewer-tokens
     (when (= "exact" reviewer-token-status)
       (parse-nonnegative-long (one facts "shadow_reviewer_tokens")))
     :propensity (parse-probability (one facts "learning_propensity"))
     :explore-propensity
     (parse-probability (one facts "learning_explore_propensity"))
     :base-reasons (concat (eligibility-reasons run facts)
                           (execution-exclusion-reasons execution))}))

(defn- trace-chain-root [by-run observation]
  (loop [run (:run observation) seen #{}]
    (if (seen run)
      {:root (first (sort (conj seen run))) :invalid true}
      (if-let [current (get by-run run)]
        (if-let [predecessor (:retry-of current)]
          (recur predecessor (conj seen run))
          {:root run :invalid false})
        {:root run :invalid true}))))

(defn- annotate-retry-chains [observations]
  (let [by-run (into {} (map (juxt :run identity)) observations)
        traced (mapv (fn [item]
                       (let [{:keys [root invalid]}
                             (trace-chain-root by-run item)]
                         (assoc item :chain-id root :chain-trace-invalid invalid)))
                     observations)]
    (->> traced
         (group-by :chain-id)
         (sort-by key)
         (mapcat
          (fn [[chain-id members]]
            (let [members (vec (sort-by :run members))
                  child-counts (frequencies (keep :retry-of members))
                  children (set (keep :retry-of members))
                  leaves (filterv #(not (children (:run %))) members)
                  member-by-run (into {} (map (juxt :run identity)) members)
                  attempt-invalid?
                  (some (fn [item]
                          (if-let [predecessor (:retry-of item)]
                            (let [parent (get member-by-run predecessor)
                                  parent-attempt (or (:retry-attempt parent) 0)]
                              (or (nil? parent)
                                  (not= (inc parent-attempt)
                                        (:retry-attempt item))))
                            (some? (:retry-attempt item))))
                        members)
                  structural-invalid?
                  (or (some :chain-trace-invalid members)
                      (some :retry-link-invalid members)
                      (some #(> % 1) (vals child-counts))
                      (not= 1 (count leaves))
                      attempt-invalid?)
                  mismatch? (or (> (count (set (map :cohort members))) 1)
                                (> (count (set (map :assignment-id members))) 1))
                  selected-attempt (when-not (or structural-invalid? mismatch?)
                                     (first leaves))]
              (map
               (fn [item]
                 (let [selected? (= (:run item) (:run selected-attempt))
                       reasons (concat
                                (:base-reasons item)
                                (when structural-invalid?
                                  ["retry_chain_invalid"])
                                (when mismatch? ["retry_chain_cohort_mismatch"])
                                (when (and selected-attempt (not selected?))
                                  ["retry_superseded"]))
                       exclusions (ordered-reasons reasons)]
                   (assoc item
                          :chain-id chain-id
                          :selected selected?
                          :exclusion-reasons exclusions
                          :included (and selected? (empty? exclusions)))))
               members))))
         vec)))

(defn- mean-metric [included field]
  (let [values (mapv field included)
        known (filterv some? values)
        population (count values)
        unknown (- population (count known))]
    (array-map
     "mean" (when (and (pos? population) (zero? unknown))
              (/ (reduce + 0.0 known) population))
     "populationN" population
     "knownN" (count known)
     "unknownN" unknown)))

(defn- public-propensity [item]
  (array-map
   "assigned" (:propensity item)
   "explore" (:explore-propensity item)
   "axis" nil
   "arm" nil))

(defn- public-observation [item]
  (array-map
   "runId" (:run item)
   "episodeId" (:episode-id item)
   "assignmentId" (:assignment-id item)
   "retryChainId" (:chain-id item)
   "retryOfRun" (:retry-of item)
   "retryAttempt" (:retry-attempt item)
   "selectedAttempt" (:selected item)
   "included" (:included item)
   "exclusionReasons" (:exclusion-reasons item)
   "outcome" (:outcome item)
   "durationMs" (:duration-ms item)
   "tokens" (:tokens item)
   "tokenStatus" (:token-status item)
   "reviewerDurationMs" (:reviewer-duration-ms item)
   "reviewerTokens" (:reviewer-tokens item)
   "reviewerUsageStatus" (:reviewer-token-status item)
   "executionMode" (get-in item [:execution :mode])
   "executionCoverage" (get-in item [:execution :coverage])
   "executionSource" (get-in item [:execution :source])
   "executionEvidence" (get-in item [:execution :evidence])
   "turnUnit" (get-in item [:execution :turn-unit])
   "toolCallUnit" (get-in item [:execution :tool-call-unit])
   "turnCount" (get-in item [:execution :turn-count])
   "toolCallCount" (get-in item [:execution :tool-call-count])
   "modeSegments" (get-in item [:execution :segments])
   "propensity" (public-propensity item)))

(defn- exclusion-counts [observations]
  (into (sorted-map)
        (frequencies (mapcat :exclusion-reasons observations))))

(defn- cohort-sort-key [[task axis arm execution-mode baseline options]]
  [task (if (= axis "control") 0 1) axis
   (if (= arm "control") 0 1) arm execution-mode baseline options])

(defn- cohort-document [[task axis arm execution-mode baseline options] observations]
  (let [observations (vec (sort-by (juxt :chain-id :run) observations))
        included (filterv :included observations)
        outcomes (frequencies (map #(or (:outcome %) "unknown") included))]
    (array-map
     "taskSignature" task
     "axis" axis
     "armId" arm
     "executionMode" execution-mode
     "baselineSha256" baseline
     "optionsSha256" options
     "population" (array-map
                    "attempts" (count observations)
                    "logicalChains" (count (set (map :chain-id observations)))
                    "included" (count included)
                    "excludedAttempts" (count (remove :included observations)))
     "outcomes" (into (sorted-map) outcomes)
     "metrics" (array-map
                 "durationMs" (mean-metric included :duration-ms)
                 "tokens" (mean-metric included :tokens)
                 "reviewerDurationMs"
                 (mean-metric included :reviewer-duration-ms)
                 "reviewerTokens" (mean-metric included :reviewer-tokens))
     "exclusionCounts" (exclusion-counts observations)
     "observations" (mapv public-observation observations))))

(defn- facts-by-run [rows]
  (reduce (fn [runs row]
            (when-not (and (sequential? row) (= 3 (count row)))
              (throw (ex-info "learning comparison query returned a malformed row"
                              {:type :malformed-learning-query-row
                               :row row})))
            (let [[run predicate value] row]
              (when-not (and (string? run) (re-matches run-pattern run)
                             (string? predicate) (string? value))
                (throw (ex-info
                        "learning comparison query returned a malformed fact"
                        {:type :malformed-learning-query-row :row row})))
              (update-in runs [run predicate] (fnil conj #{}) value)))
          {}
          rows))

(defn comparison-document
  "Build the stable public comparison from one snapshot's [run predicate value]
   rows. This pure boundary is shared by the command and contract tests."
  [experiment-id source-version rows]
  (let [observations (->> (facts-by-run rows)
                          (map (fn [[run facts]] (observation run facts)))
                          (sort-by :run)
                          vec
                          annotate-retry-chains)
        chain-groups (vals (group-by :chain-id observations))
        cohorts (->> observations
                     (filter :cohort)
                     (group-by :cohort)
                     (sort-by (comp cohort-sort-key key))
                     (mapv (fn [[cohort items]]
                             (cohort-document cohort items))))
        uncohorted (->> observations
                        (remove :cohort)
                        (sort-by (juxt :chain-id :run))
                        (mapv public-observation))
        included (count (filter :included observations))
        included-chains (count (filter #(some :included %) chain-groups))]
    (array-map
     "schemaVersion" schema-version
     "experimentId" experiment-id
     "interpretation" "descriptive_only"
     "notice" "Observed cohorts only; no causal estimate is produced."
     "source" (array-map "kind" "store_facts" "version" source-version)
     "population" (array-map
                    "attempts" (count observations)
                    "logicalChains" (count chain-groups)
                    "includedLogicalObservations" included
                    "excludedAttempts" (- (count observations) included)
                    "excludedLogicalChains" (- (count chain-groups)
                                                included-chains))
     "exclusionCounts" (exclusion-counts observations)
     "cohorts" cohorts
     "uncohortedExclusions" uncohorted)))

(defn- comparison-query [experiment-id]
  {:find "learning_comparison_fact"
   :rules [{:head {:rel "learning_comparison_fact"
                   :args [{:var "run"} {:var "predicate"} {:var "value"}]}
            :body [{:rel "triple"
                    :args [{:var "run"} "learning_experiment_id" experiment-id]}
                   {:rel "triple"
                    :args [{:var "run"} {:var "predicate"} {:var "value"}]}]}]})

(defn compare-experiment [port experiment-id]
  (when-not (exact-identifier? experiment-id)
    (throw (ex-info "learning experiment id must be a portable identifier"
                    {:type :usage})))
  (let [result (north.coord/bounded-query-in-domain!
                port :telemetry (comparison-query experiment-id)
                max-comparison-facts)]
    (when (empty? (:rows result))
      (throw (ex-info (str "learning experiment not found: " experiment-id)
                      {:type :not-found :experiment-id experiment-id})))
    (comparison-document experiment-id (:served-version result) (:rows result))))

(defn- metric-label [metric suffix]
  (let [mean (get metric "mean")]
    (str (if (nil? mean) "unknown" (format "%.2f%s" mean suffix))
         " (known " (get metric "knownN")
         ", unknown " (get metric "unknownN") ")")))

(defn- propensity-label [value]
  (if (nil? value) "unknown" (format "%.12f" value)))

(defn render-comparison [document]
  (let [population (get document "population")
        cohorts (get document "cohorts")
        header [(str "LEARNING COMPARISON — " (get document "experimentId"))
                "  Descriptive only — observed cohorts are not causal estimates."
                (str "  " (get population "attempts") " attempts · "
                     (get population "logicalChains") " retry chains · "
                     (get population "includedLogicalObservations")
                     " included logical observations · "
                     (get population "excludedAttempts") " excluded attempts")]
        cohort-lines
        (mapcat
         (fn [cohort]
           (let [cohort-population (get cohort "population")
                 metrics (get cohort "metrics")
                 outcomes (get cohort "outcomes")]
             (concat
              [(str "\nTASK " (get cohort "taskSignature"))
               (str "  AXIS " (get cohort "axis") " · ARM "
                    (get cohort "armId"))
               (str "  EXECUTION MODE " (get cohort "executionMode"))
               (str "  BASELINE " (get cohort "baselineSha256"))
               (str "  OPTIONS " (get cohort "optionsSha256"))
               (str "    population: " (get cohort-population "included")
                    " included / " (get cohort-population "attempts")
                    " attempts · " (get cohort-population "excludedAttempts")
                    " excluded")
               (str "    outcomes: "
                    (if (seq outcomes)
                      (str/join ", " (map (fn [[label count]]
                                             (str label "=" count)) outcomes))
                      "none"))
               (str "    duration: "
                    (metric-label (get metrics "durationMs") "ms"))
               (str "    tokens: "
                    (metric-label (get metrics "tokens") ""))
               (str "    reviewer duration: "
                    (metric-label (get metrics "reviewerDurationMs") "ms"))
               (str "    reviewer tokens: "
                    (metric-label (get metrics "reviewerTokens") ""))]
              (map
               (fn [item]
                 (let [propensity (get item "propensity")]
                   (str "    " (if (get item "included") "include " "exclude ")
                        (get item "runId")
                        " · p=" (propensity-label (get propensity "assigned"))
                        " · explore-p="
                        (propensity-label (get propensity "explore"))
                        " · axis-p="
                        (propensity-label (get propensity "axis"))
                        " · arm-p="
                        (propensity-label (get propensity "arm"))
                        (when-let [reasons (seq (get item "exclusionReasons"))]
                          (str " · " (str/join "," reasons))))))
               (get cohort "observations")))))
         cohorts)
        uncohorted (get document "uncohortedExclusions")
        uncohorted-lines
        (when (seq uncohorted)
          (concat
           ["\nUNCOHORTED EXCLUSIONS"]
           (map (fn [item]
                  (str "  " (get item "runId") " · "
                       (str/join "," (get item "exclusionReasons"))))
                uncohorted)))
        exclusion-counts (get document "exclusionCounts")
        exclusion-lines
        (concat
         ["\nEXCLUSIONS"]
         (if (seq exclusion-counts)
           (map (fn [[reason count]] (str "  " reason ": " count))
                exclusion-counts)
           ["  none"]))]
    (str (str/join "\n" (concat header cohort-lines uncohorted-lines
                                  exclusion-lines))
         "\n")))

(defn- parse-args [args]
  (cond
    (and (= 1 (count args)) (#{"help" "--help" "-h"} (first args)))
    {:help true}

    (and (<= 2 (count args) 3)
         (= "compare" (first args))
         (not (str/starts-with? (second args) "--"))
         (or (= 2 (count args)) (= "--json" (nth args 2))))
    {:experiment-id (second args) :json (= 3 (count args))}

    :else
    (throw (ex-info (usage) {:type :usage}))))

(defn -main [& args]
  (try
    (let [{:keys [help experiment-id json]} (parse-args args)]
      (if help
        (println (usage))
        (let [port (Integer/parseInt
                    (or (System/getenv "NORTH_PORT") "7977"))
              comparison (compare-experiment port experiment-id)]
          (if json
            (println (json/generate-string comparison))
            (print (render-comparison comparison))))))
    (catch clojure.lang.ExceptionInfo error
      (binding [*out* *err*] (println (.getMessage error)))
      (System/exit (if (= :usage (:type (ex-data error))) 2 1)))
    (catch Throwable error
      (binding [*out* *err*]
        (println (str "learning comparison failed: " (.getMessage error))))
      (System/exit 1))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
