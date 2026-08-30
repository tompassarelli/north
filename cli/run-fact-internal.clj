(ns user
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.java.shell :as shell]
            [clojure.set :as set]
            [clojure.string :as str]))

(def cli-dir (.getParent (io/file (System/getProperty "babashka.file"))))

(load-file (str cli-dir "/coord.clj"))

(load-file (str cli-dir "/terminal-projection.clj"))

(load-file (str cli-dir "/run-ledger.clj"))

(def wire-telemetry-validator (.getCanonicalPath (io/file cli-dir "../sdk/src/wire-telemetry-validator.ts")))

(defn fail! [message data]
  (throw (ex-info message data)))

(defn checked! [result operation]
  (if (:reject result) (do
  (fail! "coordinator rejected wire run telemetry publication" {:operation operation})))
  result)

(def terminal-predicates #{"kind" "wire_run_id" "thread" "thread_provenance" "agent" "parent_run" "parent_thread" "run_coordinator" "wire_ledger_version" "wire_version" "wire_ledger_status" "wire_event_count" "wire_event_first_sequence" "wire_event_last_sequence" "wire_terminal_event_id" "wire_ledger_sha256" "wire_run_lifecycle" "wire_termination_code" "outcome" "at" "started_at" "duration_ms" "estimate_hours" "estimate_delta_ms" "estimate_ratio" "estimate_classification" "judgment_grade" "judgment_grade_status" "judgment_grade_source" "lifetime_input_tokens" "lifetime_output_tokens" "lifetime_cache_read_tokens" "lifetime_cache_write_tokens" "lifetime_reasoning_tokens" "model_call_count" "tokens" "usage_terminal_count" "usage_scope" "usage_total_status" "context_tokens" "context_window_tokens" "compaction_count" "tool_admitted_count" "tool_succeeded_count" "tool_failed_count" "tool_cancelled_count" "tool_synthetic_failure_count" "run_owner" "model_tier" "capability_class" "effort" "posture" "role" "provider" "provider_target" "provider_reason" "model_availability_target" "model_availability_source" "model_availability_observed_at" "model_availability_digest" "requested_provider" "requested_target" "requested_tier" "requested_effort" "allocation_mode" "entitlement_pressure" "allocation_evidence" "fallback_count" "fallback_path" "fallback_target_path" "fallback_reason" "envelope_scope" "envelope_retries" "envelope_advisory" "process_outcome" "delivery_outcome" "delivery_reason" "error_count" "struggle" "struggle_detector_policy_version" "struggle_topology" "struggle_error_streak_threshold" "struggle_loop_repeat_threshold" "struggle_loop_window" "struggle_no_progress_turn_threshold" "delivery_evidence" "delivery_evidence_sha256" "retry_of_run" "retry_attempt" "execution_source" "execution_transport" "provider_session_persistence" "provider_join_key_version" "provider_join_coverage" "provider_session_key" "provider_turn_key" "provider_duration_ms" "turn_provenance" "num_turns" "provider_turn_units" "provider_tool_items" "provider_turn_metric_comparable" "execution_observation" "watchdog_reason" "watchdog_silence_ms" "watchdog_last_outer_activity" "watchdog_last_provider_activity" "effective_authority_provider" "effective_native_multi_agent" "effective_live_input" "effective_authoring_hooks" "effective_authority_capability" "effective_north_enabled_tool" "effective_sandbox" "effective_web" "effective_builtin" "effective_mcp_tool" "mcp_activity_source" "mcp_activity_coverage" "mcp_actual_calls" "mcp_actual_tool" "mcp_operation_receipt" "mcp_operation_aggregate" "native_command_activity_source" "native_command_activity_coverage" "native_north_binary_probe" "native_command_total" "native_command_successful" "native_command_failed" "native_command_declined" "native_command_truncated" "native_command_open" "native_command_read" "native_command_edit" "native_command_completion" "prompt_receipt_version" "prompt_receipt_sha256" "prompt_wire_sha256" "prompt_receipt_coverage" "environment_receipt_version" "environment_receipt_sha256" "environment_receipt_coverage" "available_skill_catalog_sha256" "activated_resource_closure_sha256" "run_envelope_version" "run_envelope_sha256" "requested_role" "routing_tier" "requested_reasoning" "routing_posture" "task_grade" "topology" "domain_requirement" "composition_kind" "composition_id" "composition_override" "nearest_template" "promotion_candidate" "routing_admission_receipt_version" "routing_request_sha256" "staffing_catalog_sha256" "provider_catalogs_sha256" "routing_policy_sha256" "orchestration_policy_pin_sha256" "orchestration_catalog_digest_sha256" "orchestration_catalog_version" "orchestration_catalog_tx_version" "routing_assessment_status" "routing_assessment_sha256" "routing_pin_evidence_status" "routing_pin_evidence_sha256" "routing_override_evidence_status" "routing_override_exception_code" "routing_receipt_override" "routing_applied_task_grade" "routing_applied_topology" "routing_applied_tier" "routing_applied_reasoning" "routing_applied_posture" "routing_stock_task_grade" "routing_stock_topology" "routing_stock_tier" "routing_stock_reasoning" "routing_stock_posture" "routing_assessment_policy" "routing_signal_decision_ownership" "routing_signal_seam_scope" "routing_signal_error_exposure" "routing_signal_oracle_strength" "routing_signal_foundational_impact" "routing_signal_dependency_shape" "routing_signal_reasoning_shape" "routing_derived_tier" "routing_derived_reasoning" "routing_rule_code" "routing_selected_tier" "routing_selected_reasoning" "routing_exception_code" "routing_pin_policy" "routing_pin_issued_at" "routing_pin_expires_at" "routing_pin_reason_code" "routing_pin" "prompt_composition_applied" "applied_role_contract" "applied_bespoke_contract_sha256" "applied_bespoke_contract_fingerprint_version" "applied_bespoke_contract_fingerprint_domain" "applied_template_override" "applied_template_override_reason_sha256" "applied_capability" "applied_comms_contract_sha256" "applied_task_grade" "applied_topology" "applied_routing_tier" "applied_reasoning" "applied_posture" "applied_domain_requirement" "applied_domain_requirement_count" "model_delta_provider" "model_delta_kind" "prompt_composition_version" "prompt_composition_sha256" "prompt_capability_class" "prompt_byte_measurement_source" "prompt_token_measurement_status" "prompt_token_measurement_source" "context_window_status" "context_window_source" "context_budget_status" "context_budget_source" "compaction_policy" "compaction_policy_version" "context_window_effective_from" "prompt_stable_prefix_bytes" "prompt_unique_tail_bytes" "prompt_total_bytes" "prompt_capability_count" "prompt_stable_prefix_tokens" "prompt_unique_tail_tokens" "prompt_total_composition_tokens" "provider_context_window_tokens" "effective_context_budget_tokens" "learning_assignment_version" "learning_policy_version" "learning_policy_sha256" "learning_mode" "learning_evidence_mode" "learning_experiment_id" "learning_episode_id" "learning_task_signature_sha256" "learning_task_signature_coverage" "learning_risk" "learning_arm" "learning_axis" "learning_arm_id" "learning_propensity" "learning_explore_propensity" "learning_narrowing_reason" "learning_baseline_sha256" "learning_options_sha256" "learning_assignment_sha256" "shadow_reviewer_version" "shadow_reviewer_target" "shadow_reviewer_status" "shadow_reviewer_eligible_updates" "shadow_reviewer_reviewed_updates" "shadow_reviewer_dropped_updates" "shadow_reviewer_emitted_notes" "shadow_reviewer_quarantined_outputs" "shadow_reviewer_failed_reviews" "shadow_reviewer_usage_status" "shadow_reviewer_tokens" "shadow_reviewer_duration_ms" "shadow_reviewer_source_run" "shadow_reviewer_source_from_sequence" "shadow_reviewer_source_through_sequence" "shadow_reviewer_privacy_omitted_events" "shadow_reviewer_capacity_omitted_events" "shadow_reviewer_input_sha256"})

(def multi-predicates #{"allocation_evidence" "fallback_reason" "envelope_scope" "envelope_advisory" "effective_authority_capability" "effective_north_enabled_tool" "effective_builtin" "effective_mcp_tool" "mcp_actual_tool" "mcp_operation_receipt" "mcp_operation_aggregate" "native_command_completion" "domain_requirement" "composition_override" "routing_receipt_override" "routing_rule_code" "routing_pin" "applied_template_override" "applied_capability" "applied_domain_requirement" "struggle" "provider_turn_key"})

(def required-predicates #{"kind" "wire_run_id" "thread" "agent" "wire_ledger_version" "wire_version" "wire_ledger_status" "wire_event_count" "wire_event_first_sequence" "wire_event_last_sequence" "wire_terminal_event_id" "wire_ledger_sha256" "wire_run_lifecycle" "wire_termination_code" "outcome" "at" "started_at" "duration_ms" "thread_provenance" "provider_session_persistence" "turn_provenance" "execution_observation" "lifetime_input_tokens" "lifetime_output_tokens" "lifetime_cache_read_tokens" "lifetime_cache_write_tokens" "lifetime_reasoning_tokens" "model_call_count" "usage_terminal_count" "usage_scope" "usage_total_status" "context_tokens" "compaction_count" "tool_admitted_count" "tool_succeeded_count" "tool_failed_count" "tool_cancelled_count" "tool_synthetic_failure_count"})

(def count-predicates #{"wire_event_count" "wire_event_first_sequence" "wire_event_last_sequence" "duration_ms" "lifetime_input_tokens" "lifetime_output_tokens" "lifetime_cache_read_tokens" "lifetime_cache_write_tokens" "lifetime_reasoning_tokens" "model_call_count" "context_tokens" "context_window_tokens" "compaction_count" "tool_admitted_count" "tool_succeeded_count" "tool_failed_count" "tool_cancelled_count" "tool_synthetic_failure_count" "mcp_actual_calls" "fallback_count" "native_command_total" "native_command_successful" "native_command_failed" "native_command_declined" "native_command_open" "native_command_truncated" "native_command_read" "native_command_edit" "envelope_retries" "retry_attempt" "orchestration_catalog_version" "orchestration_catalog_tx_version" "applied_domain_requirement_count" "prompt_stable_prefix_bytes" "prompt_unique_tail_bytes" "prompt_total_bytes" "prompt_capability_count" "prompt_stable_prefix_tokens" "prompt_unique_tail_tokens" "prompt_total_composition_tokens" "provider_context_window_tokens" "effective_context_budget_tokens" "error_count" "struggle_error_streak_threshold" "struggle_loop_repeat_threshold" "struggle_loop_window" "struggle_no_progress_turn_threshold" "usage_terminal_count" "provider_duration_ms" "num_turns" "provider_turn_units" "provider_tool_items" "watchdog_silence_ms" "shadow_reviewer_eligible_updates" "shadow_reviewer_reviewed_updates" "shadow_reviewer_dropped_updates" "shadow_reviewer_emitted_notes" "shadow_reviewer_quarantined_outputs" "shadow_reviewer_failed_reviews" "shadow_reviewer_tokens" "shadow_reviewer_duration_ms" "shadow_reviewer_source_from_sequence" "shadow_reviewer_source_through_sequence" "shadow_reviewer_privacy_omitted_events" "shadow_reviewer_capacity_omitted_events"})

(def learning-keys #{"learning_assignment_version" "learning_policy_version" "learning_policy_sha256" "learning_mode" "learning_evidence_mode" "learning_experiment_id" "learning_episode_id" "learning_task_signature_sha256" "learning_task_signature_coverage" "learning_risk" "learning_arm" "learning_axis" "learning_arm_id" "learning_propensity" "learning_explore_propensity" "learning_narrowing_reason" "learning_baseline_sha256" "learning_options_sha256" "learning_assignment_sha256"})

(def reservation-keys (conj (into (set north.terminal-projection/run-reservation-predicates) learning-keys) "run_bar_evidence"))

(def identifier-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$")

(def digest-pattern #"^[a-f0-9]{64}$")

(defn entity! [subject]
  (let [raw (str subject)
   canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
  (if (not (north.terminal-projection/valid-run-entity? canonical)) (do
  (fail! "invalid wire run telemetry subject" {:subject subject})))
  canonical))

(defn payload! [raw]
  (let [parsed (try
  (json/parse-string (str raw))
  (catch Exception error
    (fail! "invalid wire run telemetry JSON" {:cause (.getMessage error)})))]
  (if (not (sequential? parsed)) (do
  (fail! "wire run telemetry payload must be an array" {})))
  (mapv (fn [entry] (if (not (and (sequential? entry) (= 2 (count entry)) (every? string? entry) (every? (fn [__north_anon_1] (not (str/blank? __north_anon_1))) entry))) (do
  (fail! "wire run telemetry facts must be nonblank string pairs" {:entry entry})))
  (vec entry)) parsed)))

(defn bounded-stdin! []
  (let [buffer (byte-array 8192)
   output (java.io.ByteArrayOutputStream.)]
  (loop [total 0]
  (let [read (.read System/in buffer)]
  (if (neg? read) (.toString output "UTF-8") (let [next-total (+ total read)]
  (if (> next-total north.run-ledger/max-telemetry-projection-bytes) (do
  (fail! "wire run telemetry exceeds its encoded byte bound" {:limit north.run-ledger/max-telemetry-projection-bytes})))
  (.write output buffer 0 read)
  (recur next-total)))))))

(defn facts-of [port subject]
  (let [rows (north.coord/query-rows port {:find "wire_run_writer_fact" :rules [{:head {:rel "wire_run_writer_fact" :args [{:var "p"} {:var "r"}]} :body [{:rel "triple" :args [subject {:var "p"} {:var "r"}]}]}]})]
  (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))

(def operation-tool-pattern #"[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}")

(def operation-component-pattern #"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}")

(defn instant! [label value]
  (try
  (java.time.Instant/parse value)
  (catch Exception _
    (fail! (str "invalid wire run telemetry " label) {:value value}))))

(defn parse-operation-json! [label raw]
  (try
  (json/parse-string raw)
  (catch Exception error
    (fail! (str "invalid " label " JSON") {:cause (.getMessage error)}))))

(defn exact-json-fields! [label record fields]
  (let [actual (if (map? record) (set (keys record)) #{})]
  (if (not (= fields actual)) (do
  (fail! (str label " requires its exact field set") {:fields actual})))))

(defn validate-mcp-tool-evidence! [entries scalar]
  (if (> (count entries) 512) (do
  (fail! "MCP tool evidence exceeds the bounded tool limit" {})))
  (let [tools (mapv (fn [[_ raw]] (let [record (parse-operation-json! "MCP tool activity" raw)]
  (exact-json-fields! "MCP tool activity" record #{"server" "tool" "count"})
  (if (not (and (every? (fn [__north_anon_1] (and (string? __north_anon_1) (re-matches operation-component-pattern __north_anon_1))) ((juxt (fn [__north_anon_1] (get __north_anon_1 "server")) (fn [__north_anon_1] (get __north_anon_1 "tool"))) record)) (integer? (get record "count")) (pos? (get record "count")))) (do
  (fail! "MCP tool activity contains invalid values" {:record record})))
  record)) entries)
   actual-calls (some-> (get scalar "mcp_actual_calls") parse-long)
   known-calls (reduce + 0 (map (fn [__north_anon_1] (get __north_anon_1 "count")) tools))
   coverage (get scalar "mcp_activity_coverage")]
  (if (not (= (count tools) (count (set (map (juxt (fn [__north_anon_1] (get __north_anon_1 "server")) (fn [__north_anon_1] (get __north_anon_1 "tool"))) tools))))) (do
  (fail! "MCP tool activity identities must be unique" {})))
  (if (and actual-calls (> known-calls actual-calls)) (do
  (fail! "MCP identified calls exceed the observed call count" {})))
  (if (and (= "exact" coverage) actual-calls (not= known-calls actual-calls)) (do
  (fail! "exact MCP tool activity does not reconcile with observed calls" {})))))

(def allocation-kinds #{"numeric-headroom" "categorical-pressure" "conservative-floor"})

(def allocation-sources #{"claude-agent-sdk:usage-control-experimental" "claude-agent-sdk:rate-limit-event" "claude-code:statusline" "codex-app-server:account-rate-limits" "legacy-observation" "manual-policy" "policy-default"})

(def allocation-failure-reasons #{"anthropic_usage_capability_unavailable" "anthropic_usage_probe_failed" "anthropic_usage_probe_timed_out" "anthropic_usage_rate_limits_unavailable" "anthropic_usage_response_schema_changed" "anthropic_usage_windows_unavailable" "codex_usage_command_unavailable" "codex_usage_probe_failed" "codex_usage_probe_timed_out" "codex_usage_response_schema_changed" "codex_usage_subscription_auth_required" "codex_usage_transport_failed" "codex_usage_windows_unavailable"})

(def allocation-fields #{"target" "kind" "source" "observedAt" "limitId" "usedPercent" "resetsAt" "routingFloorPercent" "routingFloorExpiresAt" "measuredUsedPercent" "measurementSource" "measurementObservedAt" "collectionFailure"})

(defn valid-percent? [value]
  (and (number? value) (<= 0 value 100)))

(defn validate-allocation-evidence! [entries]
  (doseq [[_ raw] entries
   :let [record (parse-operation-json! "allocation evidence" raw)
   fields (set (keys record))]]
  (if (not (and (set/subset? #{"target" "kind" "source"} fields) (set/subset? fields allocation-fields))) (do
  (fail! "allocation evidence contains an unsupported field set" {:fields fields})))
  (if (not (and (string? (get record "target")) (re-matches identifier-pattern (get record "target")) (allocation-kinds (get record "kind")) (allocation-sources (get record "source")))) (do
  (fail! "allocation evidence contains invalid route identity" {})))
  (doseq [field ["observedAt" "resetsAt" "routingFloorExpiresAt" "measurementObservedAt"]
   :when (contains? record field)]
  (instant! (str "allocation " field) (get record field)))
  (doseq [field ["usedPercent" "routingFloorPercent" "measuredUsedPercent"]
   :when (contains? record field)]
  (if (not (valid-percent? (get record field))) (do
  (fail! "allocation evidence percentage is invalid" {:field field}))))
  (let [bind__1 (get record "measurementSource")]
  (if bind__1 (let [source bind__1]
  (do
  (if (not (allocation-sources source)) (do
  (fail! "allocation measurement source is invalid" {})))))))
  (let [bind__2 (get record "collectionFailure")]
  (if bind__2 (let [failure bind__2]
  (do
  (exact-json-fields! "allocation collection failure" failure #{"observedAt" "reason"})
  (instant! "allocation collection failure" (get failure "observedAt"))
  (if (not (allocation-failure-reasons (get failure "reason"))) (do
  (fail! "allocation collection failure reason is invalid" {})))))))))

(def fallback-fields #{"sequence" "reason" "fromTarget" "fromProvider" "toTarget" "toProvider" "phase" "replay" "proof"})

(def unsent-proof-fields #{"version" "durability" "mode" "source" "requestBytesPrepared" "requestBytesSent" "observableEvents"})

(defn validate-fallback-evidence! [entries]
  (doseq [[_ raw] entries
   :let [record (parse-operation-json! "fallback reason" raw)
   proof (get record "proof")]]
  (exact-json-fields! "fallback reason" record fallback-fields)
  (exact-json-fields! "fallback unsent proof" proof unsent-proof-fields)
  (if (not (and (integer? (get record "sequence")) (pos? (get record "sequence")) (= "provider_retry_safe_before_acceptance" (get record "reason")) (every? (fn [__north_anon_1] (and (string? __north_anon_1) (re-matches identifier-pattern __north_anon_1))) ((juxt (fn [__north_anon_1] (get __north_anon_1 "fromTarget")) (fn [__north_anon_1] (get __north_anon_1 "toTarget"))) record)) (every? #{"anthropic" "openai"} ((juxt (fn [__north_anon_1] (get __north_anon_1 "fromProvider")) (fn [__north_anon_1] (get __north_anon_1 "toProvider"))) record)) (= "preaccept" (get record "phase")) (= "proved_unsent" (get record "replay")) (= "north:provider-unsent-proof:v1" (get proof "version")) (= "adapter_receipt" (get proof "durability")) (#{"managed" "native"} (get proof "mode")) (#{"adapter_preflight" "managed_pre_thread_receipt" "native_supervisor_unavailable"} (get proof "source")) (integer? (get proof "requestBytesPrepared")) (<= 0 (get proof "requestBytesPrepared")) (zero? (get proof "requestBytesSent")) (zero? (get proof "observableEvents")))) (do
  (fail! "fallback reason contains invalid zero-send evidence" {})))
  (if (not (= (= "managed" (get proof "mode")) (boolean (#{"adapter_preflight" "managed_pre_thread_receipt"} (get proof "source"))))) (do
  (fail! "fallback unsent proof mode and source differ" {})))))

(defn validate-routing-pin-evidence! [entries]
  (doseq [[_ raw] entries
   :let [record (parse-operation-json! "routing pin" raw)]]
  (exact-json-fields! "routing pin" record #{"kind" "value"})
  (if (not (and (= "provider" (get record "kind")) (string? (get record "value")) (re-matches identifier-pattern (get record "value")))) (do
  (fail! "routing pin exposes an unsupported or invalid value" {})))))

(defn validate-operation-evidence! [receipt-entries aggregate-entries]
  (if (or (> (count receipt-entries) 512) (> (count aggregate-entries) 512)) (do
  (fail! "MCP operation evidence exceeds the bounded receipt limit" {})))
  (let [receipts (mapv (fn [[_ raw]] (let [record (parse-operation-json! "MCP operation receipt" raw)
   fields (set (keys record))]
  (if (not (or (= #{"tool" "operation" "durationMs" "outcome" "resultSize"} fields) (= #{"tool" "operation" "durationMs" "batchSize" "outcome" "resultSize"} fields))) (do
  (fail! "MCP operation receipt requires the exact v1 field set" {})))
  (if (not (and (string? (get record "tool")) (re-matches operation-tool-pattern (get record "tool")) (every? (fn [__north_anon_1] (and (string? __north_anon_1) (re-matches operation-component-pattern __north_anon_1))) ((juxt (fn [__north_anon_1] (get __north_anon_1 "operation")) (fn [__north_anon_1] (get __north_anon_1 "outcome"))) record)) (every? (fn [__north_anon_1] (and (integer? __north_anon_1) (<= 0 __north_anon_1))) (cond-> [(get record "durationMs") (get record "resultSize")] (contains? record "batchSize") (conj (get record "batchSize")))))) (do
  (fail! "MCP operation receipt contains invalid values" {:record record})))
  record)) receipt-entries)
   aggregates (mapv (fn [[_ raw]] (let [record (parse-operation-json! "MCP operation aggregate" raw)
   count' (get record "count")
   total (get record "totalDurationMs")
   mean (get record "meanDurationMs")
   failures (get record "failureCount")]
  (if (not (= #{"operation" "count" "totalDurationMs" "meanDurationMs" "failureCount"} (set (keys record)))) (do
  (fail! "MCP operation aggregate requires the exact v1 field set" {})))
  (if (not (and (string? (get record "operation")) (re-matches operation-component-pattern (get record "operation")) (integer? count') (pos? count') (integer? total) (<= 0 total) (number? mean) (= (double mean) (/ (double total) count')) (integer? failures) (<= 0 failures count'))) (do
  (fail! "MCP operation aggregate contains invalid values" {:record record})))
  record)) aggregate-entries)
   derived (reduce (fn [result receipt] (update result (get receipt "operation") (fnil (fn [entry] (-> entry (update "count" inc) (update "totalDurationMs" + (get receipt "durationMs")) (update "failureCount" + (if (= "ok" (get receipt "outcome")) 0 1)))) {"count" 0 "totalDurationMs" 0 "failureCount" 0}))) {} receipts)]
  (if (not (= (set (keys derived)) (set (map (fn [__north_anon_1] (get __north_anon_1 "operation")) aggregates)))) (do
  (fail! "MCP operation aggregates do not cover the exact receipt operations" {})))
  (if (not (= (count derived) (count aggregates))) (do
  (fail! "MCP operation aggregates must be unique by operation" {})))
  (doseq [aggregate aggregates
   :let [expected (get derived (get aggregate "operation"))]]
  (if (not (= expected (select-keys aggregate ["count" "totalDurationMs" "failureCount"]))) (do
  (fail! "MCP operation aggregate does not reconcile with receipts" {:operation (get aggregate "operation")}))))))

(defn validate-native-operation-evidence! [entries]
  (if (> (count entries) 32) (do
  (fail! "native command completion evidence exceeds the bounded receipt limit" {})))
  (doseq [[_ raw] entries
   :let [record (parse-operation-json! "native command completion" raw)]]
  (if (not (= #{"commandSha256" "outputSha256" "status" "exitCode" "shape" "durationMs"} (set (keys record)))) (do
  (fail! "native command completion requires the exact duration-bearing field set" {})))
  (if (not (and (every? (fn [__north_anon_1] (boolean (re-matches (re-pattern "[a-f0-9]{64}") (or __north_anon_1 "")))) ((juxt (fn [__north_anon_1] (get __north_anon_1 "commandSha256")) (fn [__north_anon_1] (get __north_anon_1 "outputSha256"))) record)) (#{"completed" "failed" "declined"} (get record "status")) (#{"read" "edit" "other"} (get record "shape")) (integer? (get record "exitCode")) (<= -2147483648 (get record "exitCode") 2147483647) (integer? (get record "durationMs")) (<= 0 (get record "durationMs")))) (do
  (fail! "native command completion contains invalid operation evidence" {})))))

(defn singleton-map! [facts]
  (let [grouped (group-by first facts)]
  (doseq [[predicate entries] grouped]
  (if (and (not (multi-predicates predicate)) (> (count entries) 1)) (do
  (fail! "wire run telemetry predicates must be singleton" {:predicate predicate :values (mapv second entries)}))))
  (into {} (keep (fn [[predicate entries]] (if (not (multi-predicates predicate)) (do
  [predicate (second (first entries))])))) grouped)))

(defn nonnegative-count! [predicate value]
  (let [parsed (parse-long value)]
  (if (not (and parsed (<= 0 parsed) (<= parsed 9007199254740991))) (do
  (fail! "invalid wire run telemetry count" {:predicate predicate :value value})))
  parsed))

(def mcp-activity-predicates #{"mcp_activity_source" "mcp_activity_coverage" "mcp_actual_calls" "mcp_actual_tool" "mcp_operation_receipt" "mcp_operation_aggregate"})

(def native-activity-predicates #{"native_command_activity_source" "native_command_activity_coverage" "native_north_binary_probe" "native_command_total" "native_command_successful" "native_command_failed" "native_command_declined" "native_command_open" "native_command_truncated" "native_command_read" "native_command_edit" "native_command_completion"})

(defn validate-activity-summaries! [scalar grouped]
  (let [fields (set (keys grouped))
   mcp-fields (set/intersection fields mcp-activity-predicates)
   mcp-coverage (get scalar "mcp_activity_coverage")]
  (if (seq mcp-fields) (do
  (if (not (and (re-matches identifier-pattern (or (get scalar "mcp_activity_source") "")) (#{"exact" "partial" "unknown"} mcp-coverage))) (do
  (fail! "MCP activity requires a valid source and coverage" {})))
  (if (and (= "exact" mcp-coverage) (nil? (get scalar "mcp_actual_calls"))) (do
  (fail! "exact MCP activity requires an observed call count" {})))
  (if (and (= "unknown" mcp-coverage) (seq (set/intersection fields (disj mcp-activity-predicates "mcp_activity_source" "mcp_activity_coverage")))) (do
  (fail! "unknown MCP activity cannot carry terminal evidence" {}))))))
  (let [fields (set (keys grouped))
   native-fields (set/intersection fields native-activity-predicates)
   coverage (get scalar "native_command_activity_coverage")
   probe (get scalar "native_north_binary_probe")]
  (if (seq native-fields) (do
  (if (not (and (re-matches identifier-pattern (or (get scalar "native_command_activity_source") "")) (#{"exact" "partial" "unknown"} coverage) (#{"passed" "failed" "not_observed"} probe))) (do
  (fail! "native command activity requires a valid source, coverage, and probe" {})))
  (if (and (= "unknown" coverage) (seq (set/intersection fields (disj native-activity-predicates "native_command_activity_source" "native_command_activity_coverage" "native_north_binary_probe")))) (do
  (fail! "unknown native command activity cannot carry terminal evidence" {})))
  (if (and (= "exact" coverage) (nil? (get scalar "native_command_total"))) (do
  (fail! "exact native command activity requires an observed command count" {})))
  (let [bind__5 (some-> (get scalar "native_command_total") parse-long)]
  (if bind__5 (let [total bind__5]
  (do
  (let [successful (let [bind__7 (some-> (get scalar "native_command_successful") parse-long)]
  (if bind__7 (let [value bind__7]
  value) 0))
   failed (let [bind__9 (some-> (get scalar "native_command_failed") parse-long)]
  (if bind__9 (let [value bind__9]
  value) 0))
   declined (let [bind__11 (some-> (get scalar "native_command_declined") parse-long)]
  (if bind__11 (let [value bind__11]
  value) 0))
   open (let [bind__13 (some-> (get scalar "native_command_open") parse-long)]
  (if bind__13 (let [value bind__13]
  value) 0))
   settled (+ successful failed declined open)]
  (if (not (= total settled)) (do
  (fail! "native command activity counts do not reconcile" {}))))))))))))

(def estimate-hours-pattern #"^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$")

(def estimate-predicates #{"estimate_hours" "estimate_delta_ms" "estimate_ratio" "estimate_classification"})

(defn canonical-estimate-ratio! [actual-ms estimated-ms]
  (let [spec (get-in north.run-ledger/contract ["telemetry" "estimateRatio"])
   scale (get spec "scale")
   scale-text (str scale)]
  (if (not (and (integer? scale) (pos? scale) (re-matches #"10*" scale-text) (= "nearest-half-up" (get spec "rounding")) (= "omit" (get spec "trailingFractionZeros")))) (do
  (fail! "invalid wire run estimate ratio contract" {})))
  (let [scale' (bigint scale)
   denominator (bigint estimated-ms)
   numerator (* (bigint actual-ms) scale')
   quotient (quot numerator denominator)
   remainder (rem numerator denominator)
   rounded (+ quotient (if (>= (* 2 remainder) denominator) 1 0))
   whole (quot rounded scale')
   fraction-raw (str (mod rounded scale'))
   fraction-padded (str (apply str (repeat (- (dec (count scale-text)) (count fraction-raw)) "0")) fraction-raw)
   fraction (str/replace fraction-padded #"0+$" "")]
  (if (str/blank? fraction) (str whole) (str whole "." fraction)))))

(defn validate-estimate! [scalar grouped]
  (let [present (set/intersection estimate-predicates (set (keys grouped)))]
  (if (seq present) (do
  (if (not (= estimate-predicates present)) (do
  (fail! "run estimate comparison requires its complete fact set" {:fields present})))
  (let [hours (get scalar "estimate_hours")
   parsed (if (re-matches estimate-hours-pattern (or hours "")) (do
  (try
  (Double/parseDouble hours)
  (catch Exception _
    nil))))
   estimated-ms (if (and parsed (Double/isFinite parsed) (pos? parsed)) (do
  (Math/round (* parsed 3600000.0))))
   actual-ms (parse-long (get scalar "duration_ms"))
   delta-ms (parse-long (get scalar "estimate_delta_ms"))]
  (if (not (and estimated-ms (pos? estimated-ms) (<= estimated-ms 9007199254740991))) (do
  (fail! "run estimate hours is not positive and finite" {:value hours})))
  (let [bind__15 actual-ms]
  (if bind__15 (let [actual bind__15]
  (let [bind__16 estimated-ms]
  (if bind__16 (let [estimated bind__16]
  (let [bind__17 delta-ms]
  (if bind__17 (let [delta bind__17]
  (let [expected-delta (- actual estimated)
   ^String expected-classification (cond
  (neg? expected-delta) "under"
  (pos? expected-delta) "over"
  :else "on")]
  (if (not (and (= expected-delta delta) (= (canonical-estimate-ratio! actual estimated) (get scalar "estimate_ratio")) (= expected-classification (get scalar "estimate_classification")))) (do
  (fail! "run estimate comparison differs from terminal duration" {}))))) (fail! "run estimate comparison differs from terminal duration" {})))) (fail! "run estimate comparison differs from terminal duration" {})))) (fail! "run estimate comparison differs from terminal duration" {}))))))))

(def struggle-predicates #{"error_count" "struggle" "struggle_detector_policy_version" "struggle_topology" "struggle_error_streak_threshold" "struggle_loop_repeat_threshold" "struggle_loop_window" "struggle_no_progress_turn_threshold"})

(def struggle-required-predicates (disj struggle-predicates "struggle"))

(def struggle-trigger-values #{"consecutive_errors" "tool_loop" "no_progress"})

(def judgment-grade-predicates #{"judgment_grade" "judgment_grade_status" "judgment_grade_source"})

(defn validate-judgment-grade! [scalar grouped]
  (let [present (set/intersection judgment-grade-predicates (set (keys grouped)))]
  (if (seq present) (do
  (if (not (set/subset? #{"judgment_grade_status" "judgment_grade_source"} present)) (do
  (fail! "judgment grade observation requires status and source" {:fields present})))
  (let [grade (get scalar "judgment_grade")
   status (get scalar "judgment_grade_status")
   source (get scalar "judgment_grade_source")
   valid (and (= "valid" status) (= "thread" source) (#{"s" "m" "l"} grade))
   unavailable (and (= "unavailable" status) (nil? grade) (#{"thread" "ad-hoc"} source))
   invalid (and (= "invalid" status) (nil? grade) (= "thread" source))]
  (if (not (or valid unavailable invalid)) (do
  (fail! "invalid run-local judgment grade observation" {}))))))))

(defn validate-struggle! [scalar grouped]
  (let [present (set/intersection struggle-predicates (set (keys grouped)))]
  (if (seq present) (do
  (if (not (set/subset? struggle-required-predicates present)) (do
  (fail! "struggle observation requires its complete policy fact set" {:fields present})))
  (if (not (= "north:struggle-observer:v2" (get scalar "struggle_detector_policy_version"))) (do
  (fail! "unsupported struggle detector policy version" {})))
  (if (not (#{"worker" "orchestrator"} (get scalar "struggle_topology"))) (do
  (fail! "invalid struggle topology" {})))
  (let [error-count (parse-long (get scalar "error_count"))
   error-streak (parse-long (get scalar "struggle_error_streak_threshold"))
   loop-repeat (parse-long (get scalar "struggle_loop_repeat_threshold"))
   loop-window (parse-long (get scalar "struggle_loop_window"))
   no-progress (parse-long (get scalar "struggle_no_progress_turn_threshold"))
   thresholds [error-streak loop-repeat loop-window no-progress]
   triggers (mapv second (get grouped "struggle" []))]
  (if (not (and error-count (<= 0 error-count) (every? (fn [__north_anon_1] (and __north_anon_1 (<= 1 __north_anon_1 1000))) thresholds) (<= loop-repeat loop-window))) (do
  (fail! "invalid struggle observation counts or thresholds" {})))
  (if (not (and (= (count triggers) (count (set triggers))) (every? struggle-trigger-values triggers))) (do
  (fail! "invalid struggle trigger observation" {}))))))))

(def shadow-reviewer-common-predicates #{"shadow_reviewer_version" "shadow_reviewer_target"})

(def shadow-reviewer-summary-required-predicates (set/union shadow-reviewer-common-predicates #{"shadow_reviewer_status" "shadow_reviewer_eligible_updates" "shadow_reviewer_reviewed_updates" "shadow_reviewer_dropped_updates" "shadow_reviewer_emitted_notes" "shadow_reviewer_quarantined_outputs" "shadow_reviewer_failed_reviews" "shadow_reviewer_usage_status" "shadow_reviewer_duration_ms"}))

(def shadow-reviewer-execution-predicates (set/union shadow-reviewer-common-predicates #{"shadow_reviewer_source_run" "shadow_reviewer_source_from_sequence" "shadow_reviewer_source_through_sequence" "shadow_reviewer_privacy_omitted_events" "shadow_reviewer_capacity_omitted_events" "shadow_reviewer_input_sha256"}))

(def shadow-reviewer-predicates (set/union shadow-reviewer-summary-required-predicates shadow-reviewer-execution-predicates #{"shadow_reviewer_tokens"}))

(def shadow-reviewer-target-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$")

(def shadow-reviewer-wire-id-pattern #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$")

(def shadow-reviewer-statuses #{"not_assigned" "completed" "partial" "aborted"})

(def shadow-reviewer-usage-statuses #{"exact" "partial" "unknown_incomplete_terminal" "unknown_no_terminal" "unknown_provider" "unknown_overflow"})

(defn validate-shadow-reviewer-summary! [scalar present]
  (let [status (get scalar "shadow_reviewer_status")
   usage-status (get scalar "shadow_reviewer_usage_status")
   exact-usage? (= "exact" usage-status)
   expected (cond-> shadow-reviewer-summary-required-predicates exact-usage? (conj "shadow_reviewer_tokens"))]
  (if (not (= expected present)) (do
  (fail! "shadow reviewer summary requires its exact fact set" {:fields present})))
  (if (not (and (shadow-reviewer-statuses status) (shadow-reviewer-usage-statuses usage-status))) (do
  (fail! "invalid shadow reviewer summary status" {})))
  (let [eligible (nonnegative-count! "shadow_reviewer_eligible_updates" (get scalar "shadow_reviewer_eligible_updates"))
   reviewed (nonnegative-count! "shadow_reviewer_reviewed_updates" (get scalar "shadow_reviewer_reviewed_updates"))
   dropped (nonnegative-count! "shadow_reviewer_dropped_updates" (get scalar "shadow_reviewer_dropped_updates"))
   emitted (nonnegative-count! "shadow_reviewer_emitted_notes" (get scalar "shadow_reviewer_emitted_notes"))
   quarantined (nonnegative-count! "shadow_reviewer_quarantined_outputs" (get scalar "shadow_reviewer_quarantined_outputs"))
   failed (nonnegative-count! "shadow_reviewer_failed_reviews" (get scalar "shadow_reviewer_failed_reviews"))
   duration (nonnegative-count! "shadow_reviewer_duration_ms" (get scalar "shadow_reviewer_duration_ms"))
   tokens (if exact-usage? (do
  (nonnegative-count! "shadow_reviewer_tokens" (get scalar "shadow_reviewer_tokens"))))
   handled (+ reviewed dropped)
   surfaced (+ emitted quarantined)
   all-zero? (every? zero? [eligible reviewed dropped emitted quarantined failed duration (or tokens 0)])]
  (if (not (and (<= handled 9007199254740991) (<= handled eligible) (<= surfaced 9007199254740991) (<= surfaced reviewed) (<= failed eligible))) (do
  (fail! "shadow reviewer summary counts do not reconcile" {})))
  (if (and (= "not_assigned" status) (not (and exact-usage? all-zero?))) (do
  (fail! "inactive shadow reviewer summary carries activity" {})))
  (if (and (= "completed" status) (not (and exact-usage? (= reviewed eligible) (zero? dropped) (zero? quarantined) (zero? failed)))) (do
  (fail! "completed shadow reviewer summary carries incomplete work" {})))
  (if (and (= "partial" status) exact-usage? (zero? dropped) (zero? quarantined) (zero? failed)) (do
  (fail! "partial shadow reviewer summary lacks partial evidence" {}))))))

(defn validate-shadow-reviewer-execution! [scalar present grouped]
  (if (not (= shadow-reviewer-execution-predicates present)) (do
  (fail! "shadow reviewer execution requires its exact fact set" {:fields present})))
  (let [source-run (get scalar "shadow_reviewer_source_run")
   from-sequence (nonnegative-count! "shadow_reviewer_source_from_sequence" (get scalar "shadow_reviewer_source_from_sequence"))
   through-sequence (nonnegative-count! "shadow_reviewer_source_through_sequence" (get scalar "shadow_reviewer_source_through_sequence"))
   learning-fields (set/intersection learning-keys (set (keys grouped)))]
  (if (not (and (re-matches shadow-reviewer-wire-id-pattern (or source-run "")) (<= from-sequence through-sequence) (re-matches digest-pattern (or (get scalar "shadow_reviewer_input_sha256") "")))) (do
  (fail! "invalid shadow reviewer source evidence" {})))
  (if (not (= (get scalar "parent_run") (north.run-ledger/run-summary-subject! source-run))) (do
  (fail! "shadow reviewer execution is not linked to its source run" {})))
  (if (not (= "shadow-reviewer" (get scalar "role"))) (do
  (fail! "shadow reviewer execution requires its isolated role" {})))
  (if (seq learning-fields) (do
  (fail! "shadow reviewer execution cannot carry a learning assignment" {:fields learning-fields})))
  (let [admitted (nonnegative-count! "tool_admitted_count" (get scalar "tool_admitted_count"))
   succeeded (nonnegative-count! "tool_succeeded_count" (get scalar "tool_succeeded_count"))
   failed (nonnegative-count! "tool_failed_count" (get scalar "tool_failed_count"))
   cancelled (nonnegative-count! "tool_cancelled_count" (get scalar "tool_cancelled_count"))
   synthetic (nonnegative-count! "tool_synthetic_failure_count" (get scalar "tool_synthetic_failure_count"))
   zero-tools? (every? zero? [admitted succeeded failed cancelled synthetic])
   truthful-violation? (and (pos? admitted) (= admitted synthetic) (every? zero? [succeeded failed cancelled]) (= "failed" (get scalar "wire_run_lifecycle")))]
  (if (not (or zero-tools? truthful-violation?)) (do
  (fail! "shadow reviewer tool evidence is not a truthful data-only failure" {}))))))

(defn validate-shadow-reviewer! [scalar grouped]
  (let [present (set/intersection shadow-reviewer-predicates (set (keys grouped)))]
  (if (seq present) (do
  (if (not (and (= "north-shadow-reviewer:v1" (get scalar "shadow_reviewer_version")) (re-matches shadow-reviewer-target-pattern (or (get scalar "shadow_reviewer_target") "")))) (do
  (fail! "invalid shadow reviewer identity" {})))
  (cond
  (contains? present "shadow_reviewer_status") (validate-shadow-reviewer-summary! scalar present)
  (contains? present "shadow_reviewer_source_run") (validate-shadow-reviewer-execution! scalar present grouped)
  :else (fail! "shadow reviewer evidence is incomplete" {:fields present}))))))

(def provider-join-predicates #{"provider_join_key_version" "provider_join_coverage" "provider_session_key" "provider_turn_key"})

(def turn-evidence-predicates #{"num_turns" "provider_turn_units" "provider_tool_items" "provider_turn_metric_comparable"})

(def execution-observation-version "agent-execution-observation/v1")

(def execution-modes #{"standard" "fast"})

(def execution-observation-token-pattern #"^[a-z0-9][a-z0-9._:/-]*$")

(def watchdog-predicates #{"watchdog_reason" "watchdog_silence_ms" "watchdog_last_outer_activity" "watchdog_last_provider_activity"})

(def execution-transports #{"sdk-stream" "managed-app-server" "cli-jsonl"})

(def watchdog-activity-kinds {"outer" #{"message" "model" "tool" "artifact" "compaction" "activity"} "provider" #{"turn" "item" "tool" "progress" "message" "activity"}})

(defn nonnegative-bigint! [label value]
  (let [parsed (try
  (bigint value)
  (catch Exception _
    nil))]
  (if (not (and parsed (<= 0 parsed))) (do
  (fail! (str label " must be a nonnegative integer") {:value value})))
  parsed))

(defn canonical-usage-total! [scalar]
  (let [input (nonnegative-bigint! "lifetime input tokens" (get scalar "lifetime_input_tokens"))
   output (nonnegative-bigint! "lifetime output tokens" (get scalar "lifetime_output_tokens"))
   cache-read (nonnegative-bigint! "lifetime cache-read tokens" (get scalar "lifetime_cache_read_tokens"))
   cache-write (nonnegative-bigint! "lifetime cache-write tokens" (get scalar "lifetime_cache_write_tokens"))]
  (case (get scalar "provider")
    "anthropic" (+ input output cache-read cache-write)
    "openai" (+ input output)
    nil)))

(defn validate-usage-coverage! [scalar]
  (let [terminal-count (parse-long (get scalar "usage_terminal_count"))
   scope (get scalar "usage_scope")
   status (get scalar "usage_total_status")
   tokens (let [bind__20 (get scalar "tokens")]
  (if bind__20 (let [raw bind__20]
  (do
  (nonnegative-bigint! "exact tokens" raw)))))]
  (if (not (= "wire_run_cumulative" scope)) (do
  (fail! "usage scope is not the canonical Wire run cumulative scope" {})))
  (case status
    "exact" (let [expected (canonical-usage-total! scalar)]
  (if (not (and terminal-count (pos? terminal-count) tokens expected (= tokens expected))) (do
  (fail! "exact usage requires an authoritative provider terminal and exact formula" {}))))
    "partial" (if tokens (do
  (fail! "partial usage cannot publish an exact aggregate" {})))
    "unknown_incomplete_terminal" (if (not (and terminal-count (pos? terminal-count) (nil? tokens))) (do
  (fail! "incomplete provider terminal usage must remain unknown" {})))
    "unknown_no_terminal" (if (not (and terminal-count (zero? terminal-count) (nil? tokens))) (do
  (fail! "usage without a provider terminal must remain unknown" {})))
    (fail! "usage total status is invalid" {:status status}))))

(defn validate-watchdog-activity! [label raw origin]
  (if (not (= "none" raw)) (do
  (let [record (parse-operation-json! label raw)]
  (exact-json-fields! label record #{"origin" "kind" "observedAt"})
  (if (not (and (= origin (get record "origin")) ((get watchdog-activity-kinds origin) (get record "kind")))) (do
  (fail! (str label " contains invalid provider-neutral activity") {})))
  (instant! label (get record "observedAt"))))))

(defn validate-core-evidence! [scalar grouped]
  (let [fields (set (keys grouped))
   thread (get scalar "thread")
   thread-provenance (get scalar "thread_provenance")]
  (validate-usage-coverage! scalar)
  (if (not (= thread-provenance (if (= "(ad-hoc)" thread) "ad-hoc" "exact"))) (do
  (fail! "thread provenance differs from the exact graph identity" {})))
  (let [source (get scalar "execution_source")
   transport (get scalar "execution_transport")]
  (if (not (or (nil? source) (#{"north-managed" "provider-native"} source))) (do
  (fail! "execution source is invalid" {})))
  (if (and transport (nil? source)) (do
  (fail! "execution transport requires an execution source" {})))
  (if (and transport (not (execution-transports transport))) (do
  (fail! "execution transport is invalid" {}))))
  (let [join-fields (set/intersection fields provider-join-predicates)
   persistence (get scalar "provider_session_persistence")
   coverage (get scalar "provider_join_coverage")
   session-key (get scalar "provider_session_key")
   turn-keys (mapv second (get grouped "provider_turn_key" []))]
  (if (not (#{"persisted" "ephemeral" "unknown"} persistence)) (do
  (fail! "provider session persistence is invalid" {})))
  (if (seq join-fields) (do
  (if (not (set/subset? #{"provider_join_key_version" "provider_join_coverage"} join-fields)) (do
  (fail! "provider join evidence requires version and coverage" {})))
  (if (not (= "north-provider-join:v1" (get scalar "provider_join_key_version"))) (do
  (fail! "provider join evidence has an unsupported version" {})))
  (if (not (#{"exact" "partial" "unknown"} coverage)) (do
  (fail! "provider join coverage is invalid" {})))
  (if (and session-key (nil? (re-matches digest-pattern session-key))) (do
  (fail! "provider session key is not a privacy-bounded digest" {})))
  (if (not (and (= (count turn-keys) (count (set turn-keys))) (every? (fn [__north_anon_1] (re-matches digest-pattern __north_anon_1)) turn-keys))) (do
  (fail! "provider turn keys are not unique privacy-bounded digests" {})))
  (if (and (= "exact" coverage) (or (nil? session-key) (empty? turn-keys))) (do
  (fail! "exact provider join evidence requires session and turn keys" {})))
  (if (and (= "partial" coverage) (nil? session-key) (empty? turn-keys)) (do
  (fail! "partial provider join evidence requires a bounded join key" {})))) (if (not (= "unknown" persistence)) (do
  (fail! "session persistence without join evidence must be unknown" {})))))
  (let [provenance (get scalar "turn_provenance")
   turn-fields (set/intersection fields turn-evidence-predicates)
   num-turns (get scalar "num_turns")
   provider-units (get scalar "provider_turn_units")
   provider-tools (get scalar "provider_tool_items")
   comparable (get scalar "provider_turn_metric_comparable")]
  (if (not (#{"provider-terminal" "pre-provider" "unknown"} provenance)) (do
  (fail! "turn provenance is invalid" {})))
  (if (and num-turns provider-units) (do
  (fail! "assistant turns and provider turn units are not comparable" {})))
  (if (and provider-tools (nil? provider-units)) (do
  (fail! "provider tool items require provider turn units" {})))
  (if (not (= (some? provider-units) (= "false" comparable))) (do
  (fail! "provider turn units require their non-comparability disclaimer" {})))
  (case provenance
    "provider-terminal" nil
    "pre-provider" (if (not (and (= "0" num-turns) (= #{"num_turns"} turn-fields))) (do
  (fail! "pre-provider turn provenance requires exact zero assistant turns" {})))
    "unknown" (if (seq turn-fields) (do
  (fail! "unknown turn provenance cannot carry terminal turn evidence" {})))
    (throw (IllegalArgumentException. (str "No matching clause: " provenance)))))
  (let [observation (parse-operation-json! "execution observation" (get scalar "execution_observation"))
   coverage (get observation "coverage")
   segments (get observation "segments")
   evidence (get observation "evidence")]
  (if (not (= execution-observation-version (get observation "version"))) (do
  (fail! "execution observation version is unsupported" {})))
  (if (not (and (string? (get observation "source")) (re-matches execution-observation-token-pattern (get observation "source")))) (do
  (fail! "execution observation source is invalid" {})))
  (case coverage
    "unknown" (do
  (exact-json-fields! "unknown execution observation" observation #{"coverage" "turn_unit" "segments" "source" "version" "tool_call_unit" "evidence"})
  (if (not (and (= "unknown" (get observation "turn_unit")) (= "unknown" (get observation "tool_call_unit")) (map? evidence) (empty? evidence) (sequential? segments) (empty? segments))) (do
  (fail! "unknown execution observation cannot contain units, evidence, or segments" {}))))
    "exact" (do
  (exact-json-fields! "exact execution observation" observation #{"coverage" "turn_unit" "segments" "source" "version" "tool_call_unit" "evidence"})
  (if (not (and (= "assistant-turn" (get observation "turn_unit")) (= "admitted-tool-call" (get observation "tool_call_unit")))) (do
  (fail! "exact execution observation units are not comparable" {})))
  (exact-json-fields! "execution observation evidence" evidence #{"session_sha256" "provider" "attempt_sha256"})
  (if (not (and (string? (get evidence "provider")) (re-matches execution-observation-token-pattern (get evidence "provider")) (re-matches digest-pattern (or (get evidence "attempt_sha256") "")) (re-matches digest-pattern (or (get evidence "session_sha256") "")))) (do
  (fail! "execution observation evidence is invalid" {})))
  (if (not (and (sequential? segments) (seq segments))) (do
  (fail! "exact execution observation requires segments" {})))
  (loop [remaining segments
   preceding-mode nil
   turns 0
   tools 0
   seen-turns #{}]
  (let [bind__21 (first remaining)]
  (if bind__21 (let [segment bind__21]
  (do
  (exact-json-fields! "execution observation segment" segment #{"turn_count" "mode" "turn_sha256" "tool_call_count"})
  (let [mode (get segment "mode")
   segment-turns (get segment "turn_count")
   segment-tools (get segment "tool_call_count")
   turn-keys (get segment "turn_sha256")]
  (if (not (execution-modes mode)) (do
  (fail! "execution observation segment mode is invalid" {})))
  (if (= preceding-mode mode) (do
  (fail! "execution observation contains adjacent equal modes" {})))
  (if (not (and (integer? segment-turns) (<= 1 segment-turns 9007199254740991) (integer? segment-tools) (<= 0 segment-tools 9007199254740991) (<= (+ turns segment-turns) 9007199254740991) (<= (+ tools segment-tools) 9007199254740991) (sequential? turn-keys) (= segment-turns (count turn-keys)) (= (count turn-keys) (count (set turn-keys))) (empty? (set/intersection seen-turns (set turn-keys))) (every? (fn [__north_anon_1] (and (string? __north_anon_1) (re-matches digest-pattern __north_anon_1))) turn-keys))) (do
  (fail! "execution observation counts exceed the safe range" {})))
  (recur (next remaining) mode (+ turns segment-turns) (+ tools segment-tools) (into seen-turns turn-keys)))))))))
    (fail! "execution observation coverage is invalid" {})))
  (let [present (set/intersection fields watchdog-predicates)]
  (if (seq present) (do
  (if (not (= watchdog-predicates present)) (do
  (fail! "watchdog evidence requires its complete replay-derived fact set" {})))
  (if (not (= "north_watchdog_execution_inactivity" (get scalar "watchdog_reason"))) (do
  (fail! "watchdog reason is not canonical" {})))
  (let [silence-ms (parse-long (get scalar "watchdog_silence_ms"))]
  (if (not (and silence-ms (pos? silence-ms))) (do
  (fail! "watchdog silence must be positive" {}))))
  (validate-watchdog-activity! "watchdog outer activity" (get scalar "watchdog_last_outer_activity") "outer")
  (validate-watchdog-activity! "watchdog provider activity" (get scalar "watchdog_last_provider_activity") "provider"))))))

(defn validate-summary-facts! [subject facts]
  (let [scalar (singleton-map! facts)
   fields (set (map first facts))
   unknown (seq (remove terminal-predicates fields))
   missing (seq (remove (fn [__north_anon_1] (contains? scalar __north_anon_1)) required-predicates))
   count' (nonnegative-count! "wire_event_count" (get scalar "wire_event_count"))
   first-sequence (nonnegative-count! "wire_event_first_sequence" (get scalar "wire_event_first_sequence"))
   last-sequence (nonnegative-count! "wire_event_last_sequence" (get scalar "wire_event_last_sequence"))]
  (if unknown (do
  (fail! "wire run telemetry contains unknown predicates" {:predicates unknown})))
  (if missing (do
  (fail! "wire run telemetry is missing required predicates" {:predicates missing})))
  (if (not (= "run" (get scalar "kind"))) (do
  (fail! "wire run telemetry requires kind=run" {})))
  (if (not (= north.run-ledger/version (get scalar "wire_ledger_version"))) (do
  (fail! "unsupported wire ledger version" {})))
  (if (not (= north.run-ledger/wire-version (get scalar "wire_version"))) (do
  (fail! "unsupported wire version" {})))
  (if (not (= "complete" (get scalar "wire_ledger_status"))) (do
  (fail! "wire run summary cannot claim an unavailable ledger" {})))
  (if (not (and (pos? count') (zero? first-sequence) (= last-sequence (dec count')))) (do
  (fail! "wire run telemetry sequence summary is inconsistent" {})))
  (doseq [predicate count-predicates
   :when (contains? scalar predicate)]
  (nonnegative-count! predicate (get scalar predicate)))
  (if (not (and (re-matches identifier-pattern (get scalar "agent")) (or (= "(ad-hoc)" (get scalar "thread")) (north.terminal-projection/valid-thread-entity? (get scalar "thread"))))) (do
  (fail! "wire run telemetry graph identity is invalid" {})))
  (let [bind__22 (get scalar "parent_run")]
  (if bind__22 (let [parent-run bind__22]
  (do
  (if (not (north.terminal-projection/valid-run-entity? parent-run)) (do
  (fail! "wire run telemetry parent run is invalid" {})))))))
  (let [bind__23 (get scalar "parent_thread")]
  (if bind__23 (let [parent-thread bind__23]
  (do
  (if (not (north.terminal-projection/valid-thread-entity? parent-thread)) (do
  (fail! "wire run telemetry parent thread is invalid" {})))))))
  (let [bind__24 (get scalar "run_coordinator")]
  (if bind__24 (let [coordinator bind__24]
  (do
  (if (nil? (re-matches identifier-pattern coordinator)) (do
  (fail! "wire run telemetry coordinator is invalid" {})))))))
  (if (not (#{"completed" "failed" "cancelled" "blocked"} (get scalar "wire_run_lifecycle"))) (do
  (fail! "wire run telemetry lifecycle is not terminal" {})))
  (if (not (and (re-matches identifier-pattern (get scalar "wire_termination_code")) (re-matches identifier-pattern (get scalar "outcome")) (re-matches #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$" (get scalar "wire_terminal_event_id")) (re-matches digest-pattern (get scalar "wire_ledger_sha256")))) (do
  (fail! "wire run telemetry terminal identity is invalid" {})))
  (instant! "at" (get scalar "at"))
  (instant! "started_at" (get scalar "started_at"))
  (if (not (= subject (north.run-ledger/run-summary-subject! (get scalar "wire_run_id")))) (do
  (fail! "wire run telemetry subject does not match its exact wire run id" {})))
  (validate-core-evidence! scalar (group-by first facts))
  scalar))

(defn thread-entity! [raw]
  (if (and (string? raw) (not= raw "(ad-hoc)")) (do
  (let [canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
  (if (not (north.terminal-projection/valid-thread-entity? canonical)) (do
  (fail! "invalid wire run telemetry thread" {:thread raw})))
  canonical))))

(defn canonical-record [record]
  (json/generate-string (into (sorted-map) record)))

(defn validate-reported-run! [port subject scalar delivery-facts run-facts]
  (if (= "reported" (get delivery-facts "delivery_outcome")) (do
  (let [evidence (json/parse-string (get delivery-facts "delivery_evidence"))
   expected-reporter (str "@agent:" (get scalar "agent"))
   expected-thread (thread-entity! (get scalar "thread"))
   reservation-origin (north.terminal-projection/singleton-value run-facts "run_reservation_contract_origin")
   reservation-baseline (north.terminal-projection/run-reservation-done-when run-facts)
   current-bars (north.terminal-projection/canonical-done-when (facts-of port expected-thread))
   records (set (mapcat (fn [match] (map canonical-record (get match "evidence"))) (get evidence "matches")))
   evidence-state (north.terminal-projection/run-evidence-state run-facts subject expected-thread expected-reporter)
   stored-records (:raws evidence-state)]
  (if (not (north.terminal-projection/run-reservation-valid? run-facts)) (do
  (fail! "reported run lost its committed reservation" {:subject subject})))
  (if (not (= #{expected-reporter} (get run-facts "run_reservation_agent"))) (do
  (fail! "run telemetry agent does not match its reservation" {:subject subject})))
  (if (not (= #{expected-thread} (get run-facts "run_reservation_thread"))) (do
  (fail! "run telemetry thread does not match its reservation" {:subject subject})))
  (if (not (= expected-reporter (get evidence "reporter"))) (do
  (fail! "run evidence reporter must match its managed agent" {:subject subject})))
  (if (not (= subject (get evidence "run"))) (do
  (fail! "run evidence must name the exact committed run subject" {:subject subject})))
  (if (not (= expected-thread (get evidence "thread"))) (do
  (fail! "run evidence must name the exact driven thread" {:subject subject})))
  (if (not (= reservation-origin (get evidence "contractOrigin"))) (do
  (fail! "run delivery contract origin differs from its reservation" {:subject subject})))
  (if (not (= reservation-baseline (get evidence "baselineDoneWhen"))) (do
  (fail! "run delivery baseline differs from its reservation" {:subject subject})))
  (if (not (= current-bars (get evidence "doneWhen"))) (do
  (fail! "run delivery contract changed before telemetry publication" {:subject subject})))
  (if (not (:valid? evidence-state)) (do
  (fail! "reported run contains malformed or cross-scoped evidence" {:subject subject})))
  (if (not (= stored-records records)) (do
  (fail! "run delivery snapshot must cite the exact stored evidence set" {:subject subject})))))))

(defn durable-optional-event-facts! [port run-id predicate]
  (let [response (north.coord/bounded-query-in-domain port :telemetry {:find "durable_wire_event_optional_fact" :rules [{:head {:rel "durable_wire_event_optional_fact" :args [{:var "e"} {:var "value"}]} :body [{:rel "triple" :args [{:var "e"} "kind" "wire_event"]} {:rel "triple" :args [{:var "e"} "wire_run_id" run-id]} {:rel "triple" :args [{:var "e"} predicate {:var "value"}]}]}]} (inc north.run-ledger/max-events))
   grouped (group-by first (:rows response))]
  (doseq [[subject rows] grouped]
  (if (not (= 1 (count rows))) (do
  (fail! "durable wire event optional lineage is not singleton" {:subject subject :predicate predicate}))))
  (into {} (map (fn [[subject rows]] [subject (second (first rows))]) grouped))))

(defn durable-wire-events! [port run-id]
  (let [response (north.coord/bounded-query-in-domain port :telemetry {:find "durable_wire_event" :rules [{:head {:rel "durable_wire_event" :args [{:var "e"} {:var "sequence"} {:var "id"} {:var "at"} {:var "kind"} {:var "essential"} {:var "json"} {:var "digest"} {:var "thread"} {:var "agent"}]} :body [{:rel "triple" :args [{:var "e"} "kind" "wire_event"]} {:rel "triple" :args [{:var "e"} "wire_run_id" run-id]} {:rel "triple" :args [{:var "e"} "wire_event_sequence" {:var "sequence"}]} {:rel "triple" :args [{:var "e"} "wire_event_id" {:var "id"}]} {:rel "triple" :args [{:var "e"} "wire_event_at" {:var "at"}]} {:rel "triple" :args [{:var "e"} "wire_event_kind" {:var "kind"}]} {:rel "triple" :args [{:var "e"} "wire_event_essential" {:var "essential"}]} {:rel "triple" :args [{:var "e"} "wire_event_json" {:var "json"}]} {:rel "triple" :args [{:var "e"} "wire_event_sha256" {:var "digest"}]} {:rel "triple" :args [{:var "e"} "thread" {:var "thread"}]} {:rel "triple" :args [{:var "e"} "agent" {:var "agent"}]}]}]} (inc north.run-ledger/max-events))
   parent-threads (durable-optional-event-facts! port run-id "parent_thread")
   coordinators (durable-optional-event-facts! port run-id "run_coordinator")]
  (->> (:rows response) (mapv (fn [[subject sequence id at kind essential raw digest thread agent]] (let [parsed-sequence (parse-long sequence)]
  (if (not (and parsed-sequence (= digest (north.run-ledger/sha256 raw)) (= subject (north.run-ledger/event-subject run-id parsed-sequence)))) (do
  (fail! "durable wire event projection is inconsistent" {:subject subject})))
  {:subject subject :sequence parsed-sequence :id id :at at :kind kind :essential essential :json raw :digest digest :thread thread :agent agent :parent-thread (get parent-threads subject) :coordinator (get coordinators subject)}))) (sort-by (fn [event] (:sequence event))) vec)))

(defn validate-durable-ledger! [port subject scalar]
  (let [run-id (get scalar "wire_run_id")
   events (durable-wire-events! port run-id)
   expected-count (parse-long (get scalar "wire_event_count"))
   sequences (mapv (fn [event] (:sequence event)) events)
   terminal (last events)]
  (if (not (= expected-count (count events))) (do
  (fail! "wire run summary count differs from durable events" {})))
  (if (not (= sequences (vec (range expected-count)))) (do
  (fail! "durable wire event sequence is incomplete" {:sequences sequences})))
  (if (not (and (= "run.terminated" (:kind terminal)) (= (get scalar "wire_terminal_event_id") (:id terminal)) (= (get scalar "at") (:at terminal)))) (do
  (fail! "wire run summary terminal differs from durable terminal event" {})))
  (if (not (every? (fn [__north_anon_1] (and (= (get scalar "thread") (:thread __north_anon_1)) (= (get scalar "agent") (:agent __north_anon_1)) (= (get scalar "parent_thread") (:parent-thread __north_anon_1)) (= (get scalar "run_coordinator") (:coordinator __north_anon_1)))) events)) (do
  (fail! "wire run summary graph identity differs from durable events" {})))
  (if (not (= (get scalar "wire_ledger_sha256") (north.run-ledger/ledger-digest (mapv (fn [__north_anon_1] (hash-map "digest" (:digest __north_anon_1))) events)))) (do
  (fail! "wire run summary digest differs from durable events" {})))
  events))

(defn validate-core-projection! [subject facts events]
  (let [wire-jsonl (str (str/join "\n" (map (fn [event] (:json event)) events)) "\n")
   request (json/generate-string {"subject" subject "facts" facts "wireJsonl" wire-jsonl})
   bun (or (System/getenv "NORTH_BUN") "bun")
   {:keys [exit]} (shell/sh bun wire-telemetry-validator :in request)]
  (if (not (zero? exit)) (do
  (fail! "wire run core projection differs from the canonical reducer" {})))))

(defn terminal-fact-map [facts]
  (reduce (fn [result [predicate value]] (update result predicate (fnil conj #{}) value)) {} facts))

(def summary-chunk-size 200)

(def summary-lease-ttl-ms 120000)

(defn acquire-summary-lease! [port subject]
  (let [resource (str "wire-run-summary:" (north.run-ledger/sha256 subject))
   holder (str "wire-run-summary-writer:" (java.util.UUID/randomUUID))
   outcome (north.coord/retry-conflicts-until! (north.coord/retry-deadline-ns) (fn [] (let [result (north.coord/acquire-lease! port resource holder summary-lease-ttl-ms)]
  (cond
  (:epoch result) {:done (select-keys result [:resource :holder :epoch])}
  (= :held (:reject result)) {:reject :conflict}
  :else result))))]
  (or (:done outcome) (fail! "wire run summary lease is unavailable" {:subject subject :result outcome}))))

(defn release-summary-lease! [port lease]
  (try
  (north.coord/release-lease! port lease)
  (catch Exception _
    nil)))

(defn summary-subset? [actual expected]
  (every? (fn [[predicate values]] (set/subset? values (get expected predicate #{}))) actual))

(defn publish-summary-chunks! [port subject lease body-facts]
  (if (not (:valid? (north.coord/check-lease! port lease))) (do
  (fail! "wire run summary lease was lost before publication" {:subject subject})))
  (let [deadline (north.coord/retry-deadline-ns)]
  (loop []
  (if (not (< (System/nanoTime) deadline)) (do
  (fail! "wire run summary chunk publication exceeded its retry deadline" {:subject subject})))
  (let [before (facts-of port subject)
   missing (filterv (fn [[predicate value]] (not (contains? (get before predicate #{}) value))) body-facts)]
  (if (seq missing) (do
  (let [chunk (vec (take summary-chunk-size missing))
   base (north.coord/cur-ver-for-subject port subject)
   result (north.coord/publish! port (mapv (fn [[predicate value]] {:op :assert :subject subject :predicate predicate :value value :cardinality :many}) chunk) {:expected-version base})]
  (cond
  (:ok result) (recur)
  (= :conflict (:reject result)) (do
  (Thread/sleep 1)
  (recur))
  :else (checked! result [:wire-run-summary-chunk subject]))))))))
  (let [before (facts-of port subject)
   missing (seq (filter (fn [[predicate value]] (not (contains? (get before predicate #{}) value))) body-facts))]
  (if missing (do
  (fail! "wire run summary body readback is incomplete" {:subject subject}))))
  (let [deadline (north.coord/retry-deadline-ns)]
  (loop []
  (if (not (< (System/nanoTime) deadline)) (do
  (fail! "wire run summary commit exceeded its retry deadline" {:subject subject})))
  (let [base (north.coord/cur-ver-for-subject port subject)
   result (north.coord/publish! port [{:op :assert :subject subject :predicate "kind" :value "run" :cardinality :many}] {:expected-version base})]
  (cond
  (:ok result) result
  (= :conflict (:reject result)) (do
  (Thread/sleep 1)
  (recur))
  :else (checked! result [:wire-run-summary-commit subject]))))))

(defn publish-summary! [port subject facts scalar]
  (let [grouped (group-by first facts)
   summary-predicates (set/difference terminal-predicates learning-keys)
   summary-facts (filterv (fn [__north_anon_1] (not (learning-keys (first __north_anon_1)))) facts)
   body-facts (filterv (fn [__north_anon_1] (not= "kind" (first __north_anon_1))) summary-facts)
   expected-summary (terminal-fact-map summary-facts)
   expected-readback (terminal-fact-map facts)
   terminal-learning-keys (set (filter learning-keys (keys grouped)))
   delivery-predicates (set north.terminal-projection/terminal-projection-predicates)
   delivery-facts (select-keys scalar delivery-predicates)
   validate-context! (fn [before validate-core?] (let [existing-summary (select-keys before summary-predicates)
   learning-before (select-keys before learning-keys)
   unexpected (seq (remove reservation-keys (remove summary-predicates (keys before))))
   reserved? (north.terminal-projection/run-reservation-valid? before)]
  (if unexpected (do
  (fail! "run subject has non-reservation facts before wire summary" {:subject subject :predicates unexpected})))
  (if (not (summary-subset? existing-summary expected-summary)) (do
  (fail! "run subject has a conflicting wire summary" {:subject subject})))
  (if (and (contains? existing-summary "kind") (not= existing-summary expected-summary)) (do
  (fail! "committed wire run summary is incomplete" {:subject subject})))
  (if (and (seq terminal-learning-keys) (empty? learning-before)) (do
  (fail! "terminal run cannot introduce a learning assignment after execution" {:subject subject})))
  (if (seq learning-before) (do
  (if (not (= learning-keys (set (keys learning-before)))) (do
  (fail! "pre-provider learning assignment is incomplete" {:subject subject})))
  (if (not (= learning-keys terminal-learning-keys)) (do
  (fail! "terminal run must repeat the complete pre-provider learning assignment" {:subject subject})))
  (doseq [predicate learning-keys
   :let [expected (set (map second (get grouped predicate [])))
   actual (get before predicate #{})]]
  (if (not (= expected actual)) (do
  (fail! "terminal run learning assignment differs from pre-provider assignment" {:subject subject :predicate predicate}))))))
  (if (and (= "reported" (get delivery-facts "delivery_outcome")) (not reserved?)) (do
  (fail! "reported delivery requires a committed run reservation" {:subject subject})))
  (if reserved? (do
  (let [expected-agent (str "@agent:" (get scalar "agent"))
   expected-thread (get scalar "thread")]
  (if (not (= #{expected-agent} (get before "run_reservation_agent"))) (do
  (fail! "wire run telemetry agent differs from its reservation" {})))
  (if (not (= #{expected-thread} (get before "run_reservation_thread"))) (do
  (fail! "wire run telemetry thread differs from its reservation" {}))))))
  (if (contains? delivery-facts "delivery_outcome") (do
  (if (not (north.terminal-projection/delivery-projection-valid? delivery-facts)) (do
  (fail! "run delivery outcome lacks a valid proof projection" {:subject subject})))
  (validate-reported-run! port subject scalar delivery-facts before)))
  (if validate-core? (do
  (let [events (validate-durable-ledger! port subject scalar)]
  (validate-core-projection! subject facts events))))
  existing-summary))]
  (validate-operation-evidence! (get grouped "mcp_operation_receipt" []) (get grouped "mcp_operation_aggregate" []))
  (validate-mcp-tool-evidence! (get grouped "mcp_actual_tool" []) scalar)
  (validate-native-operation-evidence! (get grouped "native_command_completion" []))
  (validate-allocation-evidence! (get grouped "allocation_evidence" []))
  (validate-fallback-evidence! (get grouped "fallback_reason" []))
  (validate-routing-pin-evidence! (get grouped "routing_pin" []))
  (validate-activity-summaries! scalar grouped)
  (validate-estimate! scalar grouped)
  (validate-judgment-grade! scalar grouped)
  (validate-struggle! scalar grouped)
  (validate-shadow-reviewer! scalar grouped)
  (validate-context! (facts-of port subject) true)
  (let [lease (acquire-summary-lease! port subject)]
  (try
  (let [before (facts-of port subject)
   existing-summary (validate-context! before false)]
  (if (not (= existing-summary expected-summary)) (do
  (publish-summary-chunks! port subject lease body-facts))))
  (finally
    (release-summary-lease! port lease))))
  (let [stored (facts-of port subject)]
  (doseq [[predicate values] expected-readback]
  (if (not (set/subset? values (get stored predicate #{}))) (do
  (fail! "wire run telemetry readback is incomplete" {:subject subject :predicate predicate})))))))

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (let [[port-s subject-s] args
   _ (if (not (= 2 (count args))) (do
  (fail! "usage: run-fact-internal.clj PORT SUBJECT < FACTS_JSON" {:argc (count args)})))
   port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
   subject (entity! subject-s)
   facts (payload! (bounded-stdin!))
   scalar (validate-summary-facts! subject facts)]
  (publish-summary! port subject facts scalar)
  (println (json/generate-string {:ok true :subject subject :facts (count facts)})))))

(if (= *file* (System/getProperty "babashka.file")) (do
  (apply -main *command-line-args*)))
