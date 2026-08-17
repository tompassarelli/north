#!/usr/bin/env bb
;; Commit one event-derived run summary after the exact terminal WireEvent
;; sequence is durable. The run's kind marker is the last-write commit signal.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.set :as set]
         '[clojure.string :as str])

(def cli-dir (.getParent (io/file (System/getProperty "babashka.file"))))
(load-file (str cli-dir "/coord.clj"))
(load-file (str cli-dir "/terminal-projection.clj"))
(load-file (str cli-dir "/run-ledger.clj"))
(def wire-telemetry-validator
  (.getCanonicalPath (io/file cli-dir "../sdk/src/wire-telemetry-validator.ts")))

(defn fail! [message data] (throw (ex-info message data)))
(defn checked! [result operation]
  (when (:reject result)
    (fail! "coordinator rejected wire run telemetry publication" {:operation operation}))
  result)

(def terminal-predicates
  #{"kind" "wire_run_id" "thread" "thread_provenance" "agent"
    "parent_run" "parent_thread" "run_coordinator"
    "wire_ledger_version" "wire_version" "wire_ledger_status"
    "wire_event_count" "wire_event_first_sequence" "wire_event_last_sequence"
    "wire_terminal_event_id" "wire_ledger_sha256" "wire_run_lifecycle"
    "wire_termination_code" "outcome" "at" "started_at" "duration_ms"
    "estimate_hours" "estimate_delta_ms" "estimate_ratio" "estimate_classification"
    "judgment_grade" "judgment_grade_status" "judgment_grade_source"
    "lifetime_input_tokens" "lifetime_output_tokens" "lifetime_cache_read_tokens"
    "lifetime_cache_write_tokens" "lifetime_reasoning_tokens" "model_call_count"
    "tokens" "usage_terminal_count" "usage_scope" "usage_total_status"
    "context_tokens" "context_window_tokens" "compaction_count"
    "tool_admitted_count" "tool_succeeded_count" "tool_failed_count"
    "tool_cancelled_count" "tool_synthetic_failure_count" "run_owner"
    "model_tier" "capability_class" "effort"
    "posture" "role" "provider" "provider_target" "provider_reason"
    "model_availability_target" "model_availability_source"
    "model_availability_observed_at" "model_availability_digest"
    "requested_provider" "requested_target" "requested_tier" "requested_effort"
    "allocation_mode" "entitlement_pressure" "allocation_evidence"
    "fallback_count" "fallback_path" "fallback_target_path" "fallback_reason"
    "envelope_scope" "envelope_retries" "envelope_advisory"
    "process_outcome" "delivery_outcome" "delivery_reason"
    "error_count" "struggle" "struggle_detector_policy_version" "struggle_topology"
    "struggle_error_streak_threshold" "struggle_loop_repeat_threshold"
    "struggle_loop_window" "struggle_no_progress_turn_threshold"
    "delivery_evidence" "delivery_evidence_sha256"
    "delivery_attestation" "delivery_attestation_sha256"
    "retry_of_run" "retry_attempt" "execution_source" "execution_transport"
    "provider_session_persistence" "provider_join_key_version"
    "provider_join_coverage" "provider_session_key" "provider_turn_key"
    "provider_duration_ms" "turn_provenance" "num_turns"
    "provider_turn_units" "provider_tool_items" "provider_turn_metric_comparable"
    "watchdog_reason" "watchdog_silence_ms"
    "watchdog_last_outer_activity" "watchdog_last_provider_activity"
    "effective_authority_provider" "effective_native_multi_agent"
    "effective_live_input" "effective_authoring_hooks"
    "effective_authority_capability" "effective_north_enabled_tool"
    "effective_sandbox" "effective_web" "effective_builtin" "effective_mcp_tool"
    "mcp_activity_source" "mcp_activity_coverage" "mcp_actual_calls"
    "mcp_actual_tool" "mcp_operation_receipt" "mcp_operation_aggregate"
    "native_command_activity_source" "native_command_activity_coverage"
    "native_north_binary_probe" "native_command_total" "native_command_successful"
    "native_command_failed" "native_command_declined" "native_command_truncated"
    "native_command_open" "native_command_read" "native_command_edit"
    "native_command_completion"
    "prompt_receipt_version" "prompt_receipt_sha256" "prompt_wire_sha256"
    "prompt_receipt_coverage" "environment_receipt_version"
    "environment_receipt_sha256" "environment_receipt_coverage"
    "available_skill_catalog_sha256" "activated_resource_closure_sha256"
    "run_envelope_version" "run_envelope_sha256"
    "requested_role" "routing_tier" "requested_reasoning" "routing_posture"
    "task_grade" "topology" "domain_requirement" "composition_kind"
    "composition_id" "composition_override" "nearest_template" "promotion_candidate"
    "routing_admission_receipt_version" "routing_request_sha256"
    "staffing_catalog_sha256" "provider_catalogs_sha256" "routing_policy_sha256"
    "orchestration_policy_pin_sha256" "orchestration_catalog_digest_sha256"
    "orchestration_catalog_version" "orchestration_catalog_tx_version"
    "routing_assessment_status" "routing_assessment_sha256"
    "routing_pin_evidence_status" "routing_pin_evidence_sha256"
    "routing_override_evidence_status" "routing_override_exception_code"
    "routing_receipt_override"
    "routing_applied_task_grade" "routing_applied_topology" "routing_applied_tier"
    "routing_applied_reasoning" "routing_applied_posture"
    "routing_stock_task_grade" "routing_stock_topology" "routing_stock_tier"
    "routing_stock_reasoning" "routing_stock_posture"
    "routing_assessment_policy" "routing_signal_decision_ownership"
    "routing_signal_seam_scope" "routing_signal_error_exposure"
    "routing_signal_oracle_strength" "routing_signal_foundational_impact"
    "routing_signal_dependency_shape" "routing_signal_reasoning_shape"
    "routing_derived_tier" "routing_derived_reasoning" "routing_rule_code"
    "routing_selected_tier" "routing_selected_reasoning"
    "routing_exception_code" "routing_pin_policy"
    "routing_pin_issued_at" "routing_pin_expires_at" "routing_pin_reason_code"
    "routing_pin"
    "prompt_composition_applied" "applied_role_contract"
    "applied_bespoke_contract_sha256"
    "applied_bespoke_contract_fingerprint_version"
    "applied_bespoke_contract_fingerprint_domain"
    "applied_template_override" "applied_template_override_reason_sha256"
    "applied_capability" "applied_comms_contract_sha256" "applied_task_grade"
    "applied_topology" "applied_routing_tier" "applied_reasoning"
    "applied_posture" "applied_domain_requirement"
    "applied_domain_requirement_count" "model_delta_provider" "model_delta_kind"
    "prompt_composition_version" "prompt_composition_sha256"
    "prompt_capability_class"
    "prompt_byte_measurement_source" "prompt_token_measurement_status"
    "prompt_token_measurement_source" "context_window_status"
    "context_window_source" "context_budget_status" "context_budget_source"
    "compaction_policy" "compaction_policy_version" "context_window_effective_from"
    "prompt_stable_prefix_bytes" "prompt_unique_tail_bytes" "prompt_total_bytes"
    "prompt_capability_count" "prompt_stable_prefix_tokens"
    "prompt_unique_tail_tokens" "prompt_total_composition_tokens"
    "provider_context_window_tokens" "effective_context_budget_tokens"
    "learning_assignment_version" "learning_policy_version"
    "learning_policy_sha256" "learning_mode" "learning_evidence_mode"
    "learning_experiment_id" "learning_episode_id"
    "learning_task_signature_sha256" "learning_task_signature_coverage"
    "learning_risk" "learning_arm" "learning_axis" "learning_arm_id"
    "learning_propensity" "learning_explore_propensity"
    "learning_narrowing_reason" "learning_baseline_sha256"
    "learning_options_sha256" "learning_assignment_sha256"
    "shadow_reviewer_version" "shadow_reviewer_target"
    "shadow_reviewer_status" "shadow_reviewer_eligible_updates"
    "shadow_reviewer_reviewed_updates" "shadow_reviewer_dropped_updates"
    "shadow_reviewer_emitted_notes" "shadow_reviewer_quarantined_outputs"
    "shadow_reviewer_failed_reviews" "shadow_reviewer_usage_status"
    "shadow_reviewer_tokens" "shadow_reviewer_duration_ms"
    "shadow_reviewer_source_run" "shadow_reviewer_source_from_sequence"
    "shadow_reviewer_source_through_sequence"
    "shadow_reviewer_privacy_omitted_events"
    "shadow_reviewer_capacity_omitted_events" "shadow_reviewer_input_sha256"})

(def multi-predicates
  #{"allocation_evidence" "fallback_reason" "envelope_scope" "envelope_advisory"
    "effective_authority_capability" "effective_north_enabled_tool"
    "effective_builtin" "effective_mcp_tool" "mcp_actual_tool"
    "mcp_operation_receipt" "mcp_operation_aggregate" "native_command_completion"
    "domain_requirement"
    "composition_override" "routing_receipt_override" "routing_rule_code"
    "routing_pin" "applied_template_override" "applied_capability"
    "applied_domain_requirement" "struggle" "provider_turn_key"})

(def required-predicates
  #{"kind" "wire_run_id" "thread" "agent" "wire_ledger_version" "wire_version"
    "wire_ledger_status" "wire_event_count" "wire_event_first_sequence"
    "wire_event_last_sequence" "wire_terminal_event_id" "wire_ledger_sha256"
    "wire_run_lifecycle" "wire_termination_code" "outcome" "at" "started_at"
    "duration_ms" "thread_provenance" "provider_session_persistence"
    "turn_provenance" "lifetime_input_tokens" "lifetime_output_tokens"
    "lifetime_cache_read_tokens" "lifetime_cache_write_tokens"
    "lifetime_reasoning_tokens" "model_call_count" "usage_terminal_count"
    "usage_scope" "usage_total_status" "context_tokens"
    "compaction_count" "tool_admitted_count" "tool_succeeded_count"
    "tool_failed_count" "tool_cancelled_count" "tool_synthetic_failure_count"})

(def count-predicates
  #{"wire_event_count" "wire_event_first_sequence" "wire_event_last_sequence"
    "duration_ms" "lifetime_input_tokens" "lifetime_output_tokens"
    "lifetime_cache_read_tokens" "lifetime_cache_write_tokens"
    "lifetime_reasoning_tokens" "model_call_count" "context_tokens"
    "context_window_tokens" "compaction_count" "tool_admitted_count"
    "tool_succeeded_count" "tool_failed_count" "tool_cancelled_count"
    "tool_synthetic_failure_count" "mcp_actual_calls" "fallback_count"
    "native_command_total" "native_command_successful" "native_command_failed"
    "native_command_declined" "native_command_open" "native_command_truncated" "native_command_read"
    "native_command_edit"
    "envelope_retries" "retry_attempt" "orchestration_catalog_version"
    "orchestration_catalog_tx_version" "applied_domain_requirement_count"
    "prompt_stable_prefix_bytes" "prompt_unique_tail_bytes" "prompt_total_bytes"
    "prompt_capability_count" "prompt_stable_prefix_tokens"
    "prompt_unique_tail_tokens" "prompt_total_composition_tokens"
    "provider_context_window_tokens" "effective_context_budget_tokens"
    "error_count" "struggle_error_streak_threshold"
    "struggle_loop_repeat_threshold" "struggle_loop_window"
    "struggle_no_progress_turn_threshold" "usage_terminal_count"
    "provider_duration_ms" "num_turns"
    "provider_turn_units" "provider_tool_items" "watchdog_silence_ms"
    "shadow_reviewer_eligible_updates" "shadow_reviewer_reviewed_updates"
    "shadow_reviewer_dropped_updates" "shadow_reviewer_emitted_notes"
    "shadow_reviewer_quarantined_outputs" "shadow_reviewer_failed_reviews"
    "shadow_reviewer_tokens" "shadow_reviewer_duration_ms"
    "shadow_reviewer_source_from_sequence" "shadow_reviewer_source_through_sequence"
    "shadow_reviewer_privacy_omitted_events"
    "shadow_reviewer_capacity_omitted_events"})

(def learning-keys
  #{"learning_assignment_version" "learning_policy_version"
    "learning_policy_sha256" "learning_mode" "learning_evidence_mode"
    "learning_experiment_id" "learning_episode_id"
    "learning_task_signature_sha256" "learning_task_signature_coverage"
    "learning_risk" "learning_arm" "learning_axis" "learning_arm_id"
    "learning_propensity" "learning_explore_propensity"
    "learning_narrowing_reason" "learning_baseline_sha256"
    "learning_options_sha256" "learning_assignment_sha256"})

(def reservation-keys
  (conj (into (set north.terminal-projection/run-reservation-predicates)
              learning-keys)
        "run_bar_evidence"))

(def identifier-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$")
(def digest-pattern #"^[a-f0-9]{64}$")

(defn entity [subject]
  (let [raw (str subject)
        canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
    (when-not (north.terminal-projection/valid-run-entity? canonical)
      (fail! "invalid wire run telemetry subject" {:subject subject}))
    canonical))

(defn payload [raw]
  (let [parsed (try (json/parse-string (str raw))
                    (catch Exception error
                      (fail! "invalid wire run telemetry JSON" {:cause (.getMessage error)})))]
    (when-not (sequential? parsed)
      (fail! "wire run telemetry payload must be an array" {}))
    (mapv (fn [entry]
            (when-not (and (sequential? entry) (= 2 (count entry))
                           (every? string? entry)
                           (every? #(not (str/blank? %)) entry))
              (fail! "wire run telemetry facts must be nonblank string pairs" {:entry entry}))
            (vec entry))
          parsed)))

(defn bounded-stdin []
  (let [buffer (byte-array 8192)
        output (java.io.ByteArrayOutputStream.)]
    (loop [total 0]
      (let [read (.read System/in buffer)]
        (if (neg? read)
          (.toString output "UTF-8")
          (let [next-total (+ total read)]
            (when (> next-total north.run-ledger/max-telemetry-projection-bytes)
              (fail! "wire run telemetry exceeds its encoded byte bound"
                     {:limit north.run-ledger/max-telemetry-projection-bytes}))
            (.write output buffer 0 read)
            (recur next-total)))))))

(defn facts-of [port subject]
  (let [rows (north.coord/query-rows
              port {:find "wire_run_writer_fact"
                    :rules [{:head {:rel "wire_run_writer_fact"
                                    :args [{:var "p"} {:var "r"}]}
                             :body [{:rel "triple"
                                     :args [subject {:var "p"} {:var "r"}]}]}]})]
    (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))

(def operation-tool-pattern #"[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}")
(def operation-component-pattern #"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}")

(declare instant!)

(defn parse-operation-json! [label raw]
  (try (json/parse-string raw)
       (catch Exception error
         (fail! (str "invalid " label " JSON") {:cause (.getMessage error)}))))

(defn exact-json-fields! [label record fields]
  (let [actual (if (map? record) (set (keys record)) #{})]
    (when-not (= fields actual)
      (fail! (str label " requires its exact field set") {:fields actual}))))

(defn validate-mcp-tool-evidence! [entries scalar]
  (when (> (count entries) 512)
    (fail! "MCP tool evidence exceeds the bounded tool limit" {}))
  (let [tools
        (mapv (fn [[_ raw]]
                (let [record (parse-operation-json! "MCP tool activity" raw)]
                  (exact-json-fields! "MCP tool activity" record #{"server" "tool" "count"})
                  (when-not (and (every? #(and (string? %)
                                               (re-matches operation-component-pattern %))
                                          ((juxt #(get % "server") #(get % "tool")) record))
                                 (integer? (get record "count"))
                                 (pos? (get record "count")))
                    (fail! "MCP tool activity contains invalid values" {:record record}))
                  record)) entries)
        actual-calls (some-> (get scalar "mcp_actual_calls") parse-long)
        known-calls (reduce + 0 (map #(get % "count") tools))
        coverage (get scalar "mcp_activity_coverage")]
    (when-not (= (count tools)
                 (count (set (map (juxt #(get % "server") #(get % "tool")) tools))))
      (fail! "MCP tool activity identities must be unique" {}))
    (when (and actual-calls (> known-calls actual-calls))
      (fail! "MCP identified calls exceed the observed call count" {}))
    (when (and (= "exact" coverage) actual-calls (not= known-calls actual-calls))
      (fail! "exact MCP tool activity does not reconcile with observed calls" {}))))

(def allocation-kinds
  #{"numeric-headroom" "categorical-pressure" "conservative-floor"})
(def allocation-sources
  #{"claude-agent-sdk:usage-control-experimental" "claude-agent-sdk:rate-limit-event"
    "claude-code:statusline" "codex-app-server:account-rate-limits"
    "legacy-observation" "manual-policy" "policy-default"})
(def allocation-failure-reasons
  #{"anthropic_usage_capability_unavailable" "anthropic_usage_probe_failed"
    "anthropic_usage_probe_timed_out" "anthropic_usage_rate_limits_unavailable"
    "anthropic_usage_response_schema_changed" "anthropic_usage_windows_unavailable"
    "codex_usage_command_unavailable" "codex_usage_probe_failed"
    "codex_usage_probe_timed_out" "codex_usage_response_schema_changed"
    "codex_usage_subscription_auth_required" "codex_usage_transport_failed"
    "codex_usage_windows_unavailable"})
(def allocation-fields
  #{"target" "kind" "source" "observedAt" "limitId" "usedPercent" "resetsAt"
    "routingFloorPercent" "routingFloorExpiresAt" "measuredUsedPercent"
    "measurementSource" "measurementObservedAt" "collectionFailure"})

(defn valid-percent? [value]
  (and (number? value) (<= 0 value 100)))

(defn validate-allocation-evidence! [entries]
  (doseq [[_ raw] entries
          :let [record (parse-operation-json! "allocation evidence" raw)
                fields (set (keys record))]]
    (when-not (and (set/subset? #{"target" "kind" "source"} fields)
                   (set/subset? fields allocation-fields))
      (fail! "allocation evidence contains an unsupported field set" {:fields fields}))
    (when-not (and (string? (get record "target"))
                   (re-matches identifier-pattern (get record "target"))
                   (allocation-kinds (get record "kind"))
                   (allocation-sources (get record "source")))
      (fail! "allocation evidence contains invalid route identity" {}))
    (doseq [field ["observedAt" "resetsAt" "routingFloorExpiresAt"
                   "measurementObservedAt"]
            :when (contains? record field)]
      (instant! (str "allocation " field) (get record field)))
    (doseq [field ["usedPercent" "routingFloorPercent" "measuredUsedPercent"]
            :when (contains? record field)]
      (when-not (valid-percent? (get record field))
        (fail! "allocation evidence percentage is invalid" {:field field})))
    (when-let [source (get record "measurementSource")]
      (when-not (allocation-sources source)
        (fail! "allocation measurement source is invalid" {})))
    (when-let [failure (get record "collectionFailure")]
      (exact-json-fields! "allocation collection failure" failure #{"observedAt" "reason"})
      (instant! "allocation collection failure" (get failure "observedAt"))
      (when-not (allocation-failure-reasons (get failure "reason"))
        (fail! "allocation collection failure reason is invalid" {})))))

(def fallback-fields
  #{"sequence" "reason" "fromTarget" "fromProvider" "toTarget" "toProvider"
    "phase" "replay" "proof"})
(def unsent-proof-fields
  #{"version" "durability" "mode" "source" "requestBytesPrepared"
    "requestBytesSent" "observableEvents"})

(defn validate-fallback-evidence! [entries]
  (doseq [[_ raw] entries
          :let [record (parse-operation-json! "fallback reason" raw)
                proof (get record "proof")]]
    (exact-json-fields! "fallback reason" record fallback-fields)
    (exact-json-fields! "fallback unsent proof" proof unsent-proof-fields)
    (when-not (and (integer? (get record "sequence")) (pos? (get record "sequence"))
                   (= "provider_retry_safe_before_acceptance" (get record "reason"))
                   (every? #(and (string? %) (re-matches identifier-pattern %))
                           ((juxt #(get % "fromTarget") #(get % "toTarget")) record))
                   (every? #{"anthropic" "openai"}
                           ((juxt #(get % "fromProvider") #(get % "toProvider")) record))
                   (= "preaccept" (get record "phase"))
                   (= "proved_unsent" (get record "replay"))
                   (= "north:provider-unsent-proof:v1" (get proof "version"))
                   (= "adapter_receipt" (get proof "durability"))
                   (#{"managed" "native"} (get proof "mode"))
                   (#{"adapter_preflight" "managed_pre_thread_receipt"
                      "native_supervisor_unavailable"} (get proof "source"))
                   (integer? (get proof "requestBytesPrepared"))
                   (<= 0 (get proof "requestBytesPrepared"))
                   (zero? (get proof "requestBytesSent"))
                   (zero? (get proof "observableEvents")))
      (fail! "fallback reason contains invalid zero-send evidence" {}))
    (when-not (= (= "managed" (get proof "mode"))
                 (boolean (#{"adapter_preflight" "managed_pre_thread_receipt"}
                           (get proof "source"))))
      (fail! "fallback unsent proof mode and source differ" {}))))

(defn validate-routing-pin-evidence! [entries]
  (doseq [[_ raw] entries
          :let [record (parse-operation-json! "routing pin" raw)]]
    (exact-json-fields! "routing pin" record #{"kind" "value"})
    (when-not (and (= "provider" (get record "kind"))
                   (string? (get record "value"))
                   (re-matches identifier-pattern (get record "value")))
      (fail! "routing pin exposes an unsupported or invalid value" {}))))

(defn validate-operation-evidence! [receipt-entries aggregate-entries]
  (when (or (> (count receipt-entries) 512) (> (count aggregate-entries) 512))
    (fail! "MCP operation evidence exceeds the bounded receipt limit" {}))
  (let [receipts
        (mapv (fn [[_ raw]]
                (let [record (parse-operation-json! "MCP operation receipt" raw)
                      fields (set (keys record))]
                  (when-not (or (= #{"tool" "operation" "durationMs" "outcome" "resultSize"} fields)
                                (= #{"tool" "operation" "durationMs" "batchSize"
                                     "outcome" "resultSize"} fields))
                    (fail! "MCP operation receipt requires the exact v1 field set" {}))
                  (when-not (and (string? (get record "tool"))
                                 (re-matches operation-tool-pattern (get record "tool"))
                                 (every? #(and (string? %) (re-matches operation-component-pattern %))
                                         ((juxt #(get % "operation") #(get % "outcome")) record))
                                 (every? #(and (integer? %) (<= 0 %))
                                         (cond-> [(get record "durationMs") (get record "resultSize")]
                                           (contains? record "batchSize")
                                           (conj (get record "batchSize")))))
                    (fail! "MCP operation receipt contains invalid values" {:record record}))
                  record)) receipt-entries)
        aggregates
        (mapv (fn [[_ raw]]
                (let [record (parse-operation-json! "MCP operation aggregate" raw)
                      count' (get record "count")
                      total (get record "totalDurationMs")
                      mean (get record "meanDurationMs")
                      failures (get record "failureCount")]
                  (when-not (= #{"operation" "count" "totalDurationMs"
                                 "meanDurationMs" "failureCount"}
                               (set (keys record)))
                    (fail! "MCP operation aggregate requires the exact v1 field set" {}))
                  (when-not (and (string? (get record "operation"))
                                 (re-matches operation-component-pattern (get record "operation"))
                                 (integer? count') (pos? count')
                                 (integer? total) (<= 0 total)
                                 (number? mean) (= (double mean) (/ (double total) count'))
                                 (integer? failures) (<= 0 failures count'))
                    (fail! "MCP operation aggregate contains invalid values" {:record record}))
                  record)) aggregate-entries)
        derived
        (reduce (fn [result receipt]
                  (update result (get receipt "operation")
                          (fnil (fn [entry]
                                  (-> entry
                                      (update "count" inc)
                                      (update "totalDurationMs" + (get receipt "durationMs"))
                                      (update "failureCount" +
                                              (if (= "ok" (get receipt "outcome")) 0 1))))
                                {"count" 0 "totalDurationMs" 0 "failureCount" 0})))
                {} receipts)]
    (when-not (= (set (keys derived)) (set (map #(get % "operation") aggregates)))
      (fail! "MCP operation aggregates do not cover the exact receipt operations" {}))
    (when-not (= (count derived) (count aggregates))
      (fail! "MCP operation aggregates must be unique by operation" {}))
    (doseq [aggregate aggregates
            :let [expected (get derived (get aggregate "operation"))]]
      (when-not (= expected (select-keys aggregate ["count" "totalDurationMs" "failureCount"]))
        (fail! "MCP operation aggregate does not reconcile with receipts"
               {:operation (get aggregate "operation")})))))

(defn validate-native-operation-evidence! [entries]
  (when (> (count entries) 32)
    (fail! "native command completion evidence exceeds the bounded receipt limit" {}))
  (doseq [[_ raw] entries
          :let [record (parse-operation-json! "native command completion" raw)]]
    (when-not (= #{"commandSha256" "outputSha256" "status" "exitCode" "shape" "durationMs"}
                 (set (keys record)))
      (fail! "native command completion requires the exact duration-bearing field set" {}))
    (when-not (and (every? #(boolean (re-matches #"[a-f0-9]{64}" (or % "")))
                           ((juxt #(get % "commandSha256") #(get % "outputSha256")) record))
                   (#{"completed" "failed" "declined"} (get record "status"))
                   (#{"read" "edit" "other"} (get record "shape"))
                   (integer? (get record "exitCode"))
                   (<= -2147483648 (get record "exitCode") 2147483647)
                   (integer? (get record "durationMs"))
                   (<= 0 (get record "durationMs")))
      (fail! "native command completion contains invalid operation evidence" {}))))

(defn singleton-map [facts]
  (let [grouped (group-by first facts)]
    (doseq [[predicate entries] grouped]
      (when (and (not (multi-predicates predicate)) (> (count entries) 1))
        (fail! "wire run telemetry predicates must be singleton"
               {:predicate predicate :values (mapv second entries)})))
    (into {} (keep (fn [[predicate entries]]
                     (when-not (multi-predicates predicate)
                       [predicate (second (first entries))]))) grouped)))

(defn nonnegative-count! [predicate value]
  (let [parsed (parse-long value)]
    (when-not (and parsed (<= 0 parsed) (<= parsed 9007199254740991))
      (fail! "invalid wire run telemetry count" {:predicate predicate :value value}))
    parsed))

(defn instant! [label value]
  (try
    (java.time.Instant/parse value)
    (catch Exception _ (fail! (str "invalid wire run telemetry " label) {:value value}))))

(def mcp-activity-predicates
  #{"mcp_activity_source" "mcp_activity_coverage" "mcp_actual_calls"
    "mcp_actual_tool" "mcp_operation_receipt" "mcp_operation_aggregate"})
(def native-activity-predicates
  #{"native_command_activity_source" "native_command_activity_coverage"
    "native_north_binary_probe" "native_command_total" "native_command_successful"
    "native_command_failed" "native_command_declined" "native_command_open"
    "native_command_truncated" "native_command_read" "native_command_edit"
    "native_command_completion"})

(defn validate-activity-summaries! [scalar grouped]
  (let [fields (set (keys grouped))
        mcp-fields (set/intersection fields mcp-activity-predicates)
        mcp-coverage (get scalar "mcp_activity_coverage")]
    (when (seq mcp-fields)
      (when-not (and (re-matches identifier-pattern (or (get scalar "mcp_activity_source") ""))
                     (#{"exact" "partial" "unknown"} mcp-coverage))
        (fail! "MCP activity requires a valid source and coverage" {}))
      (when (and (= "exact" mcp-coverage) (nil? (get scalar "mcp_actual_calls")))
        (fail! "exact MCP activity requires an observed call count" {}))
      (when (and (= "unknown" mcp-coverage)
                 (seq (set/intersection fields
                                        (disj mcp-activity-predicates
                                              "mcp_activity_source"
                                              "mcp_activity_coverage"))))
        (fail! "unknown MCP activity cannot carry terminal evidence" {}))))
  (let [fields (set (keys grouped))
        native-fields (set/intersection fields native-activity-predicates)
        coverage (get scalar "native_command_activity_coverage")
        probe (get scalar "native_north_binary_probe")]
    (when (seq native-fields)
      (when-not (and (re-matches identifier-pattern
                                 (or (get scalar "native_command_activity_source") ""))
                     (#{"exact" "partial" "unknown"} coverage)
                     (#{"passed" "failed" "not_observed"} probe))
        (fail! "native command activity requires a valid source, coverage, and probe" {}))
      (when (and (= "unknown" coverage)
                 (seq (set/intersection fields
                                        (disj native-activity-predicates
                                              "native_command_activity_source"
                                              "native_command_activity_coverage"
                                              "native_north_binary_probe"))))
        (fail! "unknown native command activity cannot carry terminal evidence" {}))
      (when (and (= "exact" coverage) (nil? (get scalar "native_command_total")))
        (fail! "exact native command activity requires an observed command count" {}))
      (when-let [total (some-> (get scalar "native_command_total") parse-long)]
        (let [settled (+ (or (some-> (get scalar "native_command_successful") parse-long) 0)
                         (or (some-> (get scalar "native_command_failed") parse-long) 0)
                         (or (some-> (get scalar "native_command_declined") parse-long) 0)
                         (or (some-> (get scalar "native_command_open") parse-long) 0))]
          (when-not (= total settled)
            (fail! "native command activity counts do not reconcile" {})))))))

(def estimate-hours-pattern
  #"^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$")
(def estimate-predicates
  #{"estimate_hours" "estimate_delta_ms" "estimate_ratio" "estimate_classification"})

(defn canonical-estimate-ratio [actual-ms estimated-ms]
  (let [spec (get-in north.run-ledger/contract ["telemetry" "estimateRatio"])
        scale (get spec "scale")
        scale-text (str scale)]
    (when-not (and (integer? scale) (pos? scale) (re-matches #"10*" scale-text)
                   (= "nearest-half-up" (get spec "rounding"))
                   (= "omit" (get spec "trailingFractionZeros")))
      (fail! "invalid wire run estimate ratio contract" {}))
    (let [scale' (bigint scale)
          denominator (bigint estimated-ms)
          numerator (*' (bigint actual-ms) scale')
          quotient (quot numerator denominator)
          remainder (rem numerator denominator)
          rounded (+ quotient (if (>= (*' 2 remainder) denominator) 1 0))
          whole (quot rounded scale')
          fraction-raw (str (mod rounded scale'))
          fraction-padded (str (apply str (repeat (- (dec (count scale-text))
                                                       (count fraction-raw)) "0"))
                               fraction-raw)
          fraction (str/replace fraction-padded #"0+$" "")]
      (if (str/blank? fraction) (str whole) (str whole "." fraction)))))

(defn validate-estimate! [scalar grouped]
  (let [present (set/intersection estimate-predicates (set (keys grouped)))]
    (when (seq present)
      (when-not (= estimate-predicates present)
        (fail! "run estimate comparison requires its complete fact set" {:fields present}))
      (let [hours (get scalar "estimate_hours")
            parsed (when (re-matches estimate-hours-pattern (or hours ""))
                     (try (Double/parseDouble hours) (catch Exception _ nil)))
            estimated-ms (when (and parsed (Double/isFinite parsed) (pos? parsed))
                           (Math/round (* parsed 3600000.0)))
            actual-ms (parse-long (get scalar "duration_ms"))
            delta-ms (parse-long (get scalar "estimate_delta_ms"))]
        (when-not (and estimated-ms (pos? estimated-ms)
                       (<= estimated-ms 9007199254740991))
          (fail! "run estimate hours is not positive and finite" {:value hours}))
        (let [expected-delta (- actual-ms estimated-ms)
              expected-classification (cond (neg? expected-delta) "under"
                                            (pos? expected-delta) "over"
                                            :else "on")]
          (when-not (and (= expected-delta delta-ms)
                         (= (canonical-estimate-ratio actual-ms estimated-ms)
                            (get scalar "estimate_ratio"))
                         (= expected-classification
                            (get scalar "estimate_classification")))
            (fail! "run estimate comparison differs from terminal duration" {})))))))

(def struggle-predicates
  #{"error_count" "struggle" "struggle_detector_policy_version" "struggle_topology"
    "struggle_error_streak_threshold" "struggle_loop_repeat_threshold"
    "struggle_loop_window" "struggle_no_progress_turn_threshold"})
(def struggle-required-predicates (disj struggle-predicates "struggle"))
(def struggle-trigger-values #{"consecutive_errors" "tool_loop" "no_progress"})

(def judgment-grade-predicates
  #{"judgment_grade" "judgment_grade_status" "judgment_grade_source"})

(defn validate-judgment-grade! [scalar grouped]
  (let [present (set/intersection judgment-grade-predicates (set (keys grouped)))]
    (when (seq present)
      (when-not (set/subset? #{"judgment_grade_status" "judgment_grade_source"} present)
        (fail! "judgment grade observation requires status and source" {:fields present}))
      (let [grade (get scalar "judgment_grade")
            status (get scalar "judgment_grade_status")
            source (get scalar "judgment_grade_source")
            valid (and (= "valid" status) (= "thread" source)
                       (#{"s" "m" "l"} grade))
            unavailable (and (= "unavailable" status) (nil? grade)
                             (#{"thread" "ad-hoc"} source))
            invalid (and (= "invalid" status) (nil? grade) (= "thread" source))]
        (when-not (or valid unavailable invalid)
          (fail! "invalid run-local judgment grade observation" {}))))))

(defn validate-struggle! [scalar grouped]
  (let [present (set/intersection struggle-predicates (set (keys grouped)))]
    (when (seq present)
      (when-not (set/subset? struggle-required-predicates present)
        (fail! "struggle observation requires its complete policy fact set" {:fields present}))
      (when-not (= "north:struggle-observer:v2"
                   (get scalar "struggle_detector_policy_version"))
        (fail! "unsupported struggle detector policy version" {}))
      (when-not (#{"worker" "orchestrator"} (get scalar "struggle_topology"))
        (fail! "invalid struggle topology" {}))
      (let [error-count (parse-long (get scalar "error_count"))
            error-streak (parse-long (get scalar "struggle_error_streak_threshold"))
            loop-repeat (parse-long (get scalar "struggle_loop_repeat_threshold"))
            loop-window (parse-long (get scalar "struggle_loop_window"))
            no-progress (parse-long (get scalar "struggle_no_progress_turn_threshold"))
            thresholds [error-streak loop-repeat loop-window no-progress]
            triggers (mapv second (get grouped "struggle" []))]
        (when-not (and error-count (<= 0 error-count)
                       (every? #(and % (<= 1 % 1000)) thresholds)
                       (<= loop-repeat loop-window))
          (fail! "invalid struggle observation counts or thresholds" {}))
        (when-not (and (= (count triggers) (count (set triggers)))
                       (every? struggle-trigger-values triggers))
          (fail! "invalid struggle trigger observation" {}))))))

(def shadow-reviewer-common-predicates
  #{"shadow_reviewer_version" "shadow_reviewer_target"})
(def shadow-reviewer-summary-required-predicates
  (set/union
   shadow-reviewer-common-predicates
   #{"shadow_reviewer_status" "shadow_reviewer_eligible_updates"
     "shadow_reviewer_reviewed_updates" "shadow_reviewer_dropped_updates"
     "shadow_reviewer_emitted_notes" "shadow_reviewer_quarantined_outputs"
     "shadow_reviewer_failed_reviews" "shadow_reviewer_usage_status"
     "shadow_reviewer_duration_ms"}))
(def shadow-reviewer-execution-predicates
  (set/union
   shadow-reviewer-common-predicates
   #{"shadow_reviewer_source_run" "shadow_reviewer_source_from_sequence"
     "shadow_reviewer_source_through_sequence"
     "shadow_reviewer_privacy_omitted_events"
     "shadow_reviewer_capacity_omitted_events" "shadow_reviewer_input_sha256"}))
(def shadow-reviewer-predicates
  (set/union shadow-reviewer-summary-required-predicates
             shadow-reviewer-execution-predicates
             #{"shadow_reviewer_tokens"}))
(def shadow-reviewer-target-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$")
(def shadow-reviewer-wire-id-pattern #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$")
(def shadow-reviewer-statuses #{"not_assigned" "completed" "partial" "aborted"})
(def shadow-reviewer-usage-statuses
  #{"exact" "partial" "unknown_incomplete_terminal" "unknown_no_terminal"
    "unknown_provider" "unknown_overflow"})

(defn validate-shadow-reviewer-summary! [scalar present]
  (let [status (get scalar "shadow_reviewer_status")
        usage-status (get scalar "shadow_reviewer_usage_status")
        exact-usage? (= "exact" usage-status)
        expected (cond-> shadow-reviewer-summary-required-predicates
                   exact-usage? (conj "shadow_reviewer_tokens"))]
    (when-not (= expected present)
      (fail! "shadow reviewer summary requires its exact fact set" {:fields present}))
    (when-not (and (shadow-reviewer-statuses status)
                   (shadow-reviewer-usage-statuses usage-status))
      (fail! "invalid shadow reviewer summary status" {}))
    (let [eligible (nonnegative-count! "shadow_reviewer_eligible_updates"
                                       (get scalar "shadow_reviewer_eligible_updates"))
          reviewed (nonnegative-count! "shadow_reviewer_reviewed_updates"
                                       (get scalar "shadow_reviewer_reviewed_updates"))
          dropped (nonnegative-count! "shadow_reviewer_dropped_updates"
                                      (get scalar "shadow_reviewer_dropped_updates"))
          emitted (nonnegative-count! "shadow_reviewer_emitted_notes"
                                      (get scalar "shadow_reviewer_emitted_notes"))
          quarantined (nonnegative-count! "shadow_reviewer_quarantined_outputs"
                                          (get scalar "shadow_reviewer_quarantined_outputs"))
          failed (nonnegative-count! "shadow_reviewer_failed_reviews"
                                     (get scalar "shadow_reviewer_failed_reviews"))
          duration (nonnegative-count! "shadow_reviewer_duration_ms"
                                       (get scalar "shadow_reviewer_duration_ms"))
          tokens (when exact-usage?
                   (nonnegative-count! "shadow_reviewer_tokens"
                                       (get scalar "shadow_reviewer_tokens")))
          handled (+ reviewed dropped)
          surfaced (+ emitted quarantined)
          all-zero? (every? zero? [eligible reviewed dropped emitted quarantined
                                   failed duration (or tokens 0)])]
      (when-not (and (<= handled 9007199254740991) (<= handled eligible)
                     (<= surfaced 9007199254740991) (<= surfaced reviewed)
                     (<= failed eligible))
        (fail! "shadow reviewer summary counts do not reconcile" {}))
      (when (and (= "not_assigned" status) (not (and exact-usage? all-zero?)))
        (fail! "inactive shadow reviewer summary carries activity" {}))
      (when (and (= "completed" status)
                 (not (and exact-usage? (= reviewed eligible) (zero? dropped)
                           (zero? quarantined) (zero? failed))))
        (fail! "completed shadow reviewer summary carries incomplete work" {}))
      (when (and (= "partial" status) exact-usage? (zero? dropped)
                 (zero? quarantined) (zero? failed))
        (fail! "partial shadow reviewer summary lacks partial evidence" {})))))

(defn validate-shadow-reviewer-execution! [scalar present grouped]
  (when-not (= shadow-reviewer-execution-predicates present)
    (fail! "shadow reviewer execution requires its exact fact set" {:fields present}))
  (let [source-run (get scalar "shadow_reviewer_source_run")
        from-sequence (nonnegative-count! "shadow_reviewer_source_from_sequence"
                                          (get scalar "shadow_reviewer_source_from_sequence"))
        through-sequence (nonnegative-count! "shadow_reviewer_source_through_sequence"
                                             (get scalar "shadow_reviewer_source_through_sequence"))
        learning-fields (set/intersection learning-keys (set (keys grouped)))]
    (when-not (and (re-matches shadow-reviewer-wire-id-pattern (or source-run ""))
                   (<= from-sequence through-sequence)
                   (re-matches digest-pattern
                               (or (get scalar "shadow_reviewer_input_sha256") "")))
      (fail! "invalid shadow reviewer source evidence" {}))
    (when-not (= (get scalar "parent_run")
                 (north.run-ledger/run-summary-subject source-run))
      (fail! "shadow reviewer execution is not linked to its source run" {}))
    (when-not (= "shadow-reviewer" (get scalar "role"))
      (fail! "shadow reviewer execution requires its isolated role" {}))
    (when (seq learning-fields)
      (fail! "shadow reviewer execution cannot carry a learning assignment"
             {:fields learning-fields}))
    (let [admitted (nonnegative-count! "tool_admitted_count"
                                       (get scalar "tool_admitted_count"))
          succeeded (nonnegative-count! "tool_succeeded_count"
                                        (get scalar "tool_succeeded_count"))
          failed (nonnegative-count! "tool_failed_count"
                                     (get scalar "tool_failed_count"))
          cancelled (nonnegative-count! "tool_cancelled_count"
                                        (get scalar "tool_cancelled_count"))
          synthetic (nonnegative-count! "tool_synthetic_failure_count"
                                        (get scalar "tool_synthetic_failure_count"))
          zero-tools? (every? zero? [admitted succeeded failed cancelled synthetic])
          truthful-violation? (and (pos? admitted)
                                   (= admitted synthetic)
                                   (every? zero? [succeeded failed cancelled])
                                   (= "failed" (get scalar "wire_run_lifecycle")))]
      (when-not (or zero-tools? truthful-violation?)
        (fail! "shadow reviewer tool evidence is not a truthful data-only failure" {})))))

(defn validate-shadow-reviewer! [scalar grouped]
  (let [present (set/intersection shadow-reviewer-predicates (set (keys grouped)))]
    (when (seq present)
      (when-not (and (= "north-shadow-reviewer:v1"
                        (get scalar "shadow_reviewer_version"))
                     (re-matches shadow-reviewer-target-pattern
                                 (or (get scalar "shadow_reviewer_target") "")))
        (fail! "invalid shadow reviewer identity" {}))
      (cond
        (contains? present "shadow_reviewer_status")
        (validate-shadow-reviewer-summary! scalar present)

        (contains? present "shadow_reviewer_source_run")
        (validate-shadow-reviewer-execution! scalar present grouped)

        :else
        (fail! "shadow reviewer evidence is incomplete" {:fields present})))))

(def provider-join-predicates
  #{"provider_join_key_version" "provider_join_coverage"
    "provider_session_key" "provider_turn_key"})
(def turn-evidence-predicates
  #{"num_turns" "provider_turn_units"
    "provider_tool_items" "provider_turn_metric_comparable"})
(def watchdog-predicates
  #{"watchdog_reason" "watchdog_silence_ms"
    "watchdog_last_outer_activity" "watchdog_last_provider_activity"})
(def execution-transports
  #{"sdk-stream" "managed-app-server" "cli-jsonl"})
(def watchdog-activity-kinds
  {"outer" #{"message" "model" "tool" "artifact" "compaction" "activity"}
   "provider" #{"turn" "item" "tool" "progress" "frame" "activity"}})

(defn nonnegative-bigint! [label value]
  (let [parsed (try (bigint value) (catch Exception _ nil))]
    (when-not (and parsed (<= 0 parsed))
      (fail! (str label " must be a nonnegative integer") {:value value}))
    parsed))

(defn canonical-usage-total [scalar]
  (let [input (nonnegative-bigint! "lifetime input tokens"
                                   (get scalar "lifetime_input_tokens"))
        output (nonnegative-bigint! "lifetime output tokens"
                                    (get scalar "lifetime_output_tokens"))
        cache-read (nonnegative-bigint! "lifetime cache-read tokens"
                                        (get scalar "lifetime_cache_read_tokens"))
        cache-write (nonnegative-bigint! "lifetime cache-write tokens"
                                         (get scalar "lifetime_cache_write_tokens"))]
    (case (get scalar "provider")
      "anthropic" (+ input output cache-read cache-write)
      "openai" (+ input output)
      nil)))

(defn validate-usage-coverage! [scalar]
  (let [terminal-count (parse-long (get scalar "usage_terminal_count"))
        scope (get scalar "usage_scope")
        status (get scalar "usage_total_status")
        tokens (when-let [raw (get scalar "tokens")]
                 (nonnegative-bigint! "exact tokens" raw))]
    (when-not (= "wire_run_cumulative" scope)
      (fail! "usage scope is not the canonical Wire run cumulative scope" {}))
    (case status
      "exact"
      (let [expected (canonical-usage-total scalar)]
        (when-not (and terminal-count (pos? terminal-count)
                       tokens expected (= tokens expected))
          (fail! "exact usage requires an authoritative provider terminal and exact formula" {})))
      "partial"
      (when tokens
        (fail! "partial usage cannot publish an exact aggregate" {}))
      "unknown_incomplete_terminal"
      (when-not (and terminal-count (pos? terminal-count) (nil? tokens))
        (fail! "incomplete provider terminal usage must remain unknown" {}))
      "unknown_no_terminal"
      (when-not (and terminal-count (zero? terminal-count) (nil? tokens))
        (fail! "usage without a provider terminal must remain unknown" {}))
      (fail! "usage total status is invalid" {:status status}))))

(defn validate-watchdog-activity! [label raw origin]
  (when-not (= "none" raw)
    (let [record (parse-operation-json! label raw)]
      (exact-json-fields! label record #{"origin" "kind" "observedAt"})
      (when-not (and (= origin (get record "origin"))
                     ((get watchdog-activity-kinds origin) (get record "kind")))
        (fail! (str label " contains invalid provider-neutral activity") {}))
      (instant! label (get record "observedAt")))))

(defn validate-core-evidence! [scalar grouped]
  (let [fields (set (keys grouped))
        thread (get scalar "thread")
        thread-provenance (get scalar "thread_provenance")]
    (validate-usage-coverage! scalar)
    (when-not (= thread-provenance (if (= "(ad-hoc)" thread) "ad-hoc" "exact"))
      (fail! "thread provenance differs from the exact graph identity" {}))
    (let [source (get scalar "execution_source")
          transport (get scalar "execution_transport")]
      (when-not (or (nil? source) (#{"north-managed" "provider-native"} source))
        (fail! "execution source is invalid" {}))
      (when (and transport (nil? source))
        (fail! "execution transport requires an execution source" {}))
      (when (and transport (not (execution-transports transport)))
        (fail! "execution transport is invalid" {})))
    (let [join-fields (set/intersection fields provider-join-predicates)
          persistence (get scalar "provider_session_persistence")
          coverage (get scalar "provider_join_coverage")
          session-key (get scalar "provider_session_key")
          turn-keys (mapv second (get grouped "provider_turn_key" []))]
      (when-not (#{"persisted" "ephemeral" "unknown"} persistence)
        (fail! "provider session persistence is invalid" {}))
      (if (seq join-fields)
        (do
          (when-not (set/subset? #{"provider_join_key_version" "provider_join_coverage"}
                                 join-fields)
            (fail! "provider join evidence requires version and coverage" {}))
          (when-not (= "north-provider-join:v1" (get scalar "provider_join_key_version"))
            (fail! "provider join evidence has an unsupported version" {}))
          (when-not (#{"exact" "partial" "unknown"} coverage)
            (fail! "provider join coverage is invalid" {}))
          (when (and session-key (not (re-matches digest-pattern session-key)))
            (fail! "provider session key is not a privacy-bounded digest" {}))
          (when-not (and (= (count turn-keys) (count (set turn-keys)))
                         (every? #(re-matches digest-pattern %) turn-keys))
            (fail! "provider turn keys are not unique privacy-bounded digests" {}))
          (when (and (= "exact" coverage) (or (nil? session-key) (empty? turn-keys)))
            (fail! "exact provider join evidence requires session and turn keys" {}))
          (when (and (= "partial" coverage) (nil? session-key) (empty? turn-keys))
            (fail! "partial provider join evidence requires a bounded join key" {})))
        (when-not (= "unknown" persistence)
          (fail! "session persistence without join evidence must be unknown" {}))))
    (let [provenance (get scalar "turn_provenance")
          turn-fields (set/intersection fields turn-evidence-predicates)
          num-turns (get scalar "num_turns")
          provider-units (get scalar "provider_turn_units")
          provider-tools (get scalar "provider_tool_items")
          comparable (get scalar "provider_turn_metric_comparable")]
      (when-not (#{"provider-terminal" "pre-provider" "unknown"} provenance)
        (fail! "turn provenance is invalid" {}))
      (when (and num-turns provider-units)
        (fail! "assistant turns and provider turn units are not comparable" {}))
      (when (and provider-tools (nil? provider-units))
        (fail! "provider tool items require provider turn units" {}))
      (when-not (= (some? provider-units) (= "false" comparable))
        (fail! "provider turn units require their non-comparability disclaimer" {}))
      (case provenance
        "provider-terminal" nil
        "pre-provider"
        (when-not (and (= "0" num-turns)
                       (= #{"num_turns"} turn-fields))
          (fail! "pre-provider turn provenance requires exact zero assistant turns" {}))
        "unknown"
        (when (seq turn-fields)
          (fail! "unknown turn provenance cannot carry terminal turn evidence" {}))))
    (let [present (set/intersection fields watchdog-predicates)]
      (when (seq present)
        (when-not (= watchdog-predicates present)
          (fail! "watchdog evidence requires its complete replay-derived fact set" {}))
        (when-not (= "north_watchdog_execution_inactivity" (get scalar "watchdog_reason"))
          (fail! "watchdog reason is not canonical" {}))
        (let [silence-ms (parse-long (get scalar "watchdog_silence_ms"))]
          (when-not (and silence-ms (pos? silence-ms))
            (fail! "watchdog silence must be positive" {})))
        (validate-watchdog-activity! "watchdog outer activity"
                                     (get scalar "watchdog_last_outer_activity") "outer")
        (validate-watchdog-activity! "watchdog provider activity"
                                     (get scalar "watchdog_last_provider_activity") "provider")))))

(defn validate-summary-facts! [subject facts]
  (let [scalar (singleton-map facts)
        fields (set (map first facts))
        unknown (seq (remove terminal-predicates fields))
        missing (seq (remove #(contains? scalar %) required-predicates))
        count' (nonnegative-count! "wire_event_count" (get scalar "wire_event_count"))
        first-sequence (nonnegative-count! "wire_event_first_sequence"
                                           (get scalar "wire_event_first_sequence"))
        last-sequence (nonnegative-count! "wire_event_last_sequence"
                                          (get scalar "wire_event_last_sequence"))]
    (when unknown (fail! "wire run telemetry contains unknown predicates" {:predicates unknown}))
    (when missing (fail! "wire run telemetry is missing required predicates" {:predicates missing}))
    (when-not (= "run" (get scalar "kind"))
      (fail! "wire run telemetry requires kind=run" {}))
    (when-not (= north.run-ledger/version (get scalar "wire_ledger_version"))
      (fail! "unsupported wire ledger version" {}))
    (when-not (= north.run-ledger/wire-version (get scalar "wire_version"))
      (fail! "unsupported wire version" {}))
    (when-not (= "complete" (get scalar "wire_ledger_status"))
      (fail! "wire run summary cannot claim an unavailable ledger" {}))
    (when-not (and (pos? count') (zero? first-sequence) (= last-sequence (dec count')))
      (fail! "wire run telemetry sequence summary is inconsistent" {}))
    (doseq [predicate count-predicates :when (contains? scalar predicate)]
      (nonnegative-count! predicate (get scalar predicate)))
    (when-not (and (re-matches identifier-pattern (get scalar "agent"))
                   (or (= "(ad-hoc)" (get scalar "thread"))
                       (north.terminal-projection/valid-thread-entity? (get scalar "thread"))))
      (fail! "wire run telemetry graph identity is invalid" {}))
    (when-let [parent-run (get scalar "parent_run")]
      (when-not (north.terminal-projection/valid-run-entity? parent-run)
        (fail! "wire run telemetry parent run is invalid" {})))
    (when-let [parent-thread (get scalar "parent_thread")]
      (when-not (north.terminal-projection/valid-thread-entity? parent-thread)
        (fail! "wire run telemetry parent thread is invalid" {})))
    (when-let [coordinator (get scalar "run_coordinator")]
      (when-not (re-matches identifier-pattern coordinator)
        (fail! "wire run telemetry coordinator is invalid" {})))
    (when-not (#{"completed" "failed" "cancelled" "blocked"}
                (get scalar "wire_run_lifecycle"))
      (fail! "wire run telemetry lifecycle is not terminal" {}))
    (when-not (and (re-matches identifier-pattern (get scalar "wire_termination_code"))
                   (re-matches identifier-pattern (get scalar "outcome"))
                   (re-matches #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$"
                               (get scalar "wire_terminal_event_id"))
                   (re-matches digest-pattern (get scalar "wire_ledger_sha256")))
      (fail! "wire run telemetry terminal identity is invalid" {}))
    (instant! "at" (get scalar "at"))
    (instant! "started_at" (get scalar "started_at"))
    (when-not (= subject (north.run-ledger/run-summary-subject (get scalar "wire_run_id")))
      (fail! "wire run telemetry subject does not match its exact wire run id" {}))
    (validate-core-evidence! scalar (group-by first facts))
    scalar))

(defn thread-entity [raw]
  (when (and (string? raw) (not= raw "(ad-hoc)"))
    (let [canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
      (when-not (north.terminal-projection/valid-thread-entity? canonical)
        (fail! "invalid wire run telemetry thread" {:thread raw}))
      canonical)))

(defn canonical-record [record]
  (json/generate-string (into (sorted-map) record)))

(defn validate-reported-run! [port subject scalar delivery-facts run-facts]
  (when (= "reported" (get delivery-facts "delivery_outcome"))
    (let [evidence (json/parse-string (get delivery-facts "delivery_evidence"))
          expected-reporter (str "@agent:" (get scalar "agent"))
          expected-thread (thread-entity (get scalar "thread"))
          reservation-origin
          (north.terminal-projection/singleton-value
           run-facts "run_reservation_contract_origin")
          reservation-baseline
          (north.terminal-projection/run-reservation-done-when run-facts)
          current-bars
          (north.terminal-projection/canonical-done-when
           (facts-of port expected-thread))
          records
          (set
           (mapcat (fn [match]
                     (map canonical-record (get match "evidence")))
                   (get evidence "matches")))
          evidence-state
          (north.terminal-projection/run-evidence-state
           run-facts subject expected-thread expected-reporter)
          stored-records (:raws evidence-state)]
      (when-not (north.terminal-projection/run-reservation-valid? run-facts)
        (fail! "reported run lost its committed reservation" {:subject subject}))
      (when-not (= #{expected-reporter} (get run-facts "run_reservation_agent"))
        (fail! "run telemetry agent does not match its reservation" {:subject subject}))
      (when-not (= #{expected-thread} (get run-facts "run_reservation_thread"))
        (fail! "run telemetry thread does not match its reservation" {:subject subject}))
      (when-not (= expected-reporter (get evidence "reporter"))
        (fail! "run evidence reporter must match its managed agent" {:subject subject}))
      (when-not (= subject (get evidence "run"))
        (fail! "run evidence must name the exact committed run subject" {:subject subject}))
      (when-not (= expected-thread (get evidence "thread"))
        (fail! "run evidence must name the exact driven thread" {:subject subject}))
      (when-not (= reservation-origin (get evidence "contractOrigin"))
        (fail! "run delivery contract origin differs from its reservation" {:subject subject}))
      (when-not (= reservation-baseline (get evidence "baselineDoneWhen"))
        (fail! "run delivery baseline differs from its reservation" {:subject subject}))
      (when-not (= current-bars (get evidence "doneWhen"))
        (fail! "run delivery contract changed before telemetry publication" {:subject subject}))
      (when-not (:valid? evidence-state)
        (fail! "reported run contains malformed or cross-scoped evidence" {:subject subject}))
      (when-not (= stored-records records)
        (fail! "run delivery snapshot must cite the exact stored evidence set"
               {:subject subject})))))

(defn durable-optional-event-facts [port run-id predicate]
  (let [response
        (north.coord/bounded-query-in-domain
         port :telemetry
         {:find "durable_wire_event_optional_fact"
          :rules [{:head {:rel "durable_wire_event_optional_fact"
                          :args [{:var "e"} {:var "value"}]}
                   :body [{:rel "triple" :args [{:var "e"} "kind" "wire_event"]}
                          {:rel "triple" :args [{:var "e"} "wire_run_id" run-id]}
                          {:rel "triple" :args [{:var "e"} predicate {:var "value"}]}]}]}
         (inc north.run-ledger/max-events))
        grouped (group-by first (:rows response))]
    (doseq [[subject rows] grouped]
      (when-not (= 1 (count rows))
        (fail! "durable wire event optional lineage is not singleton"
               {:subject subject :predicate predicate})))
    (into {} (map (fn [[subject rows]] [subject (second (first rows))]) grouped))))

(defn durable-wire-events [port run-id]
  (let [response
        (north.coord/bounded-query-in-domain
         port :telemetry
         {:find "durable_wire_event"
          :rules [{:head {:rel "durable_wire_event"
                          :args [{:var "e"} {:var "sequence"} {:var "id"}
                                 {:var "at"} {:var "kind"} {:var "essential"}
                                 {:var "json"} {:var "digest"} {:var "thread"}
                                 {:var "agent"}]}
                   :body [{:rel "triple" :args [{:var "e"} "kind" "wire_event"]}
                          {:rel "triple" :args [{:var "e"} "wire_run_id" run-id]}
                          {:rel "triple" :args [{:var "e"} "wire_event_sequence" {:var "sequence"}]}
                          {:rel "triple" :args [{:var "e"} "wire_event_id" {:var "id"}]}
                          {:rel "triple" :args [{:var "e"} "wire_event_at" {:var "at"}]}
                          {:rel "triple" :args [{:var "e"} "wire_event_kind" {:var "kind"}]}
                          {:rel "triple" :args [{:var "e"} "wire_event_essential" {:var "essential"}]}
                          {:rel "triple" :args [{:var "e"} "wire_event_json" {:var "json"}]}
                          {:rel "triple" :args [{:var "e"} "wire_event_sha256" {:var "digest"}]}
                          {:rel "triple" :args [{:var "e"} "thread" {:var "thread"}]}
                          {:rel "triple" :args [{:var "e"} "agent" {:var "agent"}]}]}]}
         (inc north.run-ledger/max-events))
        parent-threads (durable-optional-event-facts port run-id "parent_thread")
        coordinators (durable-optional-event-facts port run-id "run_coordinator")]
    (->> (:rows response)
         (mapv (fn [[subject sequence id at kind essential raw digest thread agent]]
                 (let [parsed-sequence (parse-long sequence)]
                   (when-not (and parsed-sequence
                                  (= digest (north.run-ledger/sha256 raw))
                                  (= subject (north.run-ledger/event-subject run-id parsed-sequence)))
                     (fail! "durable wire event projection is inconsistent" {:subject subject}))
                   {:subject subject :sequence parsed-sequence :id id :at at :kind kind
                    :essential essential :json raw :digest digest :thread thread :agent agent
                    :parent-thread (get parent-threads subject)
                    :coordinator (get coordinators subject)})))
         (sort-by :sequence)
         vec)))

(defn validate-durable-ledger! [port subject scalar]
  (let [run-id (get scalar "wire_run_id")
        events (durable-wire-events port run-id)
        expected-count (parse-long (get scalar "wire_event_count"))
        sequences (mapv :sequence events)
        terminal (last events)]
    (when-not (= expected-count (count events))
      (fail! "wire run summary count differs from durable events" {}))
    (when-not (= sequences (vec (range expected-count)))
      (fail! "durable wire event sequence is incomplete" {:sequences sequences}))
    (when-not (and (= "run.terminated" (:kind terminal))
                   (= (get scalar "wire_terminal_event_id") (:id terminal))
                   (= (get scalar "at") (:at terminal)))
      (fail! "wire run summary terminal differs from durable terminal event" {}))
    (when-not (every? #(and (= (get scalar "thread") (:thread %))
                            (= (get scalar "agent") (:agent %))
                            (= (get scalar "parent_thread") (:parent-thread %))
                            (= (get scalar "run_coordinator") (:coordinator %))) events)
      (fail! "wire run summary graph identity differs from durable events" {}))
    (when-not (= (get scalar "wire_ledger_sha256")
                 (north.run-ledger/ledger-digest
                  (mapv #(hash-map "digest" (:digest %)) events)))
      (fail! "wire run summary digest differs from durable events" {}))
    events))

(defn validate-core-projection! [subject facts events]
  (let [wire-jsonl (str (str/join "\n" (map :json events)) "\n")
        request (json/generate-string
                 {"subject" subject "facts" facts "wireJsonl" wire-jsonl})
        bun (or (System/getenv "NORTH_BUN") "bun")
        {:keys [exit]} (shell/sh bun wire-telemetry-validator :in request)]
    (when-not (zero? exit)
      (fail! "wire run core projection differs from the canonical reducer" {}))))

(defn terminal-fact-map [facts]
  (reduce (fn [result [predicate value]] (update result predicate (fnil conj #{}) value)) {} facts))

(def summary-chunk-size 200)
(def summary-lease-ttl-ms 120000)

(defn acquire-summary-lease! [port subject]
  (let [resource (str "wire-run-summary:" (north.run-ledger/sha256 subject))
        holder (str "wire-run-summary-writer:" (java.util.UUID/randomUUID))
        outcome
        (north.coord/retry-conflicts-until!
         (north.coord/retry-deadline-ns)
         (fn []
           (let [result (north.coord/acquire-lease!
                         port resource holder summary-lease-ttl-ms)]
             (cond
               (:epoch result) {:done (select-keys result [:resource :holder :epoch])}
               (= :held (:reject result)) {:reject :conflict}
               :else result))))]
    (or (:done outcome)
        (fail! "wire run summary lease is unavailable" {:subject subject :result outcome}))))

(defn release-summary-lease! [port lease]
  (try (north.coord/release-lease! port lease)
       (catch Exception _ nil)))

(defn summary-subset? [actual expected]
  (every? (fn [[predicate values]]
            (set/subset? values (get expected predicate #{})))
          actual))

(defn publish-summary-chunks! [port subject lease body-facts]
  ;; Beagle Store fences are SpaceId-local: the writer lease lives in :coordination,
  ;; while @run facts live in :telemetry. The lease serializes cooperative
  ;; writers; each telemetry chunk uses its own expected-version CAS.
  (when-not (:valid? (north.coord/check-lease! port lease))
    (fail! "wire run summary lease was lost before publication" {:subject subject}))
  (let [deadline (north.coord/retry-deadline-ns)]
  (loop []
    (when-not (< (System/nanoTime) deadline)
      (fail! "wire run summary chunk publication exceeded its retry deadline"
             {:subject subject}))
    (let [before (facts-of port subject)
          missing (filterv (fn [[predicate value]]
                             (not (contains? (get before predicate #{}) value)))
                           body-facts)]
      (when (seq missing)
        (let [chunk (vec (take summary-chunk-size missing))
              base (north.coord/cur-ver-for-subject port subject)
              result
              (north.coord/publish!
               port
               (mapv (fn [[predicate value]]
                       {:op :assert :subject subject :predicate predicate
                        :value value :cardinality :many})
                     chunk)
               {:expected-version base})]
          (cond
            (:ok result) (recur)
            (= :conflict (:reject result)) (do (Thread/sleep 1) (recur))
            :else (checked! result [:wire-run-summary-chunk subject]))))))
  )
  (let [before (facts-of port subject)
        missing (seq (filter (fn [[predicate value]]
                               (not (contains? (get before predicate #{}) value)))
                             body-facts))]
    (when missing
      (fail! "wire run summary body readback is incomplete" {:subject subject})))
  (let [deadline (north.coord/retry-deadline-ns)]
    (loop []
      (when-not (< (System/nanoTime) deadline)
        (fail! "wire run summary commit exceeded its retry deadline" {:subject subject}))
      (let [base (north.coord/cur-ver-for-subject port subject)
            result (north.coord/publish!
                    port [{:op :assert :subject subject :predicate "kind"
                           :value "run" :cardinality :many}]
                    {:expected-version base})]
        (cond
          (:ok result) result
          (= :conflict (:reject result)) (do (Thread/sleep 1) (recur))
          :else (checked! result [:wire-run-summary-commit subject]))))))

(defn publish-summary! [port subject facts scalar]
  (let [grouped (group-by first facts)
        summary-predicates (set/difference terminal-predicates learning-keys)
        summary-facts (filterv #(not (learning-keys (first %))) facts)
        body-facts (filterv #(not= "kind" (first %)) summary-facts)
        expected-summary (terminal-fact-map summary-facts)
        expected-readback (terminal-fact-map facts)
        terminal-learning-keys (set (filter learning-keys (keys grouped)))
        delivery-predicates (set north.terminal-projection/terminal-projection-predicates)
        delivery-facts (select-keys scalar delivery-predicates)
        validate-context!
        (fn [before validate-core?]
          (let [existing-summary (select-keys before summary-predicates)
                learning-before (select-keys before learning-keys)
                unexpected (seq (remove reservation-keys
                                        (remove summary-predicates (keys before))))
                reserved? (north.terminal-projection/run-reservation-valid? before)]
            (when unexpected
              (fail! "run subject has non-reservation facts before wire summary"
                     {:subject subject :predicates unexpected}))
            (when-not (summary-subset? existing-summary expected-summary)
              (fail! "run subject has a conflicting wire summary" {:subject subject}))
            (when (and (contains? existing-summary "kind")
                       (not= existing-summary expected-summary))
              (fail! "committed wire run summary is incomplete" {:subject subject}))
            (when (and (seq terminal-learning-keys) (empty? learning-before))
              (fail! "terminal run cannot introduce a learning assignment after execution"
                     {:subject subject}))
            (when (seq learning-before)
              (when-not (= learning-keys (set (keys learning-before)))
                (fail! "pre-provider learning assignment is incomplete" {:subject subject}))
              (when-not (= learning-keys terminal-learning-keys)
                (fail! "terminal run must repeat the complete pre-provider learning assignment"
                       {:subject subject}))
              (doseq [predicate learning-keys
                      :let [expected (set (map second (get grouped predicate [])))
                            actual (get before predicate #{})]]
                (when-not (= expected actual)
                  (fail! "terminal run learning assignment differs from pre-provider assignment"
                         {:subject subject :predicate predicate}))))
            (when (and (= "reported" (get delivery-facts "delivery_outcome"))
                       (not reserved?))
              (fail! "reported delivery requires a committed run reservation"
                     {:subject subject}))
            (when reserved?
              (let [expected-agent (str "@agent:" (get scalar "agent"))
                    expected-thread (get scalar "thread")]
                (when-not (= #{expected-agent} (get before "run_reservation_agent"))
                  (fail! "wire run telemetry agent differs from its reservation" {}))
                (when-not (= #{expected-thread} (get before "run_reservation_thread"))
                  (fail! "wire run telemetry thread differs from its reservation" {}))))
            (when (contains? delivery-facts "delivery_outcome")
              (when-not (north.terminal-projection/delivery-projection-valid? delivery-facts)
                (fail! "run delivery outcome lacks a valid proof projection" {:subject subject}))
              (validate-reported-run! port subject scalar delivery-facts before))
            (when validate-core?
              (let [events (validate-durable-ledger! port subject scalar)]
                (validate-core-projection! subject facts events)))
            existing-summary))]
    (validate-operation-evidence! (get grouped "mcp_operation_receipt" [])
                                  (get grouped "mcp_operation_aggregate" []))
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
    ;; Every authority check, including the fixed event reducer, precedes even
    ;; the summary lease acquisition so a rejected projection has zero mutation.
    (validate-context! (facts-of port subject) true)
    (let [lease (acquire-summary-lease! port subject)]
      (try
        (let [before (facts-of port subject)
              existing-summary (validate-context! before false)]
          (when-not (= existing-summary expected-summary)
            (publish-summary-chunks! port subject lease body-facts)))
        (finally (release-summary-lease! port lease))))
    (let [stored (facts-of port subject)]
      (doseq [[predicate values] expected-readback]
        (when-not (set/subset? values (get stored predicate #{}))
          (fail! "wire run telemetry readback is incomplete"
                 {:subject subject :predicate predicate}))))))

(defn -main [& args]
  (let [[port-s subject-s] args
        _ (when-not (= 2 (count args))
            (fail! "usage: run-fact-internal.clj PORT SUBJECT < FACTS_JSON" {:argc (count args)}))
        port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
        subject (entity subject-s)
        facts (payload (bounded-stdin))
        scalar (validate-summary-facts! subject facts)]
    (publish-summary! port subject facts scalar)
    (println (json/generate-string {:ok true :subject subject :facts (count facts)}))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
