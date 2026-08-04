#!/usr/bin/env bb
;; Evidence-aware routing feedback. Operational completion, self-reported thread
;; evidence, and independent delivery verification remain separate axes; none is
;; presented as causal model quality.

(require '[cheshire.core :as json]
         '[babashka.process :as proc]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def NORTH (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
(load-file (str NORTH "/cli/orchestration-staffing.clj"))
(load-file (str NORTH "/cli/terminal-projection.clj"))
(load-file (str NORTH "/cli/harness-state.clj"))

(def multi-preds #{"done_when" "bar_evidence" "domain_requirement"
                   "applied_capability" "applied_domain_requirement"
                   "composition_override" "applied_preset_override" "struggle"
                   "routing_rule_code" "routing_pin" "routing_receipt_override"
                   "mcp_actual_tool" "provider_turn_key" "canary_outcome"
                   "scope_escalation" "effective_authority_capability"
                   "effective_north_enabled_tool" "effective_builtin"
                   "effective_mcp_tool"})

(def canonical-orchestration-capabilities
  ["filesystem.read" "filesystem.search" "filesystem.write" "shell"
   "shell.readonly" "web" "coordination"])
(def bespoke-fingerprint-version "v1")
(def bespoke-fingerprint-domain "north:bespoke-contract:v1")
(def applied-axis-preds
  [[:taskGrade "applied_task_grade"]
   [:topology "applied_topology"]
   [:tier "applied_routing_tier"]
   [:reasoning "applied_reasoning"]
   [:posture "applied_posture"]])
(def applied-axis-values
  {:taskGrade #{"novice" "junior" "mid" "senior" "staff" "principal" "research-grade" "distinguished"}
   :topology #{"worker" "orchestrator"}
   :tier #{"economy" "standard" "senior" "frontier"}
   :reasoning #{"low" "medium" "high" "xhigh" "max"}
   :posture #{"explore" "evaluate" "deliver" "preserve"}})
(def sha256-pattern #"^[0-9a-f]{64}$")
(def safe-role-id-pattern #"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
(def managed-composition-kinds #{"preset" "bespoke"})
(def delivery-outcomes #{"unverified" "reported" "verified" "blocked"})
(def judgment-grade-values #{"s" "m" "l"})
(def judgment-grade-status-values #{"valid" "unavailable" "invalid"})
(def judgment-grade-source-values #{"thread" "ad-hoc"})
(def struggle-trigger-values #{"consecutive_errors" "tool_loop" "no_progress"})
(def routing-override-fields
  ["taskGrade" "domainRequirements" "tier" "reasoning" "posture"])
(declare normalized-token normalized-domains capability-summary attributed?)

(defn normalized-preset-template [defaults preset]
  (let [effective (merge defaults preset)
        capabilities (capability-summary (get effective "capabilities" []))]
    {:axes {:taskGrade (normalized-token (get effective "taskGrade"))
            :domains (normalized-domains (get effective "domainRequirements" []))
            :topology (normalized-token (get effective "topology"))
            :tier (normalized-token (get effective "tier"))
            :reasoning (normalized-token (get effective "deliberation"))
            :posture (normalized-token (get effective "posture"))}
     :capabilities (:canonical capabilities)
     :unknownCapabilities (:unknown capabilities)}))

(defn current-preset-catalog []
  (try
    (let [catalog (north.orchestration-staffing/load-catalog)
          defaults (get catalog "defaults")
          presets (into {}
                        (map (fn [[id preset]]
                               [id (normalized-preset-template defaults preset)]))
                        (north.orchestration-staffing/presets-by-name catalog))]
      (if (seq presets)
        {:available true :ids (set (keys presets)) :presets presets}
        {:available false :ids #{} :presets {}}))
    (catch Exception _
      {:available false :ids #{} :presets {}})))

(defn default-paths []
  (let [home (System/getenv "HOME")
        dir (str home "/.local/state/north")
        split (io/file dir "coordination.log")]
    [(or (System/getenv "FRAM_LOG")
         (if (.exists split) (.getPath split) (str dir "/facts.log")))
     (or (System/getenv "FRAM_TELEMETRY_LOG")
         (let [path (str dir "/telemetry.log")] (when (.exists (io/file path)) path)))]))

(defn routing-policy-path []
  (or (System/getenv "NORTH_ROUTING_POLICY")
      (str (System/getProperty "user.home") "/.config/north/routing-policy.json")))

(defn configured-targets
  "Current routing-policy targets. A bounded usage report includes each one in
  every interval, even when it has no terminal runs there. Used targets absent
  from the current policy are added separately by the report builder."
  []
  (let [file (io/file (routing-policy-path))]
    (if-not (.exists file) []
      (let [document (json/parse-string (slurp file))
            targets (get document "targets" [])]
        (when-not (sequential? targets)
          (throw (ex-info "routing policy targets must be an array" {})))
        (->> targets
             (map-indexed
              (fn [index target]
                (let [id (when (map? target) (normalized-token (get target "id")))
                      provider (when (map? target)
                                 (normalized-token (get target "provider")))]
                  (when-not (and id provider)
                    (throw (ex-info (str "routing policy target " index
                                         " must name id and provider") {})))
                  {:providerTarget id :provider provider :configuredNow true})))
             distinct vec)))))

(defn accounts-root []
  (or (System/getenv "NORTH_ACCOUNTS_ROOT")
      (str (System/getProperty "user.home") "/.local/state/north/accounts")))

(defn configured-account-log-targets []
  (let [file (io/file (routing-policy-path))]
    (if-not (.exists file) []
      (let [document (json/parse-string (slurp file))
            targets (get document "targets" [])]
        (->> targets
             (keep (fn [target]
                     (let [id (normalized-token (get target "id"))
                           provider (normalized-token (get target "provider"))
                           profile (or (normalized-token (get target "profile")) id)]
                       (when (and id profile (#{"anthropic" "openai"} provider))
                         {:providerTarget id :provider provider
                          :root (io/file (accounts-root) provider profile)}))))
             vec)))))

(def log-predicate-pattern #":p \"([^\"]+)\"")

(defn read-ops
  ([paths] (read-ops paths nil))
  ([paths predicate-filter]
   (mapcat (fn [path]
             (if (and path (.exists (io/file path)))
               (with-open [reader (io/reader path)]
                 (doall
                  (keep (fn [line]
                          (let [predicate (some-> (re-find log-predicate-pattern line) second)]
                            (when (or (nil? predicate-filter)
                                      (and predicate (predicate-filter predicate)))
                              (try (edn/read-string line) (catch Exception _ nil)))))
                        (line-seq reader))))
               []))
           (distinct (remove nil? paths)))))

(def learning-fast-predicates
  #{"kind" "agent" "thread" "at" "outcome" "process_outcome"
    "delivery_outcome" "delivery_reason" "delivery_evidence"
    "delivery_evidence_sha256" "delivery_attestation"
    "delivery_attestation_sha256" "tokens" "duration_ms" "num_turns"
    "struggle" "error_count" "done_when" "bar_evidence"
    "mcp_activity_source" "mcp_activity_coverage" "mcp_actual_calls"
    "mcp_actual_tool" "execution_source" "effective_authority_provider"
    "effective_authority_capability" "authoring_authority_surface"
    "authoring_authority_surface_coverage" "prompt_receipt_version"
    "prompt_receipt_sha256" "prompt_wire_sha256" "prompt_receipt_coverage"
    "environment_receipt_version" "environment_receipt_sha256"
    "environment_receipt_coverage" "available_skill_catalog_sha256"
    "activated_resource_closure_sha256" "run_envelope_version"
    "run_envelope_sha256" "mcp_operation_aggregate"
    "native_command_activity_coverage" "native_command_completion"})

(defn learning-fast-predicate? [predicate]
  (or (contains? learning-fast-predicates predicate)
      (str/starts-with? predicate "learning_")))

(defn fold-facts [ops]
  (reduce
   (fn [facts {:keys [op l p r]}]
     (if-not (and l p) facts
       (let [current (get-in facts [l p] [])]
         (cond
           (= op "assert")
           (assoc-in facts [l p]
                     (if (multi-preds p) (if (some #{r} current) current (conj current r)) [r]))
           (= op "retract")
           (let [remaining (vec (remove #{r} current))]
             (if (seq remaining) (assoc-in facts [l p] remaining) (update facts l dissoc p)))
           :else facts))))
   {} ops))

(defn one [facts entity pred] (last (get-in facts [entity pred])))
(defn many [facts entity pred] (get-in facts [entity pred] []))
(defn long' [value] (try (parse-long (str value)) (catch Exception _ 0)))
(defn maybe-long [value] (when (some? value) (try (parse-long (str value)) (catch Exception _ nil))))
(defn maybe-double [value]
  (when (some? value)
    (try
      (let [parsed (parse-double (str value))]
        (when (Double/isFinite parsed) parsed))
      (catch Exception _ nil))))
(defn maybe-positive-long [value]
  (let [parsed (maybe-long value)]
    (when (and parsed (pos? parsed)) parsed)))
(defn observed-turns [value process-outcome]
  (let [parsed (maybe-long value)]
    (when (and (some? parsed)
               (not (neg? parsed))
               (or (pos? parsed)
                   ;; A preflight block proves that no provider turn began.
                   ;; Historical successful runs used 0 as a missing-value
                   ;; sentinel, so zero is not evidence for any other outcome.
                   (and (zero? parsed)
                        (= "blocked_preflight" process-outcome))))
      parsed)))
(def model-alias-catalog-providers ["anthropic" "openai"])

(defn- orchestration-catalog-root []
  (or (System/getenv "NORTH_ORCHESTRATION_HOME")
      (str (or (System/getenv "NORTH_HOME") NORTH) "/orchestration")))

(defn- load-provider-catalog [provider]
  (try
    (let [file (io/file (orchestration-catalog-root) "providers" (str provider ".json"))]
      (when (.exists file) (json/parse-string (slurp file))))
    (catch Exception _ nil)))

(defn model-alias-map
  "Read-time alias -> canonical model id, assembled from the Orchestration provider
  catalogs' modelAliases (bare tier names like opus/sonnet/fable/luna/terra/sol
  never appear as canonical model ids). This is the ONE place aliases are
  normalized until the write-side fix + migration land; that fix should reuse
  or replace this function rather than growing a second mapping."
  []
  (into {}
        (mapcat (fn [provider]
                  (get (load-provider-catalog provider) "modelAliases")))
        model-alias-catalog-providers))

(defn normalize-model-alias [alias-map model]
  (or (get alias-map model) model))

(defn derive-provider-from-model
  "Provider derivation for runs that recorded a model fact but no provider
  fact. Only covers the canonical id prefixes; anything else stays unattributed
  rather than guessing."
  [model]
  (cond
    (nil? model) nil
    (str/starts-with? model "claude-") "anthropic"
    (str/starts-with? model "gpt-") "openai"
    :else nil))

(defn thread-ref [value]
  (when (and value (not= value "(ad-hoc)"))
    (if (str/starts-with? value "@") value (str "@" value))))

(defn normalized-token [value]
  (let [token (some-> value str str/trim)] (when (seq token) token)))

(defn json-map [value]
  (try
    (let [parsed (when value (json/parse-string value))]
      (when (map? parsed) parsed))
    (catch Exception _ nil)))

(defn normalized-domain [value]
  (some-> (normalized-token value)
          (java.text.Normalizer/normalize java.text.Normalizer$Form/NFC)
          (.toLowerCase java.util.Locale/ROOT)))

(defn normalized-domains [values]
  (->> values (keep normalized-domain) distinct sort vec))

(defn capability-summary [values]
  (let [normalized (->> values (keep normalized-token) distinct vec)
        requested (set normalized)
        unknown (->> normalized (remove (set canonical-orchestration-capabilities)) sort vec)]
    {:canonical (vec (filter requested canonical-orchestration-capabilities))
     :unknown unknown}))

(defn sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256")
                        (.getBytes (str value) "UTF-8"))]
    (apply str (map #(format "%02x" (bit-and % 0xff)) digest))))

(defn override-summary [values]
  (let [normalized (->> values (keep normalized-token) distinct vec)
        known (set routing-override-fields)]
    {:values normalized
     :canonical (set (filter known normalized))
     :unknown (vec (remove known normalized))}))

(defn preset-application-debt
  "Compare one run's applied prompt authority with the current stock template.
  Overrides are valid only when both requested and applied evidence cover the
  exact semantic delta, and a nonempty delta carries its rationale digest."
  [template effective-axes applied-capabilities requested-overrides requested-reason
   applied-overrides reason-hash]
  (let [expected (:axes template)
        actual (select-keys effective-axes (keys expected))
        topology-mismatch? (not= (:topology actual) (:topology expected))
        deltas (->> [["taskGrade" :taskGrade]
                     ["domainRequirements" :domains]
                     ["tier" :tier]
                     ["reasoning" :reasoning]
                     ["posture" :posture]]
                    (keep (fn [[field axis]]
                            (when (not= (get actual axis) (get expected axis)) field)))
                    set)
        requested (override-summary requested-overrides)
        applied (override-summary applied-overrides)
        override-evidence? (or (seq (:values requested))
                               (some? requested-reason)
                               (seq (:values applied))
                               (some? reason-hash))]
    (vec
     (concat
      (when (seq (:unknownCapabilities template))
        ["current-preset-has-noncanonical-capabilities"])
      (when (not= applied-capabilities (:capabilities template))
        ["preset-applied-capabilities-mismatch"])
      (when topology-mismatch?
        ["preset-topology-mismatch"])
      (when (seq (:unknown requested))
        ["invalid-composition-override-evidence"])
      (when (seq (:unknown applied))
        ["invalid-applied-preset-override-evidence"])
      (if (empty? deltas)
        (when override-evidence? ["unexpected-preset-override-evidence"])
        (concat
         (when (not= deltas (:canonical requested))
           ["composition-override-coverage-mismatch"])
         (when (not= deltas (:canonical applied))
           ["applied-preset-override-coverage-mismatch"])
         (when (nil? requested-reason)
           ["missing-composition-override-reason"])
         (cond
           (nil? reason-hash) ["missing-applied-preset-override-reason-sha256"]
           (not (re-matches sha256-pattern reason-hash))
           ["invalid-applied-preset-override-reason-sha256"]
           :else [])
         (when (and requested-reason
                    reason-hash
                    (re-matches sha256-pattern reason-hash)
                    (not= (sha256 requested-reason) reason-hash))
           ["applied-preset-override-reason-sha256-mismatch"])))))))

(def requested-axis-order
  [[:taskGrade "taskGrade"] [:topology "topology"] [:tier "tier"]
   [:reasoning "reasoning"] [:posture "posture"] [:domains "domainRequirements"]])

(defn requested-applied-axis-debt [requested applied]
  (let [missing (->> requested-axis-order
                     (keep (fn [[axis label]]
                             (when (and (not= axis :domains)
                                        (nil? (get requested axis)))
                               label)))
                     vec)
        mismatched (->> requested-axis-order
                        (keep (fn [[axis label]]
                                (when (and (or (= axis :domains)
                                               (some? (get requested axis)))
                                           (not= (get requested axis)
                                                 (get applied axis)))
                                  label)))
                        vec)]
    (vec
     (concat
      (when (seq missing)
        [(str "missing-requested-axes:" (str/join "," missing))])
      (when (seq mismatched)
        [(str "requested-applied-axes-mismatch:" (str/join "," mismatched))])))))

(defn evidence [facts thread]
  (if-not thread
    {:status "no-contract" :bars 0 :evidenced 0 :hasOutcome false}
    (let [bars (many facts thread "done_when")
          evs (many facts thread "bar_evidence")
          outcome? (boolean (one facts thread "outcome"))
          evidenced
          (count
           (filter
            (fn [bar]
              (some #(north.terminal-projection/evidence-reports-bar? bar %) evs))
            bars))
          total (count bars)
          status (cond
                   (zero? total) "no-contract"
                   (and outcome? (= evidenced total)) "thread-closed-evidenced"
                   (= evidenced total) "thread-open-evidenced"
                   (pos? evidenced) "partial"
                   :else "unevidenced")]
      {:status status :bars total :evidenced evidenced :hasOutcome outcome?})))

(defn run-rows [facts]
  (let [preset-catalog (current-preset-catalog)
        alias-map (model-alias-map)]
   (for [[entity predicates] facts
        :when (and (= "run" (one facts entity "kind"))
                   ;; both legacy `@run-` and telemetry-routable `@run:` ids
                   (or (str/starts-with? entity "@run-")
                       (str/starts-with? entity "@run:")))]
    (let [agent (one facts entity "agent")
          identity (str "@agent:" agent)
          get' (fn [pred fallback] (or (one facts entity pred) (one facts identity pred) fallback))
          raw-model (normalize-model-alias alias-map (get' "model" nil))
          raw-provider (get' "provider" nil)
          derived-provider (when-not raw-provider (derive-provider-from-model raw-model))
          thread (thread-ref (one facts entity "thread"))
          composition-kind (get' "composition_kind" nil)
          composition-id (normalized-token (get' "composition_id" nil))
          role (normalized-token (get' "role" nil))
          process-outcome (normalized-token (one facts entity "process_outcome"))
          effective-process-outcome (or process-outcome (get' "outcome" "unrecorded"))
          run-facts (get facts entity {})
          lane-facts (get facts identity {})
          lane-process-outcome
          (north.terminal-projection/terminal-process-outcome lane-facts)
          lane-delivery-candidate
          (north.terminal-projection/terminal-delivery-outcome lane-facts)
          lane-evidence-candidate
          (json-map (north.terminal-projection/singleton-value
                     lane-facts "delivery_evidence"))
          lane-delivery-outcome
          (when (and (= effective-process-outcome lane-process-outcome)
                     (#{"reported" "verified"} lane-delivery-candidate)
                     (= entity (get lane-evidence-candidate "run"))
                     (= thread (get lane-evidence-candidate "thread"))
                     (= identity (get lane-evidence-candidate "reporter")))
            lane-delivery-candidate)
          run-delivery-outcome (normalized-token (one facts entity "delivery_outcome"))
          run-evidence-candidate
          (json-map (north.terminal-projection/singleton-value
                     run-facts "delivery_evidence"))
          run-delivery-valid
          (and run-delivery-outcome
               (north.terminal-projection/delivery-projection-valid? run-facts)
               (or (#{"unverified" "blocked"} run-delivery-outcome)
                   (and (= entity (get run-evidence-candidate "run"))
                        (= thread (get run-evidence-candidate "thread"))
                        (= identity (get run-evidence-candidate "reporter")))))
          delivery-outcome (or lane-delivery-outcome
                               (when run-delivery-valid run-delivery-outcome))
          delivery-source (cond
                            lane-delivery-outcome "lane-terminal"
                            run-delivery-valid "run"
                            :else nil)
          delivery-projection-facts (cond
                                      lane-delivery-outcome lane-facts
                                      run-delivery-valid run-facts
                                      :else {})
          delivery-evidence
          (json-map (north.terminal-projection/singleton-value
                     delivery-projection-facts "delivery_evidence"))
          delivery-attestation
          (json-map (north.terminal-projection/singleton-value
                     delivery-projection-facts "delivery_attestation"))
          delivery-reason
          (normalized-token
           (if lane-delivery-outcome
             (north.terminal-projection/singleton-value lane-facts "delivery_reason")
             (when run-delivery-valid (one facts entity "delivery_reason"))))
          delivery-proof-valid (boolean delivery-outcome)
          prompt-composition-applied (normalized-token
                                      (one facts entity "prompt_composition_applied"))
          applied-role-contract (normalized-token (one facts entity "applied_role_contract"))
          expected-role-contract (when (and composition-kind composition-id)
                                   (str composition-kind ":" composition-id))
          applied-hash (normalized-token (one facts entity "applied_bespoke_contract_sha256"))
          applied-version (normalized-token
                           (one facts entity "applied_bespoke_contract_fingerprint_version"))
          applied-domain (normalized-token
                          (one facts entity "applied_bespoke_contract_fingerprint_domain"))
          requested-hash (normalized-token (one facts identity "composition_contract_sha256"))
          requested-version (normalized-token
                             (one facts identity "composition_contract_fingerprint_version"))
          requested-domain (normalized-token
                            (one facts identity "composition_contract_fingerprint_domain"))
          requested-values [requested-hash requested-version requested-domain]
          requested-integrity (cond
                                (not-any? some? requested-values) "not-observed"
                                (not-every? some? requested-values) "incomplete-requested-evidence"
                                (= requested-values [applied-hash applied-version applied-domain]) "matched"
                                :else "mismatch")
          capability-evidence (capability-summary (many facts entity "applied_capability"))
          applied-capabilities (:canonical capability-evidence)
          execution-source (normalized-token (one facts entity "execution_source"))
          authority-provider (normalized-token
                              (one facts entity "effective_authority_provider"))
          authority-capability-evidence
          (capability-summary (many facts entity "effective_authority_capability"))
          authority-capabilities (:canonical authority-capability-evidence)
          explicit-authoring-authority
          (normalized-token (one facts entity "authoring_authority_surface"))
          explicit-authoring-authority-coverage
          (normalized-token (one facts entity "authoring_authority_surface_coverage"))
          derived-authoring-authority
          (when (and (= "north-managed" execution-source)
                     authority-provider
                     (empty? (:unknown authority-capability-evidence)))
            (cond
              (some #{"filesystem.write" "shell"} authority-capabilities) "text"
              :else "none"))
          authoring-authority
          (if (#{"text" "none" "unknown"} explicit-authoring-authority)
            explicit-authoring-authority
            (or derived-authoring-authority "unknown"))
          authoring-authority-coverage
          (if (and (#{"text" "none" "unknown"} explicit-authoring-authority)
                   (#{"exact" "unknown"} explicit-authoring-authority-coverage))
            explicit-authoring-authority-coverage
            (if derived-authoring-authority "exact" "unknown"))
          authoring-authority-evidence-source
          (cond
            (and explicit-authoring-authority explicit-authoring-authority-coverage)
            "run-fact"
            derived-authoring-authority "effective-authority"
            :else "unobserved")
          composition-overrides (many facts entity "composition_override")
          composition-override-reason
          (normalized-token (one facts entity "composition_override_reason"))
          applied-preset-overrides (many facts entity "applied_preset_override")
          applied-preset-override-reason-hash
          (normalized-token (one facts entity "applied_preset_override_reason_sha256"))
          applied-domain-values (many facts entity "applied_domain_requirement")
          applied-domain-count-raw (one facts entity "applied_domain_requirement_count")
          applied-domain-count (maybe-long applied-domain-count-raw)
          effective-axes (assoc
                          (into {} (map (fn [[axis pred]]
                                         [axis (normalized-token (one facts entity pred))])
                                       applied-axis-preds))
                          :domains (normalized-domains
                                    applied-domain-values))
          requested-axes
          {:taskGrade (normalized-token (one facts entity "task_grade"))
           :topology (normalized-token (one facts entity "topology"))
           :tier (normalized-token (one facts entity "routing_tier"))
           :reasoning (normalized-token (one facts entity "requested_reasoning"))
           :posture (normalized-token (one facts entity "routing_posture"))
           :domains (normalized-domains (many facts entity "domain_requirement"))}
          requested-applied-axis-debt
          (requested-applied-axis-debt requested-axes effective-axes)
          missing-axes (->> applied-axis-preds
                            (keep (fn [[axis _]] (when-not (get effective-axes axis) (name axis))))
                            vec)
          invalid-axes (->> applied-axis-values
                            (keep (fn [[axis values]]
                                    (let [value (get effective-axes axis)]
                                      (when (and value (not (values value))) (name axis)))))
                            sort vec)
          common-applied-debt
          (concat
           (cond
             (nil? role) ["missing-role"]
             (not= role composition-id) ["role-composition-id-mismatch"]
             :else [])
           (when-not (= "true" prompt-composition-applied)
             ["missing-or-invalid-prompt-composition-applied"])
           (cond
             (nil? applied-role-contract) ["missing-applied-role-contract"]
             (not= expected-role-contract applied-role-contract)
             ["applied-role-contract-mismatch"]
             :else [])
           (cond
             (nil? applied-domain-count-raw) ["missing-applied-domain-count"]
             (or (nil? applied-domain-count) (neg? applied-domain-count))
             ["invalid-applied-domain-count"]
             (not= applied-domain-count (count applied-domain-values))
             ["applied-domain-count-mismatch"]
             :else [])
           (when (empty? applied-capabilities) ["missing-applied-capabilities"])
           (when (seq (:unknown capability-evidence)) ["noncanonical-applied-capabilities"])
           (when (seq missing-axes)
             [(str "missing-applied-axes:" (str/join "," missing-axes))])
           (when (seq invalid-axes)
             [(str "invalid-applied-axes:" (str/join "," invalid-axes))])
           (when-not delivery-proof-valid ["missing-or-invalid-delivery-proof"])
           requested-applied-axis-debt)
          preset-applied-debt
          (when (= "preset" composition-kind)
            (let [template (get-in preset-catalog [:presets composition-id])]
              (cond
                (not (:available preset-catalog)) ["current-preset-catalog-unavailable"]
                (nil? template) ["unknown-current-preset"]
                :else (preset-application-debt
                       template effective-axes applied-capabilities
                       composition-overrides composition-override-reason
                       applied-preset-overrides
                       applied-preset-override-reason-hash))))
          bespoke-applied-debt
          (when (= "bespoke" composition-kind)
            (concat
             (cond
               (nil? applied-hash) ["missing-applied-hash"]
               (not (re-matches sha256-pattern applied-hash)) ["invalid-applied-hash"]
               :else [])
             (when (not= bespoke-fingerprint-version applied-version)
               ["missing-or-unsupported-applied-fingerprint-version"])
             (when (not= bespoke-fingerprint-domain applied-domain)
               ["missing-or-unsupported-applied-fingerprint-domain"])
             (case requested-integrity
               "matched" []
               "not-observed" ["missing-requested-fingerprint"]
               [(str "requested-applied-fingerprint-" requested-integrity)])
             (when-not (and composition-id
                            (re-matches safe-role-id-pattern composition-id))
               ["missing-or-invalid-bespoke-composition-id"])))
          legacy-debt (vec (concat common-applied-debt preset-applied-debt
                                   bespoke-applied-debt))]
      {:entity entity :thread thread
       ;; Calibration evidence is immutable and RUN-LOCAL. Never fall back to
       ;; current thread/agent facts for these fields: later grade edits must not
       ;; relabel a completed run.
       :judgmentGrade (normalized-token (one facts entity "judgment_grade"))
       :judgmentGradeStatus (normalized-token (one facts entity "judgment_grade_status"))
       :judgmentGradeSource (normalized-token (one facts entity "judgment_grade_source"))
       :struggleTriggers (vec (many facts entity "struggle"))
       :struggleDetectorPolicyVersion
       (normalized-token (one facts entity "struggle_detector_policy_version"))
       :struggleTopology (normalized-token (one facts entity "struggle_topology"))
       :struggleErrorStreakThreshold
       (maybe-positive-long (one facts entity "struggle_error_streak_threshold"))
       :struggleLoopRepeatThreshold
       (maybe-positive-long (one facts entity "struggle_loop_repeat_threshold"))
       :struggleLoopWindow (maybe-positive-long (one facts entity "struggle_loop_window"))
       :struggleNoProgressTurnThreshold
       (maybe-positive-long (one facts entity "struggle_no_progress_turn_threshold"))
       :struggleErrorCount (maybe-long (one facts entity "error_count"))
       :provider (or raw-provider derived-provider "unattributed")
       :providerProvenance (cond raw-provider "observed"
                                  derived-provider "derived-from-model"
                                  :else "unattributed")
       :tier (or (:tier effective-axes)
                 (when-let [value (:tier requested-axes)]
                   (str "requested:" value))
                 (when-let [value (normalized-token (get' "requested_tier" nil))]
                   (str "requested-route:" value))
                 "unattributed")
       :tierProvenance (cond
                         (:tier effective-axes) "applied"
                         (:tier requested-axes) "requested-orchestration-fallback"
                         (attributed? (get' "requested_tier" nil)) "requested-route-fallback"
                         :else "unattributed")
       :model (or raw-model "unattributed") :effort (get' "effort" "unattributed")
       :providerTarget (or (normalized-token (get' "provider_target" nil))
                           "unattributed")
       :responseStrategyId (normalized-token (one facts entity "response_strategy_id"))
       :responseStrategyImplementation
       (normalized-token (one facts entity "response_strategy_implementation"))
       :responseStrategyVersion
       (normalized-token (one facts entity "response_strategy_version"))
       :mcpActivitySource (normalized-token (one facts entity "mcp_activity_source"))
       :mcpActivityCoverage (normalized-token (one facts entity "mcp_activity_coverage"))
       :mcpActualCalls (maybe-long (one facts entity "mcp_actual_calls"))
       :mcpActualTools
       (vec (keep (fn [raw]
                    (let [parsed (json-map raw)
                          server (normalized-token (get parsed "server"))
                          tool (normalized-token (get parsed "tool"))
                          count' (maybe-positive-long (get parsed "count"))]
                      (when (and server tool count')
                        {:server server :tool tool :count count'})))
                  (many facts entity "mcp_actual_tool")))
       :operationAggregates
       (vec (keep (fn [raw]
                    (let [parsed (json-map raw)
                          operation-type (normalized-token (get parsed "operation"))
                          count' (maybe-positive-long (get parsed "count"))
                          total-duration (maybe-long (get parsed "totalDurationMs"))
                          mean-duration (maybe-double (get parsed "meanDurationMs"))
                          failures (maybe-long (get parsed "failureCount"))]
                      (when (and operation-type count' total-duration mean-duration failures
                                 (<= 0 failures count') (<= 0 total-duration)
                                 (= mean-duration (/ total-duration (double count'))))
                        {:operationType operation-type
                         :count count' :totalDurationMs total-duration
                         :meanDurationMs mean-duration :failureCount failures})))
                  (many facts entity "mcp_operation_aggregate")))
       :nativeCommandActivityCoverage
       (normalized-token (one facts entity "native_command_activity_coverage"))
       :nativeCommandOperations
       (vec (keep (fn [raw]
                    (let [parsed (json-map raw)
                          shape (normalized-token (get parsed "shape"))
                          duration (maybe-long (get parsed "durationMs"))
                          status (normalized-token (get parsed "status"))]
                      (when (and (#{"read" "edit"} shape) duration status)
                        {:shape shape :durationMs duration :status status})))
                  (many facts entity "native_command_completion")))
       :requestedProvider (normalized-token (one facts entity "requested_provider"))
       :requestedTarget (normalized-token (one facts entity "requested_target"))
       :requestedModel (normalized-token (one facts entity "requested_model"))
       :requestedEffort (normalized-token (one facts entity "requested_effort"))
       :executionSource execution-source
       :executionTransport (normalized-token (one facts entity "execution_transport"))
       :providerSessionPersistence
       (normalized-token (one facts entity "provider_session_persistence"))
       :providerJoinKeyVersion
       (normalized-token (one facts entity "provider_join_key_version"))
       :providerJoinCoverage
       (normalized-token (one facts entity "provider_join_coverage"))
       :providerSessionKey
       (normalized-token (one facts entity "provider_session_key"))
       :providerTurnKeys
       (vec (sort (keep #(when (re-matches sha256-pattern %) %)
                        (many facts entity "provider_turn_key"))))
       :northSessionId (normalized-token (one facts entity "north_session_id"))
       :threadProvenance (normalized-token (one facts entity "thread_provenance"))
       :turnProvenance (normalized-token (one facts entity "turn_provenance"))
       :routingAssessmentStatus
       (normalized-token (one facts entity "routing_assessment_status"))
       :routingAssessmentPolicy
       (normalized-token (one facts entity "routing_assessment_policy"))
       :routingDerivedTier (normalized-token (one facts entity "routing_derived_tier"))
       :routingDerivedReasoning
       (normalized-token (one facts entity "routing_derived_reasoning"))
       :routingSelectedTier (normalized-token (one facts entity "routing_selected_tier"))
       :routingSelectedReasoning
       (normalized-token (one facts entity "routing_selected_reasoning"))
       :routingExceptionCode
       (normalized-token (one facts entity "routing_exception_code"))
       :routingPinEvidenceStatus
       (normalized-token (one facts entity "routing_pin_evidence_status"))
       :routingPinReasonCode
       (normalized-token (one facts entity "routing_pin_reason_code"))
       :routingPinDetail
       (normalized-token (one facts entity "routing_pin_detail"))
       :canaryOutcomes
       (vec (keep normalized-token (many facts entity "canary_outcome")))
       :canaryOutcome
       (let [values (vec (keep normalized-token
                               (many facts entity "canary_outcome")))]
         (when (= 1 (count values)) (first values)))
       :agent agent
       :routingAdmissionReceiptVersion
       (maybe-long (one facts entity "routing_admission_receipt_version"))
       :routingReceipt
       (when-let [version (maybe-long (one facts entity "routing_admission_receipt_version"))]
         {:version version
          :routingRequestSha256 (normalized-token (one facts entity "routing_request_sha256"))
          :routingAssessmentSha256
          (normalized-token (one facts entity "routing_assessment_sha256"))
          :routingPolicySha256 (normalized-token (one facts entity "routing_policy_sha256"))
          :providerCatalogsSha256
          (normalized-token (one facts entity "provider_catalogs_sha256"))
          :staffingCatalogSha256
          (normalized-token (one facts entity "staffing_catalog_sha256"))
          :assessmentStatus
          (normalized-token (one facts entity "routing_assessment_status"))})
       ;; Run time is deliberately run-local: a lane/session timestamp is not a
       ;; terminal-run timestamp and must not be borrowed for interval reports.
       :at (normalized-token (one facts entity "at"))
       :modelAvailability
       (when-let [source (normalized-token (one facts entity "model_availability_source"))]
         {:target (normalized-token (one facts entity "model_availability_target"))
          :source source
          :observedAt (normalized-token (one facts entity "model_availability_observed_at"))
          :model (normalized-token (one facts entity "model_availability_model"))
          :digest (normalized-token (one facts entity "model_availability_digest"))})
       :role (or role "unattributed")
       :taskGrade (or (:taskGrade effective-axes)
                      (when-let [value (:taskGrade requested-axes)]
                        (str "requested:" value))
                      "unattributed")
       :taskGradeProvenance (cond
                              (:taskGrade effective-axes) "applied"
                              (:taskGrade requested-axes) "requested-orchestration-fallback"
                              :else "unattributed")
       :outcome (get' "outcome" "unrecorded")
       :processOutcome effective-process-outcome
       :processOutcomeObserved (boolean process-outcome)
       :deliveryOutcome (or delivery-outcome "unrecorded")
       :deliveryOutcomeObserved (boolean delivery-outcome)
       :deliveryOutcomeSource delivery-source
       :deliveryProofValid delivery-proof-valid
       :deliveryEvidenceThread (get delivery-evidence "thread")
       :deliveryReporter (get delivery-evidence "reporter")
       :deliveryEvidenceSha256
       (north.terminal-projection/singleton-value
        delivery-projection-facts "delivery_evidence_sha256")
       :deliveryVerifier (get delivery-attestation "actor")
       :deliveryVerifierRole (get delivery-attestation "role")
       :deliveryAuthority (get delivery-attestation "authority")
       :deliveryReason delivery-reason
       :deliveryReasonObserved (boolean delivery-reason)
       :preflightCause (normalized-token (one facts entity "preflight_cause"))
       :retryOfRun (normalized-token (one facts entity "retry_of_run"))
       :retryAttempt (maybe-long (one facts entity "retry_attempt"))
       :reservedAt (normalized-token (one facts entity "run_reserved_at"))
       :tokens (maybe-long (get' "tokens" nil))
       ;; The learning assignment and construction receipts are immutable,
       ;; run-local evidence. Never borrow them from the lane identity: doing
       ;; so would relabel old episodes after a policy or environment change.
       :learningAssignmentVersion
       (normalized-token (one facts entity "learning_assignment_version"))
       :learningPolicyVersion
       (normalized-token (one facts entity "learning_policy_version"))
       :learningPolicySha256
       (normalized-token (one facts entity "learning_policy_sha256"))
       :learningMode (normalized-token (one facts entity "learning_mode"))
       :learningEvidenceMode
       (normalized-token (one facts entity "learning_evidence_mode"))
       :learningExperimentId
       (normalized-token (one facts entity "learning_experiment_id"))
       :learningEpisodeId
       (normalized-token (one facts entity "learning_episode_id"))
       :learningTaskSignatureSha256
       (normalized-token (one facts entity "learning_task_signature_sha256"))
       :learningTaskSignatureCoverage
       (normalized-token (one facts entity "learning_task_signature_coverage"))
       :learningRisk (normalized-token (one facts entity "learning_risk"))
       :learningArm (normalized-token (one facts entity "learning_arm"))
       :learningAxis (normalized-token (one facts entity "learning_axis"))
       :learningArmId (normalized-token (one facts entity "learning_arm_id"))
       :learningPropensity (maybe-double (one facts entity "learning_propensity"))
       :learningExplorePropensity
       (maybe-double (one facts entity "learning_explore_propensity"))
       :learningNarrowingReason
       (normalized-token (one facts entity "learning_narrowing_reason"))
       :learningBaselineSha256
       (normalized-token (one facts entity "learning_baseline_sha256"))
       :learningOptionsSha256
       (normalized-token (one facts entity "learning_options_sha256"))
       :learningAssignmentSha256
       (normalized-token (one facts entity "learning_assignment_sha256"))
       :promptReceiptVersion
       (normalized-token (one facts entity "prompt_receipt_version"))
       :promptReceiptSha256
       (normalized-token (one facts entity "prompt_receipt_sha256"))
       :promptWireSha256
       (normalized-token (one facts entity "prompt_wire_sha256"))
       :promptReceiptCoverage
       (normalized-token (one facts entity "prompt_receipt_coverage"))
       :environmentReceiptVersion
       (normalized-token (one facts entity "environment_receipt_version"))
       :environmentReceiptSha256
       (normalized-token (one facts entity "environment_receipt_sha256"))
       :environmentReceiptCoverage
       (normalized-token (one facts entity "environment_receipt_coverage"))
       :availableSkillCatalogSha256
       (normalized-token (one facts entity "available_skill_catalog_sha256"))
       :activatedResourceClosureSha256
       (normalized-token (one facts entity "activated_resource_closure_sha256"))
       :runEnvelopeVersion
       (normalized-token (one facts entity "run_envelope_version"))
       :runEnvelopeSha256
       (normalized-token (one facts entity "run_envelope_sha256"))
       :promptCompositionVersion (normalized-token (one facts entity "prompt_composition_version"))
       :capabilityClass (normalized-token (one facts entity "capability_class"))
       :promptStablePrefixBytes (maybe-long (one facts entity "prompt_stable_prefix_bytes"))
       :promptUniqueTailBytes (maybe-long (one facts entity "prompt_unique_tail_bytes"))
       :promptTotalBytes (maybe-long (one facts entity "prompt_total_bytes"))
       :promptTotalCompositionTokens
       (maybe-long (one facts entity "prompt_total_composition_tokens"))
       :inputTokens (maybe-long (one facts entity "input_tokens"))
       :outputTokens (maybe-long (one facts entity "output_tokens"))
       :cacheReadTokens (maybe-long (one facts entity "cache_read_tokens"))
       :cacheCreateTokens (maybe-long (one facts entity "cache_create_tokens"))
       :cachedInputTokens (maybe-long (one facts entity "cached_input_tokens"))
       :providerContextWindowTokens
       (maybe-long (one facts entity "provider_context_window_tokens"))
       :contextWindowStatus (normalized-token (one facts entity "context_window_status"))
       :effectiveContextBudgetTokens
       (maybe-long (one facts entity "effective_context_budget_tokens"))
       :contextBudgetStatus (normalized-token (one facts entity "context_budget_status"))
       :compactionCount (maybe-long (one facts entity "compaction_count"))
       ;; Historical adapters wrote 0 when they had no wall-clock observation.
       ;; A completed process cannot provide a real zero-millisecond duration, so
       ;; only positive observations count as evidence.
       :durationMs (maybe-positive-long (get' "duration_ms" nil))
       :estimateHours (maybe-double (get' "estimate_hours" nil))
       :estimateDeltaMs (maybe-long (get' "estimate_delta_ms" nil))
       :estimateRatio (maybe-double (get' "estimate_ratio" nil))
       :estimateClassification
       (let [value (normalized-token (one facts entity "estimate_classification"))]
         (when (#{"under" "on" "over"} value) value))
       :turns (observed-turns (get' "num_turns" nil) effective-process-outcome)
       :fallbacks (long' (get' "fallback_count" 0))
       :escalations (long' (get' "escalation_count" 0))
       :compositionKind composition-kind
       :compositionId composition-id
       :nearestPreset (get' "nearest_preset" nil)
       :bespokeReason (get' "bespoke_reason" nil)
       :promotionCandidate (= "true" (get' "promotion_candidate" "false"))
       :promptCompositionApplied (= "true" prompt-composition-applied)
       :appliedRoleContract applied-role-contract
       ;; Applied evidence is intentionally read from the run only. Requested
       ;; identity facts are not proof that the harness enforced a contract.
       :appliedContractSha256 applied-hash
       :appliedFingerprintVersion applied-version
       :appliedFingerprintDomain applied-domain
       :requestedAppliedIntegrity requested-integrity
       :requestedContractSha256 requested-hash
       :requestedFingerprintVersion requested-version
       :requestedFingerprintDomain requested-domain
       :appliedCapabilities applied-capabilities
       :effectiveAuthorityProvider authority-provider
       :effectiveAuthorityCapabilities authority-capabilities
       :authoringAuthoritySurface authoring-authority
       :authoringAuthoritySurfaceCoverage authoring-authority-coverage
       :authoringAuthorityEvidenceSource authoring-authority-evidence-source
       :compositionOverrides composition-overrides
       :appliedPresetOverrides applied-preset-overrides
       :appliedPresetOverrideReasonSha256 applied-preset-override-reason-hash
       :appliedDomainRequirementCount applied-domain-count
       :requestedAxes requested-axes
       :effectiveAxes effective-axes
       :legacyDebtReasons legacy-debt
       :evidence (evidence facts thread)}))))

(defn native-session-rows
  "Provider-native interactive sessions are entitlement activity, but do not
  publish terminal token totals or account targets. Keep them in a separate
  projection so their session counts are visible without contaminating managed
  run token percentages."
  [facts]
  (let [alias-map (model-alias-map)]
    (->> facts
         (keep (fn [[entity _]]
                 (when (str/starts-with? entity "@session:native-")
                   (let [agent (normalized-token (one facts entity "agent"))
                         identity (when agent (str "@agent:" agent))
                         model (normalize-model-alias
                                alias-map (normalized-token (one facts identity "model")))
                         provider (or (normalized-token (one facts identity "provider"))
                                      (derive-provider-from-model model)
                                      "unattributed")
                         actor-kind (or (normalized-token
                                         (one facts identity "native_actor_kind"))
                                        "unknown")
                         explicit-session-key
                         (normalized-token (one facts identity "provider_session_key"))
                         historical-session-key
                         (when (and (= "root" actor-kind)
                                    (string? agent)
                                    (re-matches #"native-[a-f0-9]{64}" agent))
                           (subs agent (count "native-")))]
                     {:entity entity
                      :provider provider
                      :model (or model "unattributed")
                      :effort (or (normalized-token (one facts identity "effort"))
                                  "unobserved")
                      :executionSource (or (normalized-token (one facts identity "execution_source"))
                                           "unknown")
                      :executionTransport (or (normalized-token (one facts identity "execution_transport"))
                                              "unknown")
                      :providerSessionPersistence
                      (or (normalized-token (one facts identity "provider_session_persistence"))
                          "unknown")
                      :providerJoinKeyVersion
                      (or (normalized-token (one facts identity "provider_join_key_version"))
                          (when historical-session-key "north-provider-join:v1-historical-native"))
                      :providerJoinCoverage
                      (or (normalized-token (one facts identity "provider_join_coverage"))
                          (when historical-session-key "partial") "unknown")
                      :providerSessionKey (or explicit-session-key historical-session-key)
                      :actorKind actor-kind
                      :depth (or (maybe-long (one facts identity "native_depth")) "unknown")
                      :dispatchModeAtStart
                      (or (normalized-token (one facts identity "dispatch_mode_at_start"))
                          "unknown")
                      :startedAt (normalized-token (one facts entity "started_at"))}))))
         (sort-by :entity) vec)))

(def cohort-fields [:provider :tier :role :taskGrade])
(def complete-attribution-fields (into cohort-fields [:model :effort]))
(defn cohort-label [row] (str/join "/" (map #(get row %) cohort-fields)))

(defn attributed? [value]
  (let [token (normalized-token value)]
    (and token (not (#{"?" "unknown" "unobserved" "unrecorded" "unattributed"} token)))))

;; A blocked delivery's :deliveryReason is a stable machine literal from
;; sdk/src/execution-outcome.ts's BLOCKED_REASON map, or (for reaper-written
;; deaths) cli/north-reactor.clj's publish-reaped-terminal!. classifyExecutionTerminal
;; folds ~16 distinct reasons into a single deliveryOutcome="blocked" bucket;
;; a coordinator reading that one number as a provider failure rate mistook
;; our own admission refusals for provider deaths and quarantined a provider
;; on it (thread 019f9c42). d-blk stays a stable total; these buckets answer
;; "whose fault" without joining the log by hand.
;;
;; provider-caused: the provider process itself is the terminal cause — it
;; died, stalled, or the SDK reported a terminal error. provider_terminal_empty_result
;; (a "ran" process yielding zero deliverable text, thread 019f8300) is filed
;; here too: the process reached its own terminal and produced a degenerate
;; completion, which is a provider quality failure, not North refusing to
;; contact the provider. It gets its own sub-bucket (not merged into
;; died/stalled/error) because "the process completed but delivered nothing"
;; is a materially different failure mode than "the process never terminated".
;;
;; north-caused: North's own admission/budget/turn/escalation/reconciliation
;; gates refused or gave up BEFORE (or independent of) any provider death.
;;
;; suspect-lapse: presence_lapsed_without_committed_terminal is the reaper's
;; verdict when a lane's presence lapsed without a committed terminal fact —
;; NOT a provider process outcome at all. Thread 019f9c3b (concurrent, as of
;; this writing) suspects a terminal-publication bug can produce this even
;; when the lane actually finished, i.e. the reaper manufactures a death.
;; Counting these as provider deaths would launder that bug into a provider
;; failure rate, so they get their own column and are never added to
;; provider-caused counts until 019f9c3b lands and this is re-measured.
;;
;; unattributed: an observed-but-unrecognized reason (schema drift, or the
;; single generic legacy strings some historical rows carry) — never silently
;; folded into either side.
(def blocked-reason-category
  {"provider_process_died" :provider-caused
   "provider_process_stalled" :provider-caused
   "provider_terminal_error" :provider-caused
   "provider_terminal_empty_result" :provider-caused
   "execution_preflight_blocked" :north-caused
   "spend_guard_budget_incomplete" :north-caused
   "provider_turn_cap" :north-caused
   "provider_cap" :north-caused
   "resource_envelope_exceeded" :north-caused
   "provider_escalation_unsupported" :north-caused
   "escalation_ladder_exhausted" :north-caused
   "orchestrator_children_live_at_terminal" :north-caused
   "orchestrator_minimum_children_not_dispatched" :north-caused
   "orchestrator_child_reconciliation_unavailable" :north-caused
   "orchestrator_child_results_unreconciled" :north-caused
   "orchestrator_child_relation_regressed" :north-caused
   "presence_lapsed_without_committed_terminal" :suspect-lapse})

(defn blocked-failure-category
  "Categorize a blocked row's :deliveryReason as :provider-caused, :north-caused,
  :suspect-lapse, or :unattributed (unrecognized reason). Only meaningful when
  the row's :deliveryOutcome is \"blocked\"."
  [delivery-reason]
  (get blocked-reason-category (normalized-token delivery-reason) :unattributed))

(defn complete-current-managed-run? [row]
  (and (managed-composition-kinds (:compositionKind row))
       (attributed? (:compositionId row))
       (:processOutcomeObserved row)
       (:deliveryOutcomeObserved row)
       (:deliveryReasonObserved row)
       (delivery-outcomes (:deliveryOutcome row))
       (every? #(attributed? (get row %)) complete-attribution-fields)
       (empty? (:legacyDebtReasons row))))

(defn performance-row [[label rows]]
  (let [statuses (frequencies (map #(get-in % [:evidence :status]) rows))
        deliveries (frequencies (map :deliveryOutcome rows))
        delivery-sources (frequencies (keep :deliveryOutcomeSource rows))
        delivery-authorities (frequencies (keep :deliveryAuthority rows))
        blocked-rows (filter #(= "blocked" (:deliveryOutcome %)) rows)
        blocked-categories (frequencies (map #(blocked-failure-category (:deliveryReason %))
                                             blocked-rows))
        blocked-reasons (frequencies (keep :deliveryReason blocked-rows))]
    {:cohort label :runs (count rows)
     :operationalRan (count (filter #(= "ran" (:processOutcome %)) rows))
     :deliveryVerified (get deliveries "verified" 0)
     :deliveryReported (get deliveries "reported" 0)
     :deliveryUnverified (get deliveries "unverified" 0)
     :deliveryBlocked (get deliveries "blocked" 0)
     :deliveryBlockedProviderCaused (get blocked-categories :provider-caused 0)
     :deliveryBlockedNorthCaused (get blocked-categories :north-caused 0)
     :deliveryBlockedSuspectLapse (get blocked-categories :suspect-lapse 0)
     :deliveryBlockedUnattributed (get blocked-categories :unattributed 0)
     :deliveryBlockedReasons blocked-reasons
     :deliveryUnrecorded (get deliveries "unrecorded" 0)
     :deliveryOutcomeSources delivery-sources
     :deliveryAuthorities delivery-authorities
     :threadOutcomes (count (filter #(get-in % [:evidence :hasOutcome]) rows))
     :threadClosedEvidenced (get statuses "thread-closed-evidenced" 0)
     :threadOpenEvidenced (get statuses "thread-open-evidenced" 0)
     :threadPartialEvidence (get statuses "partial" 0)
     :threadUnevidenced (get statuses "unevidenced" 0)
     :threadNoContract (get statuses "no-contract" 0)
     :escalated (count (filter #(pos? (:escalations %)) rows))}))

(defn performance-report
  ([rows] (performance-report rows false))
  ([rows all?]
   (let [all-rows (vec rows)
         selected (if all? all-rows (vec (filter complete-current-managed-run? all-rows)))]
     {:report "performance"
      :scope (if all? "all-history" "complete-current-managed")
      :evidenceVersion "v5"
      :claim (str "complete applied Orchestration contract plus proof-valid process/delivery outcomes; "
                  "reported is run-scoped self-report, independent verification is unavailable under "
                  "shared-UID lanes, and mutable thread review context is separate; not causal model "
                  "quality. deliveryBlocked is a stable total across process/delivery/orchestration "
                  "gates and is never a provider failure rate by itself — read "
                  "deliveryBlockedProviderCaused (died/stalled/error/empty-result) against "
                  "deliveryBlockedNorthCaused (our own preflight/spend-guard/cap/escalation/"
                  "reconciliation refusals) for that. deliveryBlockedSuspectLapse "
                  "(presence_lapsed_without_committed_terminal) is reaper-manufactured death "
                  "suspicion pending thread 019f9c3b and is excluded from both; "
                  "deliveryBlockedUnattributed holds reasons this report does not yet recognize.")
      :runs (count selected)
      :availableRuns (count all-rows)
      :excludedRuns (- (count all-rows) (count selected))
      :cohorts (->> selected (group-by cohort-label) (map performance-row) (sort-by :cohort) vec)})))

(defn usage-stats
  "Shared runs/tokens/wall/turns coverage stats for one cohort of rows,
  regardless of whether the cohort is keyed by provider, model, or model+effort."
  [rows]
  (let [tokens (keep :tokens rows)
        token-runs (count tokens)
        runs (count rows)
        durations (keep :durationMs rows)
        duration-runs (count durations)
        duration-ms (when (seq durations) (reduce + durations))
        turns (keep :turns rows)
        turn-runs (count turns)]
    {:runs runs
     :tokens (when (seq tokens) (reduce + tokens)) :tokenRuns token-runs
     :tokenCoverage {:exactRuns token-runs :runs runs}
     :tokenEvidence (cond
                      (zero? token-runs) "unobserved"
                      (= token-runs runs) "exact"
                      :else "lower-bound")
     :wallMilliseconds duration-ms
     :wallSeconds (when duration-ms (/ (double duration-ms) 1000.0))
     :durationRuns duration-runs
     :durationCoverage {:exactRuns duration-runs :runs runs}
     :durationEvidence (cond
                         (zero? duration-runs) "unobserved"
                         (= duration-runs runs) "exact"
                         :else "lower-bound")
     :turns (when (seq turns) (reduce + turns))
     :turnRuns turn-runs
     :turnCoverage {:exactRuns turn-runs :runs runs}
     :turnEvidence (cond
                     (zero? turn-runs) "unobserved"
                     (= turn-runs runs) "exact"
                     :else "lower-bound")
     :fallbacks (reduce + (map :fallbacks rows))
     :escalatedRuns (count (filter #(pos? (:escalations %)) rows))}))

(defn usage-row [[provider rows]]
  (assoc (usage-stats rows)
         :provider provider
         :derivedRuns (count (filter #(= "derived-from-model" (:providerProvenance %)) rows))))

(defn model-row [[model rows]]
  (assoc (usage-stats rows) :model model))

(defn model-effort-row [[[model effort] rows]]
  (assoc (usage-stats rows) :model model :effort effort))

(defn models-report [rows by-effort?]
  (if by-effort?
    (->> rows
         (group-by (juxt :model :effort))
         (map model-effort-row)
         (sort-by (juxt :model :effort))
         vec)
    (->> rows (group-by :model) (map model-row) (sort-by :model) vec)))

(defn usage-report
  ([rows] (usage-report rows {}))
  ([rows {:keys [by-model? by-effort?]}]
   ;; --by-effort implies the model breakdown even without an explicit
   ;; --by-model flag: "model x effort" has no meaning without a model axis.
   (cond-> {:report "usage" :unit "observed work, never dollars or API credits"
            :runs (count rows)
            :providers (->> rows (group-by :provider) (map usage-row) (sort-by :provider) vec)}
     (or by-model? by-effort?) (assoc :models (models-report rows by-effort?)))))

(defn parse-instant [value]
  (when value
    (try (java.time.Instant/parse value) (catch Exception _ nil))))

(def waste-window-token-limit 1000000)
(def waste-minimum-runs 20)
(def waste-minimum-exact-coverage-percent 90.0)
(def waste-bucket-order
  ["blocked-preflight" "reservation-transport" "startup-ack-timeout"
   "zero-delivery-provider-death" "died-unreported" "retry-duplicate"])

(def reservation-transport-reasons
  #{"delivery_reservation_publication_unverified"
    "delivery_reservation_unavailable_at_finalize"
    "delivery_reservation_load_failed_at_finalize"
    "delivery_thread_load_failed_at_finalize"
    "delivery_thread_unavailable_at_finalize"
    "coordinator-transport-failure"
    "transport-or-filesystem-failure"
    "transport-or-local-failure"})

(def checkpointed-escalation-outcomes
  #{"provider_escalation_unsupported" "max_tier"
    "orchestrator_children_incomplete" "orchestrator_child_obligation_unmet"
    "child_reconciliation_unavailable" "orchestrator_reduction_incomplete"
    "orchestrator_child_set_inconsistent"})

(def checkpointed-escalation-reasons
  #{"provider_escalation_unsupported" "escalation_ladder_exhausted"
    "orchestrator_children_live_at_terminal"
    "orchestrator_minimum_children_not_dispatched"
    "orchestrator_child_reconciliation_unavailable"
    "orchestrator_child_results_unreconciled"
    "orchestrator_child_relation_regressed"
    "scope_escalation_checkpointed"})

(defn exact-run-tokens [row]
  (let [tokens (:tokens row)]
    (when (and (integer? tokens) (not (neg? tokens))) (long tokens))))

(defn terminal-row? [row]
  (and (:processOutcomeObserved row)
       (:deliveryOutcomeObserved row)
       (:deliveryReasonObserved row)))

(defn scope-escalation-run-ref [value]
  (when-let [run (normalized-token value)]
    (if (str/starts-with? run "@") run (str "@" run))))

(defn checkpointed-scope-escalation? [facts row]
  (boolean
   (some
    (fn [raw]
      (let [payload (json-map raw)]
        (and (= "north.scope-escalation/v1" (get payload "schema"))
             (= "scope-overrun" (get payload "kind"))
             (= "needs-replan" (get payload "disposition"))
             (= (:entity row) (scope-escalation-run-ref (get payload "run"))))))
    (many facts (:thread row) "scope_escalation"))))

(defn lane-terminal-row [facts entity]
  (let [lane-facts (get facts entity {})
        process (north.terminal-projection/terminal-process-outcome lane-facts)
        delivery (north.terminal-projection/terminal-delivery-outcome lane-facts)
        reason (when process
                 (north.terminal-projection/singleton-value lane-facts "delivery_reason"))]
    (when (and process delivery reason)
      {:entity entity
       :agent (subs entity (count "@agent:"))
       :at (or (normalized-token (one facts entity "at"))
               (normalized-token (one facts entity "spawned_at")))
       :processOutcome process
       :processOutcomeObserved true
       :deliveryOutcome delivery
       :deliveryOutcomeObserved true
       :deliveryReason reason
       :deliveryReasonObserved true
       :preflightCause (normalized-token (one facts entity "preflight_cause"))
       :retryOfRun (normalized-token (one facts entity "retry_of_run"))
       :retryAttempt (maybe-long (one facts entity "retry_attempt"))
       :tokens nil})))

(defn waste-attempt-rows
  "Complete terminal attempts for managed lanes. Committed run rows retain the
  normal report projection. A digest-valid lane terminal fills the crash seam
  where the run writer never reached its kind=run commit marker."
  [facts rows]
  (let [managed-runs
        (->> rows
             (filter (fn [row]
                       (and (terminal-row? row)
                            (= "lane" (one facts (str "@agent:" (:agent row)) "kind")))))
             (mapv #(assoc % :checkpointedEscalation
                           (checkpointed-scope-escalation? facts %)))
             vec)
        committed-agents (set (keep :agent managed-runs))
        terminal-only
        (->> facts
             (keep (fn [[entity _]]
                     (when (and (str/starts-with? entity "@agent:")
                                (= "lane" (one facts entity "kind"))
                                (not (contains? committed-agents
                                                (subs entity (count "@agent:")))))
                       (lane-terminal-row facts entity))))
             vec)]
    (into managed-runs terminal-only)))

(defn checkpointed-escalation? [row]
  (or (:checkpointedEscalation row)
      (checkpointed-escalation-outcomes (:processOutcome row))
      (checkpointed-escalation-reasons (:deliveryReason row))))

(defn row-cause-text [row]
  (->> [(:processOutcome row) (:deliveryReason row) (:preflightCause row)]
       (keep normalized-token)
       (str/join " ")
       str/lower-case))

(defn startup-ack-timeout? [row]
  (let [cause (row-cause-text row)]
    (or (str/includes? cause "startup_ack_timeout")
        (str/includes? cause "startup-ack timeout")
        (boolean
         (re-find #"startup[-_ ]+(?:ack|acknowledg)[a-z_-]*[-_ ]+(?:timeout|timed[-_ ]*out)"
                  cause)))))

(defn reservation-transport-failure? [row]
  (let [reason (:deliveryReason row)
        cause (row-cause-text row)]
    (or (reservation-transport-reasons reason)
        (boolean
         (re-find #"(?:reservation|transport).{0,80}(?:fail|unavailable|timeout|timed[-_ ]*out|unverified)"
                  cause)))))

(defn terminal-machinery-bucket [row]
  (when-not (checkpointed-escalation? row)
    (cond
      (or (= "died-unreported" (:processOutcome row))
          (= "presence_lapsed_without_committed_terminal" (:deliveryReason row)))
      "died-unreported"

      (and (= "blocked" (:deliveryOutcome row))
           (or (= "died" (:processOutcome row))
               (= "provider_process_died" (:deliveryReason row))))
      "zero-delivery-provider-death"

      (startup-ack-timeout? row) "startup-ack-timeout"
      (reservation-transport-failure? row) "reservation-transport"

      (or (= "blocked_preflight" (:processOutcome row))
          (= "execution_preflight_blocked" (:deliveryReason row)))
      "blocked-preflight"

      :else nil)))

(defn machinery-buckets [rows]
  (let [terminal-by-run (into {} (map (juxt :entity terminal-machinery-bucket) rows))]
    (into {}
          (map (fn [row]
                 [(:entity row)
                  (or (terminal-machinery-bucket row)
                      (when (some-> (:retryOfRun row) terminal-by-run)
                        "retry-duplicate"))]))
          rows)))

(defn trailing-waste-window [rows]
  (let [dated (->> rows
                   (keep (fn [row]
                           (when-let [instant (parse-instant (:at row))]
                             (assoc row ::instant instant))))
                   (sort (fn [left right]
                           (let [time-order (compare (::instant right) (::instant left))]
                             (if (zero? time-order)
                               (compare (:entity right) (:entity left))
                               time-order))))
                   vec)]
    {:undatedRuns (- (count rows) (count dated))
     :rows
     (loop [remaining dated selected [] exact-total 0]
       (if (or (empty? remaining) (>= exact-total waste-window-token-limit))
         (mapv #(dissoc % ::instant) selected)
         (let [row (first remaining)]
           (recur (rest remaining)
                  (conj selected row)
                  (+ exact-total (or (exact-run-tokens row) 0))))))}))

(defn waste-verdict [runs coverage-percent ratio-percent]
  (cond
    (< runs waste-minimum-runs) "insufficient runs"
    (< coverage-percent waste-minimum-exact-coverage-percent) "FAIL"
    (<= ratio-percent 10.0) "PASS"
    (<= ratio-percent 20.0) "PROBATION"
    :else "FAIL"))

(defn waste-report [rows]
  (let [{window :rows undated-runs :undatedRuns} (trailing-waste-window rows)
        bucket-by-run (machinery-buckets rows)
        window (mapv #(assoc % :wasteBucket (get bucket-by-run (:entity %))) window)
        exact-rows (filterv #(some? (exact-run-tokens %)) window)
        unknown-rows (filterv #(nil? (exact-run-tokens %)) window)
        exact-runs (count exact-rows)
        unknown-runs (count unknown-rows)
        run-count (count window)
        exact-token-total (reduce + 0 (keep exact-run-tokens exact-rows))
        mean-exact-tokens (if (pos? exact-runs)
                            (/ (double exact-token-total) exact-runs)
                            1.0)
        unknown-token-weight (max 1.0 mean-exact-tokens)
        unknown-gating-tokens (* unknown-runs unknown-token-weight)
        window-token-total (+ exact-token-total unknown-gating-tokens)
        exact-machinery-tokens
        (reduce + 0 (keep (fn [row]
                            (when (:wasteBucket row) (exact-run-tokens row)))
                          exact-rows))
        wasted-tokens (+ exact-machinery-tokens unknown-gating-tokens)
        waste-runs (count (filter #(or (:wasteBucket %)
                                       (nil? (exact-run-tokens %)))
                                  window))
        ratio-percent (cond
                        (pos? window-token-total)
                        (* 100.0 (/ wasted-tokens window-token-total))
                        (pos? waste-runs) 100.0
                        :else 0.0)
        coverage-percent (if (pos? run-count)
                           (* 100.0 (/ exact-runs run-count))
                           0.0)
        bucket-breakdown
        (mapv (fn [bucket]
                (let [bucket-rows (filterv #(= bucket (:wasteBucket %)) window)
                      exact (vec (keep exact-run-tokens bucket-rows))
                      unknown (- (count bucket-rows) (count exact))]
                  {:bucket bucket
                   :runs (count bucket-rows)
                   :exactTokenRuns (count exact)
                   :unknownTokenRuns unknown
                   :exactTokens (reduce + 0 exact)
                   :gatingWasteTokens (+ (reduce + 0 exact)
                                         (* unknown unknown-token-weight))}))
              waste-bucket-order)
        verdict (waste-verdict run-count coverage-percent ratio-percent)]
    {:report "waste"
     :claim (str "machinery-wasted tokens / total managed-lane tokens over the trailing "
                 "1,000,000-token whole-run window; unknown-token runs are gating waste "
                 "at the window's mean exact run weight (minimum one token), and exact "
                 "coverage must be at least 90%")
     :windowTokenLimit waste-window-token-limit
     :windowTokenTotal window-token-total
     :exactObservedTokens exact-token-total
     :runCount run-count
     :availableTerminalRuns (count rows)
     :excludedUndatedRuns undated-runs
     :wasteRatioPercent ratio-percent
     :machineryWastedTokens wasted-tokens
     :wasteRuns waste-runs
     :buckets bucket-breakdown
     :unknownCoverage {:runs unknown-runs
                       :gatingWasteTokens unknown-gating-tokens
                       :exactRuns exact-runs
                       :totalRuns run-count
                       :exactCoveragePercent coverage-percent
                       :requiredExactCoveragePercent
                       waste-minimum-exact-coverage-percent}
     :minimumRuns waste-minimum-runs
     :verdict verdict}))

(defn jsonl-files [root child]
  (let [dir (io/file root child)]
    (if-not (.isDirectory dir) []
      (->> (file-seq dir)
           (filter #(and (.isFile %) (str/ends-with? (.getName %) ".jsonl")))
           (sort-by #(.getPath %))))))

(defn parse-json-line [line]
  (try (json/parse-string line) (catch Exception _ nil)))

(defn earlier-candidate [current candidate]
  (if (or (nil? current)
          (.isBefore (parse-instant (:at candidate)) (parse-instant (:at current))))
    candidate current))

(defn event-turn-id [event]
  (or (normalized-token (get-in event ["payload" "turn_id"]))
      (normalized-token
       (get-in event ["payload" "internal_chat_message_metadata_passthrough" "turn_id"]))))

(defn scan-openai-file [state file]
  (with-open [reader (io/reader file)]
    (let [[state _ _]
          (reduce
      (fn [[state current-turn session-ids] line]
        (if-let [event (parse-json-line line)]
          (let [turn (or (event-turn-id event) current-turn)
                payload (get event "payload")
                session-ids
                (if (= "session_meta" (get event "type"))
                  (into session-ids
                        (keep normalized-token
                              [(get payload "id") (get payload "session_id")]))
                  session-ids)
                state (if (and (= "turn_context" (get event "type")) turn)
                        (assoc-in state [:turnMetadata turn]
                                  {:model (normalized-token (get payload "model"))
                                   :effort (or (normalized-token (get payload "effort"))
                                               (normalized-token (get payload "reasoning_effort")))})
                        state)
                last-usage (get-in payload ["info" "last_token_usage"])
                cumulative (maybe-long
                            (get-in payload ["info" "total_token_usage" "total_tokens"]))
                tokens (maybe-long (get last-usage "total_tokens"))
                at (normalized-token (get event "timestamp"))]
            (if (and (= "token_count" (get payload "type"))
                     cumulative tokens (not (neg? tokens)) (parse-instant at))
              (let [key [(or turn (str "file:" (.getPath file))) cumulative]
                    candidate {:turn turn :at at :tokens tokens
                               :providerSessionIds (vec (sort session-ids))
                               :dedupKeyHasTurn (boolean turn)}]
                [(update-in state [:candidates key] earlier-candidate candidate)
                 turn session-ids])
              [state turn session-ids]))
          [state current-turn session-ids]))
      [state nil #{}] (line-seq reader))]
      state)))

(defn openai-account-records [{:keys [providerTarget root]}]
  (let [state (reduce scan-openai-file {:turnMetadata {} :candidates {}}
                      (jsonl-files root "sessions"))]
    (->> (vals (:candidates state))
         (map (fn [candidate]
                (let [metadata (get-in state [:turnMetadata (:turn candidate)])]
                  {:providerTarget providerTarget :provider "openai"
                   :model (or (:model metadata) "unattributed")
                   :effort (or (:effort metadata) "unobserved")
                   :at (:at candidate) :tokens (:tokens candidate)
                   :providerSessionIds (:providerSessionIds candidate)
                   :providerTurnId (:turn candidate)
                   :source "codex-account-jsonl:last-token-usage"
                   :deduplication "turn-id+cumulative-total-earliest-timestamp"
                   :dedupKeyHasTurn (:dedupKeyHasTurn candidate)})))
         vec)))

(defn anthropic-message-candidate [event]
  (let [message (get event "message")
        usage (get message "usage")
        id (normalized-token (get message "id"))
        at (normalized-token (get event "timestamp"))
        components (map #(maybe-long (get usage %))
                        ["input_tokens" "cache_creation_input_tokens"
                         "cache_read_input_tokens" "output_tokens"])]
    (when (and id (parse-instant at) (every? some? components)
               (every? #(not (neg? %)) components))
      {:messageId id :at at :tokens (reduce + components)
       :sessionId (normalized-token (get event "sessionId"))
       :model (or (normalized-token (get message "model")) "unattributed")})))

(defn anthropic-account-records [{:keys [providerTarget root]}]
  (let [deduped
        (reduce
         (fn [records file]
           (with-open [reader (io/reader file)]
             (reduce (fn [records line]
                       (if-let [candidate (some-> line parse-json-line
                                                  anthropic-message-candidate)]
                         (update records (:messageId candidate)
                                 earlier-candidate candidate)
                         records))
                     records (line-seq reader))))
         {} (jsonl-files root "projects"))]
    (mapv (fn [candidate]
            {:providerTarget providerTarget :provider "anthropic"
             :model (:model candidate) :effort nil
             :at (:at candidate) :tokens (:tokens candidate)
             :providerSessionIds (vec (remove nil? [(:sessionId candidate)]))
             :providerTurnId (:messageId candidate)
             :source "claude-account-jsonl:message-usage"
             :deduplication "message-id-earliest-timestamp"})
          (vals deduped))))

(defn attach-provider-join-keys [records]
  (let [records (vec records)
        session-refs (vec (mapcat (fn [[index record]]
                                    (map (fn [id] [index id])
                                         (:providerSessionIds record)))
                                  (map-indexed vector records)))
        turn-refs (vec (keep (fn [[index record]]
                               (when-let [id (:providerTurnId record)]
                                 [index (:provider record) id]))
                             (map-indexed vector records)))
        request {:sessions (mapv second session-refs)
                 :turns (mapv (fn [[_ provider id]]
                                {:provider provider :id id}) turn-refs)}]
    (try
      (let [result (proc/shell {:in (json/generate-string request)
                                :out :string :err :string}
                               "bun" (str NORTH "/sdk/src/providers/provider-join.ts"))
            response (json/parse-string (str/trim (:out result)) true)]
        (when-not (and (zero? (:exit result))
                       (= "north-provider-join:v1" (:version response))
                       (= (count session-refs) (count (:sessions response)))
                       (= (count turn-refs) (count (:turns response))))
          (throw (ex-info "provider join-key helper returned an invalid batch" {})))
        (let [session-keys (reduce (fn [by-record [[index _] key]]
                                     (update by-record index (fnil conj #{}) key))
                                   {} (map vector session-refs (:sessions response)))
              turn-keys (into {} (map (fn [[[index _ _] key]] [index key])
                                      (map vector turn-refs (:turns response))))]
          (mapv (fn [index record]
                  (cond-> (dissoc record :providerSessionIds :providerTurnId)
                    true (assoc :providerJoinKeyVersion "north-provider-join:v1"
                                :providerSessionKeys
                                (vec (sort (get session-keys index #{}))))
                    (get turn-keys index) (assoc :providerTurnKey (get turn-keys index))))
                (range) records)))
      (catch Exception _
        (mapv #(-> %
                   (dissoc :providerSessionIds :providerTurnId)
                   (assoc :providerJoinKeyVersion "unavailable"
                          :providerSessionKeys []))
              records)))))

(defn account-log-records []
  (attach-provider-join-keys
   (mapcat (fn [target]
             (case (:provider target)
               "openai" (openai-account-records target)
               "anthropic" (anthropic-account-records target)
               []))
           (configured-account-log-targets))))

(defn parse-hours [value option]
  (let [match (re-matches #"(?i)([0-9]+(?:\.[0-9]+)?)h" (or value ""))
        hours (when match (parse-double (second match)))]
    (when-not (and hours (pos? hours) (Double/isFinite hours))
      (throw (ex-info (str option " expects a positive duration such as 24h") {})))
    hours))

(defn duration-of-hours [hours]
  (java.time.Duration/ofMillis (long (Math/round (* hours 60.0 60.0 1000.0)))))

(defn row-in-interval? [row start end]
  (when-let [at (parse-instant (:at row))]
    (and (not (.isBefore at start)) (.isBefore at end))))

(defn percent [numerator denominator]
  (when (and (some? numerator) (some? denominator) (pos? denominator))
    (/ (double (* 100 numerator)) (double denominator))))

(defn account-breakdown-row [account [[model effort] rows] account-tokens]
  (let [stats (usage-stats rows)
        exact-runs (get-in stats [:tokenCoverage :exactRuns])]
    {:providerTarget (:providerTarget account)
     :provider (:provider account)
     :model model
     :effort effort
     :terminalRuns (:runs stats)
     :exactTokenRuns exact-runs
     :unknownTokenRuns (- (:runs stats) exact-runs)
     :exactObservedTokens (:tokens stats)
     :percentageOfAccountExactObservedTokens (percent (:tokens stats) account-tokens)}))

(defn account-usage-row [account rows]
  (let [stats (usage-stats rows)
        exact-runs (get-in stats [:tokenCoverage :exactRuns])
        account-tokens (:tokens stats)]
    (assoc account
           :terminalRuns (:runs stats)
           :exactTokenRuns exact-runs
           :unknownTokenRuns (- (:runs stats) exact-runs)
           ;; nil means no exact observation. An exact observed zero remains 0.
           :exactObservedTokens account-tokens
           :tokenEvidence (:tokenEvidence stats)
           :breakdown (->> rows
                           (group-by (juxt :model :effort))
                           (map #(account-breakdown-row account % account-tokens))
                           (sort-by (juxt :model :effort)) vec))))

(defn account-universe [rows]
  (let [configured (configured-targets)
        configured-ids (set (map :providerTarget configured))
        used (->> rows
                  (group-by :providerTarget)
                  (map (fn [[target target-rows]]
                         {:providerTarget target
                          :provider (:provider (first target-rows))
                          :configuredNow false}))
                  (remove #(configured-ids (:providerTarget %))))]
    (vec (concat configured (sort-by (juxt :provider :providerTarget) used)))))

(defn interval-usage [rows accounts start end]
  (let [selected (vec (filter #(row-in-interval? % start end) rows))
        by-target (group-by :providerTarget selected)
        exact-rows (filter #(some? (:tokens %)) selected)]
    {:start (.toString start)
     :end (.toString end)
     :boundary "start-inclusive,end-exclusive"
     :terminalRuns (count selected)
     :exactTokenRuns (count exact-rows)
     :unknownTokenRuns (- (count selected) (count exact-rows))
     :exactObservedTokens (when (seq exact-rows) (reduce + (map :tokens exact-rows)))
     :accounts (mapv #(account-usage-row % (get by-target (:providerTarget %) [])) accounts)}))

(defn native-session-group [[[provider model effort] sessions]]
  {:provider provider
   :providerTarget nil
   :model model
   :effort effort
   :sessions (count sessions)
   :exactObservedTokens nil
   :tokenEvidence "unobserved"
   :accountAttribution "unobserved"
   :percentageOfAccountExactObservedTokens nil})

(defn native-session-activity [sessions start end]
  (let [selected (filter (fn [session]
                           (when-let [at (parse-instant (:startedAt session))]
                             (and (not (.isBefore at start)) (.isBefore at end))))
                         sessions)]
    {:scope "provider-native-interactive-sessions"
     :sessions (count selected)
     :providerTarget nil
     :exactObservedTokens nil
     :tokenEvidence "unobserved"
     :accountAttribution "unobserved"
     :includedInManagedRunPercentages false
     :groups (->> selected
                  (group-by (juxt :provider :model :effort))
                  (map native-session-group)
                  (sort-by (juxt :provider :model :effort)) vec)}))

(defn exact-token-sum [rows]
  (when (seq rows) (reduce + (map :tokens rows))))

(defn index-many [rows keys-fn]
  (reduce (fn [index row]
            (reduce (fn [index key]
                      (update index key (fnil conj []) row))
                    index (keys-fn row)))
          {} rows))

(defn unique-row [rows]
  (let [rows (vec (vals (into {} (map (juxt :entity identity) rows))))]
    (when (= 1 (count rows)) (first rows))))

(defn managed-provider-attribution [run status]
  {:joinStatus status
   :northKind "run"
   :northEntity (:entity run)
   :dispatchMode "north"
   :account (:providerTarget run)
   :provider (:provider run)
   :model (:model run)
   :requestedModel (:requestedModel run)
   :requestedEffort (:requestedEffort run)
   :resolvedEffort (:effort run)
   :routingReceiptStatus (if (:routingReceipt run) "observed" "legacy-unknown")
   :routingReceipt (:routingReceipt run)})

(defn native-provider-attribution [session]
  {:joinStatus "matched-native-session"
   :northKind "native-session"
   :northEntity (:entity session)
   :dispatchMode (:dispatchModeAtStart session)
   :account "provider-log-observed"
   :provider (:provider session)
   :model (:model session)
   :requestedModel nil
   :requestedEffort nil
   ;; Native identity is a mutable final session snapshot, not exact per-turn
   ;; effort. Preserve it separately and keep resolvedEffort unknown.
   :resolvedEffort nil
   :sessionEffortSnapshot (:effort session)
   :routingReceiptStatus "not-applicable-provider-native"
   :routingReceipt nil})

(defn attribute-provider-records [records managed native]
  (let [managed-turn
        (index-many managed
                    (fn [run]
                      (map (fn [key] [(:provider run) (:providerTarget run) key])
                           (:providerTurnKeys run))))
        managed-session
        (index-many managed
                    (fn [run]
                      (when-let [key (:providerSessionKey run)]
                        [[(:provider run) (:providerTarget run) key]])))
        native-session
        (index-many native
                    (fn [session]
                      (when-let [key (:providerSessionKey session)]
                        [[(:provider session) key]])))]
    (mapv
     (fn [record]
       (let [turn-candidates
             (get managed-turn [(:provider record) (:providerTarget record)
                                (:providerTurnKey record)] [])
             session-candidates
             (mapcat #(get managed-session
                           [(:provider record) (:providerTarget record) %] [])
                     (:providerSessionKeys record))
             native-candidates
             (mapcat #(get native-session [(:provider record) %] [])
                     (:providerSessionKeys record))
             turn-run (unique-row turn-candidates)
             session-run (when (empty? turn-candidates)
                           (unique-row session-candidates))
             native-row (when (and (empty? turn-candidates)
                                   (empty? session-candidates))
                          (unique-row native-candidates))
             selected-candidates (cond
                                   (seq turn-candidates) turn-candidates
                                   (seq session-candidates) session-candidates
                                   :else native-candidates)
             ambiguous? (> (count (set (map :entity selected-candidates))) 1)]
         (cond
           ambiguous? (assoc record :joinStatus "ambiguous"
                             :northAttribution nil)
           turn-run (assoc record :joinStatus "matched-managed-turn"
                           :northAttribution
                           (managed-provider-attribution turn-run "matched-managed-turn"))
           session-run (assoc record :joinStatus "matched-managed-session"
                              :northAttribution
                              (managed-provider-attribution session-run
                                                            "matched-managed-session"))
           native-row (assoc record :joinStatus "matched-native-session"
                             :northAttribution (native-provider-attribution native-row))
           (= "unavailable" (:providerJoinKeyVersion record))
           (assoc record :joinStatus "join-key-unavailable" :northAttribution nil)
           :else (assoc record :joinStatus "unmatched" :northAttribution nil))))
     records)))

(defn account-observed-breakdown [records account-total]
  (->> records
       (group-by (fn [record]
                   [(:model record)
                    (or (:effort record)
                        (get-in record [:northAttribution :resolvedEffort]))]))
       (map (fn [[[model effort] grouped]]
              (let [tokens (exact-token-sum grouped)]
                {:model model :effort effort
                 :observations (count grouped)
                 :exactObservedTokens tokens
                 :percentageOfProviderOwnedAccountExactObservedTokens
                 (percent tokens account-total)
                 :percentageBasis "provider-owned-account-observed-tokens"
                 :joinStatuses (frequencies (map :joinStatus grouped))
                 :northAttributions (vec (keep :northAttribution grouped))})))
       (sort-by (juxt :model #(or (:effort %) ""))) vec))

(defn account-observed-row [account persisted managed start end]
  (let [target (:providerTarget account)
        provider (:provider account)
        persisted-target (filterv #(= target (:providerTarget %)) persisted)
        persisted-selected (vec (filter #(and (= target (:providerTarget %))
                                              (row-in-interval? % start end))
                                        persisted))
        managed-selected (vec (filter #(and (= target (:providerTarget %))
                                            (row-in-interval? % start end))
                                      managed))
        managed-exact (filterv #(some? (:tokens %)) managed-selected)
        managed-unknown (- (count managed-selected) (count managed-exact))
        provider-owned-total (exact-token-sum persisted-selected)
        provider-source (if (= provider "openai")
                          {:source "codex-account-jsonl:last-token-usage"
                           :observations (count persisted-selected)
                           :turnAttributedObservations
                           (count (filter :dedupKeyHasTurn persisted-selected))
                           :fallbackDedupObservations
                           (count (remove :dedupKeyHasTurn persisted-selected))
                           :exactObservedTokens provider-owned-total
                           :joinStatuses (frequencies (map :joinStatus persisted-selected))
                           :northAttributions
                           (vec (keep :northAttribution persisted-selected))}
                          {:source "claude-account-jsonl:message-usage"
                           :observations (count persisted-selected)
                           :exactObservedTokens provider-owned-total
                           :joinStatuses (frequencies (map :joinStatus persisted-selected))
                           :northAttributions
                           (vec (keep :northAttribution persisted-selected))})
        managed-total (exact-token-sum managed-exact)
        matched-runs (set (keep #(when (= "run" (get-in % [:northAttribution :northKind]))
                                   (get-in % [:northAttribution :northEntity]))
                                persisted-target))
        persisted-turn-keys (set (keep :providerTurnKey persisted-target))
        provider-terminal-runs
        (filterv #(not= "pre-provider" (:turnProvenance %)) managed-selected)
        ephemeral-exclusive
        (filterv (fn [run]
                   (and (= "ephemeral" (:providerSessionPersistence run))
                        (= "north-provider-join:v1" (:providerJoinKeyVersion run))
                        (seq (:providerTurnKeys run))
                        (empty? (filter persisted-turn-keys (:providerTurnKeys run)))))
                 provider-terminal-runs)
        ephemeral-entities (set (map :entity ephemeral-exclusive))
        unresolved (filterv #(and (not (matched-runs (:entity %)))
                                  (not (ephemeral-entities (:entity %))))
                            provider-terminal-runs)
        exclusive-exact (filterv #(some? (:tokens %)) ephemeral-exclusive)
        exclusive-total (exact-token-sum exclusive-exact)
        active? (or (seq persisted-selected) (seq provider-terminal-runs))
        combination-status (cond
                             (not active?) "no-activity"
                             (seq unresolved) "legacy-partial-or-ambiguous"
                             :else "exact-no-double-count")
        combined-total (when (= "exact-no-double-count" combination-status)
                         (+ (or provider-owned-total 0) (or exclusive-total 0)))
        overlap-status (cond
                         (seq unresolved) "unresolved"
                         (seq ephemeral-exclusive) "joined-plus-explicit-ephemeral"
                         (seq matched-runs) "joined"
                         :else "no-overlap-observed")]
    {:providerTarget target :provider provider
     :exactObservedTokens provider-owned-total
     :providerOwnedExactObservedTokens provider-owned-total
     :tokenEvidence (if (some? provider-owned-total) "observed-lower-bound" "unobserved")
     :overlapStatus overlap-status
     :overlapReason
     (if (seq unresolved)
       "historical, partial, or ambiguous North rows lack a unique provider-owned join"
       "provider-owned matches are canonical; only explicitly ephemeral unmatched runs are added once")
     :combinationStatus combination-status
     :combinedExactObservedTokens combined-total
     :combinedPercentageBasis
     (when (some? combined-total) "provider-owned-plus-exclusive-ephemeral-managed")
     :combinationSemantics
     "matched provider-owned turn is canonical; its North terminal is never added again"
     :sources [provider-source]
     :managedLedger {:source "north-managed-terminal"
                     :exactTokenRuns (count managed-exact)
                     :unknownTokenRuns managed-unknown
                     :exactObservedTokens managed-total
                     :tokenEvidence (cond
                                      (empty? managed-exact) "unobserved"
                                      (zero? managed-unknown) "exact"
                                      :else "lower-bound")
                     :breakdown (:breakdown (account-usage-row account managed-selected))}
     :joinCoverage {:matchedRuns (count matched-runs)
                    :exclusiveEphemeralRuns (count ephemeral-exclusive)
                    :unresolvedRuns (count unresolved)}
     :exclusiveManagedLedger
     {:source "north-managed-explicit-ephemeral"
      :exactTokenRuns (count exclusive-exact)
      :unknownTokenRuns (- (count ephemeral-exclusive) (count exclusive-exact))
      :exactObservedTokens exclusive-total}
     :breakdown (account-observed-breakdown persisted-selected provider-owned-total)}))

(defn account-observed-usage [persisted managed accounts start end]
  (let [account-rows (mapv #(account-observed-row % persisted managed start end) accounts)
        observed (filter #(some? (:exactObservedTokens %)) account-rows)
        active (filter #(not= "no-activity" (:combinationStatus %)) account-rows)
        combined-exact? (every? #(= "exact-no-double-count" (:combinationStatus %)) active)
        combined (when (and (seq active) combined-exact?)
                   (reduce + (map #(or (:combinedExactObservedTokens %) 0) active)))]
    {:scope "account-observed-provider-logs"
     :claim (str "provider-owned matched turns are canonical and North terminals are not added "
                 "again; exact unmatched terminals contribute only with positive ephemeral-session "
                 "evidence; legacy, partial, and ambiguous joins remain explicit unknowns")
     :exactObservedTokens (when (seq observed)
                            (reduce + (map :exactObservedTokens observed)))
     :providerOwnedExactObservedTokens (when (seq observed)
                                         (reduce + (map :exactObservedTokens observed)))
     :combinedExactObservedTokens combined
     :combinationStatus (cond
                          (empty? active) "no-activity"
                          combined-exact? "exact-no-double-count"
                          :else "legacy-partial-or-ambiguous")
     :overlapStatus "see-account-ledgers"
     :tokenEvidence (if (seq observed) "observed-lower-bound" "unobserved")
     :accounts account-rows}))

(defn operational-telemetry [rows]
  (let [unknown "legacy-unknown"
        provenance
        (fn [row]
          {:strategyId (or (:responseStrategyId row) unknown)
           :implementation (or (:responseStrategyImplementation row) unknown)
           :version (or (:responseStrategyVersion row) unknown)})
        provenance-status
        (fn [row]
          (let [value (provenance row)]
            (cond
              (= unknown (:strategyId value)) "legacy-unknown"
              (= "none" (:strategyId value))
              (if (and (= "disabled" (:implementation value))
                       (not= unknown (:requestedMode value))
                       (not= unknown (:mode value))
                       (not= unknown (:source value))
                       (not= unknown (:decisionReason value))
                       (not= unknown (:measurementCoverage value)))
                "complete" "partial")
              (every? #(not= unknown (get value %))
                      [:implementation :version :requestedMode :mode :source :decisionReason
                       :measurementCoverage])
              "complete"
              :else "partial")))
        group-key (fn [row] [(or (:provider row) "unattributed")
                             (or (:providerTarget row) "unattributed")
                             (or (:model row) "unattributed")])
        summarize
        (fn [cohort]
          (let [known-calls (filter #(some? (:mcpActualCalls %)) cohort)
                tools (mapcat :mcpActualTools cohort)]
            {:runs (count cohort)
             :responseStrategyCounts
             (->> cohort (map #(or (:responseStrategyId %) "legacy-unknown"))
                  frequencies (into (sorted-map)))
             :responseStrategyImplementationCounts
             (->> cohort (map #(or (:responseStrategyImplementation %) unknown))
                  frequencies (into (sorted-map)))
             :responseStrategyVersionCounts
             (->> cohort (map #(or (:responseStrategyVersion %) unknown))
                  frequencies (into (sorted-map)))
             :responseStrategyProvenanceCoverageCounts
             (->> cohort (map provenance-status) frequencies (into (sorted-map)))
             :responseStrategyProvenance
             (->> cohort (map provenance) frequencies
                  (map (fn [[value runs]] (assoc value :runs runs)))
                  (sort-by (juxt :strategyId :implementation :version :mode :source)) vec)
             :mcpCoverageCounts
             (->> cohort (map #(or (:mcpActivityCoverage %) "legacy-unknown"))
                  frequencies (into (sorted-map)))
             :mcpKnownCallRuns (count known-calls)
             :mcpActualCalls (when (seq known-calls) (reduce + (map :mcpActualCalls known-calls)))
             :mcpToolDistribution
             (->> tools
                  (group-by (juxt :server :tool))
                  (map (fn [[[server tool] entries]]
                         {:server server :tool tool :calls (reduce + (map :count entries))}))
                  (sort-by (juxt :server :tool)) vec)}))]
    {:scope "managed-provider-actuals"
     :claim (str "MCP totals count completed structured provider observations only; grants, native "
                 "tools, arguments, outputs, and unknown legacy activity are excluded")
     :coverage (summarize rows)
     :byProviderAccountModel
     (->> rows (group-by group-key)
          (map (fn [[[provider account model] cohort]]
                 (assoc (summarize cohort)
                        :provider provider :account account :model model)))
          (sort-by (juxt :provider :account :model)) vec)}))

(defn interval-report [rows sessions persisted accounts start end]
  (let [bounded (vec (filter #(row-in-interval? % start end) rows))]
   (assoc (interval-usage rows accounts start end)
         :usageScope "managed-terminal-runs-only"
         :nativeInteractiveActivity (native-session-activity sessions start end)
         :accountObserved (account-observed-usage persisted rows accounts start end)
         :operationalTelemetry (operational-telemetry bounded))))

(defn windowed-usage-report [rows sessions {:keys [window-hours slice-hours now]}]
  (let [end (or (parse-instant now)
                (when now (throw (ex-info "--now expects an ISO-8601 instant" {})))
                (java.time.Instant/now))
        window-duration (duration-of-hours window-hours)
        slice-duration (duration-of-hours slice-hours)
        window-ms (.toMillis window-duration)
        slice-ms (.toMillis slice-duration)]
    (when (or (> slice-ms window-ms) (not (zero? (mod window-ms slice-ms))))
      (throw (ex-info "--window must be an exact multiple of --slice" {})))
    (let [start (.minus end window-duration)
          window-rows (vec (filter #(row-in-interval? % start end) rows))
          accounts (account-universe window-rows)
          configured-accounts (vec (filter :configuredNow accounts))
          provider-families (set (map :provider configured-accounts))
          persisted (attribute-provider-records (vec (account-log-records)) rows sessions)
          intervals (mapv (fn [index]
                            (let [interval-start (.plusMillis start (* index slice-ms))
                                  interval-end (.plusMillis interval-start slice-ms)]
                              (assoc (interval-report rows sessions persisted accounts interval-start interval-end)
                                     :index (inc index))))
                          (range (quot window-ms slice-ms)))
          dated (count (filter #(parse-instant (:at %)) rows))]
      {:report "usage"
       :scope "bounded-intervals"
       :unit "exact observed tokens, never dollars or API credits"
       :claim (str "provider-owned turns are canonical when joined to North runs/native sessions; "
                   "managed terminal observations remain lower bounds on subscription consumption, "
                   "and unmatched historical identity remains unknown")
       :accountScope
       {:accountTargets (count configured-accounts)
        :providerFamilies (count provider-families)
        :label (format "%d account targets across %d provider families"
                       (count configured-accounts) (count provider-families))
        :targets (mapv :providerTarget configured-accounts)
        :families (vec (sort provider-families))}
       :window {:start (.toString start) :end (.toString end)
                :hours window-hours :sliceHours slice-hours
                :boundary "start-inclusive,end-exclusive"}
       :reproducibility
       {:boundaryBasis "provider-event-time"
        :fixedWindowRerunStable false
        :caveat (str "late-appended or backfilled provider events can change a rerun even when "
                     "--now and event-time boundaries are identical")}
       :timeCoverage {:datedRuns dated :undatedRuns (- (count rows) dated)}
       :intervals intervals
       :cumulative (interval-report rows sessions persisted accounts start end)})))

(def tier-rank {"economy" 0 "standard" 1 "senior" 2 "frontier" 3})
(def reasoning-rank {"low" 0 "medium" 1 "high" 2 "xhigh" 3 "max" 4 "ultra" 5})
(def economics-thresholds
  {:premiumTokenSharePercent 75.0
   :promotionSharePercent 25.0
   :pinSharePercent 25.0
   :assessmentCoveragePercent 80.0})
(def economics-design-gates
  {:minimumEligibleRuns 10
   :minimumEvidenceCoveragePercent 80.0})

(defn current-dispatch-mode []
  (north.harness-state/get-value (System/getProperty "user.home") "dispatch" "north"))

(defn assessed-promotion? [row]
  (let [derived-tier (tier-rank (:routingDerivedTier row))
        selected-tier (tier-rank (:routingSelectedTier row))
        derived-reasoning (reasoning-rank (:routingDerivedReasoning row))
        selected-reasoning (reasoning-rank (:routingSelectedReasoning row))]
    (and (= "recorded" (:routingAssessmentStatus row))
         (or (and derived-tier selected-tier (> selected-tier derived-tier))
             (and derived-reasoning selected-reasoning
                  (> selected-reasoning derived-reasoning))))))

(defn alert [code observed threshold]
  {:code code :severity "warning" :status "alert" :policy "alert-only"
   :observed observed :threshold threshold})

(defn insufficient-finding [code eligible coverage]
  {:code code :severity "info" :status "cannot-determine"
   :reason "insufficient-coverage" :policy "alert-only"
   :eligibleRuns eligible :evidenceCoveragePercent coverage
   :minimumEligibleRuns (:minimumEligibleRuns economics-design-gates)
   :minimumEvidenceCoveragePercent
   (:minimumEvidenceCoveragePercent economics-design-gates)})

(defn gated-ratio-finding [alert-code insufficient-code observed threshold eligible coverage]
  (cond
    (or (< eligible (:minimumEligibleRuns economics-design-gates))
        (nil? coverage)
        (< coverage (:minimumEvidenceCoveragePercent economics-design-gates)))
    (insufficient-finding insufficient-code eligible coverage)

    (and (some? observed) (>= observed threshold))
    (assoc (alert alert-code observed threshold)
           :eligibleRuns eligible :evidenceCoveragePercent coverage)

    :else nil))

(defn openai-provider-owned-effort [usage]
  (let [accounts (filter #(= "openai" (:provider %))
                         (get-in usage [:accountObserved :accounts]))
        rows (vec (mapcat :breakdown accounts))
        all-tokens (reduce + 0 (keep :exactObservedTokens accounts))
        known (vec (filter #(contains? reasoning-rank (:effort %)) rows))
        known-tokens (reduce + 0 (keep :exactObservedTokens known))
        known-observations (reduce + 0 (map :observations known))
        high (filter #(>= (get reasoning-rank (:effort %) -1) 2) known)
        ultra (filter #(= "ultra" (:effort %)) known)
        high-tokens (reduce + 0 (keep :exactObservedTokens high))
        ultra-tokens (reduce + 0 (keep :exactObservedTokens ultra))]
    {:source "codex-account-jsonl:last-token-usage"
     :scope "provider-owned-openai-turn-ledger"
     :claim (str "OpenAI account-log effort is provider-owned per-turn evidence; it is not "
                 "the mutable final native-session effort and cannot by itself prove which "
                 "North/native execution path produced a turn")
     :exactObservedTokens (when (pos? all-tokens) all-tokens)
     :knownEffortExactObservedTokens (when (pos? known-tokens) known-tokens)
     :unknownEffortExactObservedTokens (when (pos? all-tokens) (- all-tokens known-tokens))
     :knownEffortObservations known-observations
     :effortCoveragePercent (percent known-tokens all-tokens)
     :highEffortExactObservedTokens (when (pos? high-tokens) high-tokens)
     :highEffortTokenSharePercent (percent high-tokens known-tokens)
     :ultraExactObservedTokens (when (pos? ultra-tokens) ultra-tokens)
     :ultraTokenSharePercent (percent ultra-tokens known-tokens)}))

(defn economics-interval [rows sessions persisted accounts start end]
  (let [managed (vec (filter #(row-in-interval? % start end) rows))
        usage (interval-report rows sessions persisted accounts start end)
        native (vec (filter (fn [session]
                              (when-let [at (parse-instant (:startedAt session))]
                                (and (not (.isBefore at start)) (.isBefore at end))))
                            sessions))
        exact (vec (filter #(some? (:tokens %)) managed))
        all-exact-tokens (exact-token-sum exact)
        premium-eligible (vec (filter #(and (some? (:tokens %))
                                            (= "applied" (:tierProvenance %))
                                            (contains? tier-rank (:tier %))) managed))
        premium-eligible-tokens (exact-token-sum premium-eligible)
        premium (vec (filter #(#{"senior" "frontier"} (:tier %)) premium-eligible))
        premium-tokens (exact-token-sum premium)
        premium-share (percent premium-tokens premium-eligible-tokens)
        current-receipt (vec (filter #(= 1 (:routingAdmissionReceiptVersion %)) managed))
        assessed (vec (filter #(= "recorded" (:routingAssessmentStatus %)) current-receipt))
        current-unavailable (vec (filter #(= "unavailable" (:routingAssessmentStatus %))
                                         current-receipt))
        promotions (vec (filter assessed-promotion? assessed))
        promotion-share (percent (count promotions) (count assessed))
        known-unpinned (count (filter #(= "none" (:routingPinEvidenceStatus %)) current-receipt))
        evidenced-pins (count (filter #(= "current" (:routingPinEvidenceStatus %)) current-receipt))
        missing-pin-evidence (count (filter #(= "missing" (:routingPinEvidenceStatus %)) current-receipt))
        legacy-compatible-missing
        (count (filter #(= "legacy-missing" (:routingPinEvidenceStatus %)) current-receipt))
        known-pin-state (+ known-unpinned evidenced-pins missing-pin-evidence)
        pin-share (percent (+ evidenced-pins missing-pin-evidence) known-pin-state)
        receipt-coverage (percent (count current-receipt) (count managed))
        pin-evidence-coverage (percent known-pin-state (count managed))
        assessment-coverage (percent (count assessed) (count current-receipt))
        premium-coverage (percent (count premium-eligible) (count managed))
        assessed-coverage (percent (count assessed) (count managed))
        native-high (vec (filter #(>= (get reasoning-rank (:effort %) -1) 2) native))
        native-ultra (vec (filter #(= "ultra" (:effort %)) native))
        dispatch-mode (current-dispatch-mode)
        provider-effort (openai-provider-owned-effort usage)
        descendants-under-north (vec (filter #(and (= "subagent" (:actorKind %))
                                                    (= "north" (:dispatchModeAtStart %))) native))
        premium-finding (gated-ratio-finding
                         "ROUTING_PREMIUM_TOKEN_SHARE_HIGH"
                         "ROUTING_PREMIUM_TOKEN_SHARE_INSUFFICIENT_EVIDENCE"
                         premium-share (:premiumTokenSharePercent economics-thresholds)
                         (count premium-eligible) premium-coverage)
        promotion-finding (gated-ratio-finding
                           "ROUTING_PROMOTION_SHARE_HIGH"
                           "ROUTING_PROMOTION_SHARE_INSUFFICIENT_EVIDENCE"
                           promotion-share (:promotionSharePercent economics-thresholds)
                           (count assessed) assessed-coverage)
        pin-finding (gated-ratio-finding
                     "ROUTING_EXACT_PIN_SHARE_HIGH"
                     "ROUTING_PIN_SHARE_INSUFFICIENT_EVIDENCE"
                     pin-share (:pinSharePercent economics-thresholds)
                     known-pin-state pin-evidence-coverage)
        high-effort-finding
        (some-> (gated-ratio-finding
                 "OPENAI_PROVIDER_OWNED_HIGH_EFFORT_TOKEN_SHARE_HIGH"
                 "OPENAI_PROVIDER_OWNED_EFFORT_INSUFFICIENT_EVIDENCE"
                 (:highEffortTokenSharePercent provider-effort)
                 (:premiumTokenSharePercent economics-thresholds)
                 (:knownEffortObservations provider-effort)
                 (:effortCoveragePercent provider-effort))
                (assoc :eligibleObservations (:knownEffortObservations provider-effort)
                       :minimumEligibleObservations
                       (:minimumEligibleRuns economics-design-gates))
                (dissoc :eligibleRuns :minimumEligibleRuns))
        findings (cond-> []
                   premium-finding (conj premium-finding)
                   promotion-finding (conj promotion-finding)
                   pin-finding (conj pin-finding)
                 (pos? missing-pin-evidence)
                 (conj (alert "ROUTING_PIN_EVIDENCE_MISSING" missing-pin-evidence 0))
                 (pos? legacy-compatible-missing)
                 (conj (alert "ROUTING_LEGACY_PIN_EVIDENCE_MISSING"
                              legacy-compatible-missing 0))
                 (and (>= (count current-receipt)
                          (:minimumEligibleRuns economics-design-gates))
                      (some? receipt-coverage)
                      (>= receipt-coverage
                          (:minimumEvidenceCoveragePercent economics-design-gates))
                      assessment-coverage
                      (< assessment-coverage (:assessmentCoveragePercent economics-thresholds)))
                 (conj (alert "ROUTING_ASSESSMENT_COVERAGE_LOW" assessment-coverage
                              (:assessmentCoveragePercent economics-thresholds)))
                   high-effort-finding (conj high-effort-finding)
                   (pos? (or (:ultraExactObservedTokens provider-effort) 0))
                   (conj (assoc (alert "OPENAI_PROVIDER_OWNED_ULTRA_TOKENS_OBSERVED"
                                       (:ultraExactObservedTokens provider-effort) 0)
                                :source (:source provider-effort)))
                   (seq descendants-under-north)
                   (conj (alert "NATIVE_DESCENDANTS_UNDER_NORTH_DISPATCH"
                                (count descendants-under-north) 0)))
        prompt-rows (vec (filter #(and (:promptCompositionVersion %)
                                       (:capabilityClass %)
                                       (some? (:promptTotalBytes %))) managed))
        prompt-coverage (percent (count prompt-rows) (count managed))
        token-rows (vec (filter #(some? (:tokens %)) prompt-rows))
        token-coverage (percent (count token-rows) (count managed))
        headroom-groups
        (->> managed
             (group-by (fn [row] [(or (:promptCompositionVersion row) "unobserved")
                                  (or (:capabilityClass row) "unobserved")]))
             (map (fn [[[version capability] cohort]]
                    (let [sum-observed (fn [field]
                                         (let [values (keep field cohort)]
                                           (when (seq values) (reduce + 0 values))))
                          count-observed (fn [field] (count (keep field cohort)))]
                      {:promptCompositionVersion version
                       :capabilityClass capability
                       :runs (count cohort)
                       :promptByteRuns (count-observed :promptTotalBytes)
                       :exactTokenRuns (count-observed :tokens)
                       :stablePrefixBytes (sum-observed :promptStablePrefixBytes)
                       :uniqueTailBytes (sum-observed :promptUniqueTailBytes)
                       :totalPromptBytes (sum-observed :promptTotalBytes)
                       :authoritativeCompositionTokens
                       (sum-observed :promptTotalCompositionTokens)
                       :inputTokens (sum-observed :inputTokens)
                       :outputTokens (sum-observed :outputTokens)
                       :cacheReadTokens (sum-observed :cacheReadTokens)
                       :cacheCreateTokens (sum-observed :cacheCreateTokens)
                       :cachedInputTokens (sum-observed :cachedInputTokens)
                       :compactions (sum-observed :compactionCount)
                       :contextWindowObservedRuns
                       (count (filter #(= "observed" (:contextWindowStatus %)) cohort))
                       :effectiveContextBudgetObservedRuns
                       (count (keep :effectiveContextBudgetTokens cohort))})))
             (sort-by (juxt :promptCompositionVersion :capabilityClass)) vec)
        savings-reasons
        (cond-> ["controlled-cohort-unavailable" "matched-workload-identity-unavailable"]
          (or (nil? prompt-coverage)
              (< prompt-coverage (:minimumEvidenceCoveragePercent economics-design-gates)))
          (conj "prompt-evidence-coverage-below-threshold")
          (or (nil? token-coverage)
              (< token-coverage (:minimumEvidenceCoveragePercent economics-design-gates)))
          (conj "exact-token-coverage-below-threshold"))]
    {:start (.toString start) :end (.toString end)
     :boundary "start-inclusive,end-exclusive"
     :usage usage
     :managed {:runs (count managed)
               :exactTokenRuns (count exact)
               :unknownTokenRuns (- (count managed) (count exact))
               :exactObservedTokens all-exact-tokens
               :premiumExactObservedTokens premium-tokens
               :premiumEligibleExactObservedTokens premium-eligible-tokens
               :premiumTokenSharePercent premium-share
               :premiumEvidenceCoveragePercent premium-coverage
               :assessmentCoverage {:currentRecorded (count assessed)
                                    :currentUnavailable (count current-unavailable)
                                    :legacyUnknown (- (count managed) (count current-receipt))
                                    :currentReceiptCoveragePercent receipt-coverage
                                    :percentOfCurrent assessment-coverage}
               :promotions {:observed (count promotions)
                            :eligibleAssessedRuns (count assessed)
                            :currentUnavailable (count current-unavailable)
                            :legacyUnknown (- (count managed) (count current-receipt))
                            :evidenceCoveragePercent assessed-coverage
                            :percent promotion-share}
               :pins {:knownUnpinned known-unpinned :current evidenced-pins
                      :currentMissing missing-pin-evidence
                      :legacyCompatibleMissing legacy-compatible-missing
                      :legacyUnknown (- (count managed) (count current-receipt))
                      :currentEvidenceCoveragePercent pin-evidence-coverage
                      :currentReceiptCoveragePercent receipt-coverage
                      :percent pin-share}
               :provenanceCoverage
               {:complete (count (filter #(and (:executionSource %)
                                               (:executionTransport %)
                                               (:providerSessionPersistence %)
                                               (:threadProvenance %)
                                               (:turnProvenance %)) managed))
                :unknown (count (filter #(not (and (:executionSource %)
                                                  (:executionTransport %)
                                                  (:providerSessionPersistence %)
                                                  (:threadProvenance %)
                                                  (:turnProvenance %))) managed))}}
     :native {:sessions (count native) :currentDispatchModeContext dispatch-mode
              :historicalDispatchModeCoverage
              {:recorded (count (remove #(= "unknown" (:dispatchModeAtStart %)) native))
               :legacyUnknown (count (filter #(= "unknown" (:dispatchModeAtStart %)) native))}
              :latestSessionEffortSnapshot
              {:claim "mutable final session identity; not per-turn provider-log effort"
               :highEffortSessions (count native-high)
               :ultraSessions (count native-ultra)}
              :providerOwnedOpenAIEffort provider-effort
              :rootSessions (count (filter #(= "root" (:actorKind %)) native))
              :subagentSessions (count (filter #(= "subagent" (:actorKind %)) native))
              :unknownActorKind (count (filter #(= "unknown" (:actorKind %)) native))
              :sessionsByDepth (->> native (map #(if (number? (:depth %))
                                                  (str (:depth %)) "unknown"))
                                    frequencies (into (sorted-map)))
              :sessionsByDispatchModeAtStart
              (->> native (map :dispatchModeAtStart) frequencies (into (sorted-map)))}
     :headroomAttribution
     {:scope "prompt-version-by-capability-class"
      :policy "observational-alert-only"
      :runs (count managed)
      :promptEvidenceRuns (count prompt-rows)
      :promptEvidenceCoveragePercent prompt-coverage
      :exactTokenRuns (count token-rows)
      :exactTokenCoveragePercent token-coverage
      :groups headroom-groups
      :savingsVerdict {:status "cannot-determine" :reasons savings-reasons
                       :claim (str "Realized savings require a controlled, matched-workload cohort; "
                                   "observational cache, token, and compaction differences are not causal.")}}
     :operationalTelemetry (operational-telemetry managed)
     :findings findings
     :alerts (vec (filter #(= "alert" (:status %)) findings))}))

(defn windowed-economics-report [rows sessions {:keys [window-hours slice-hours now]}]
  (let [end (or (parse-instant now)
                (when now (throw (ex-info "--now expects an ISO-8601 instant" {})))
                (java.time.Instant/now))
        window-duration (duration-of-hours window-hours)
        slice-duration (duration-of-hours slice-hours)
        window-ms (.toMillis window-duration)
        slice-ms (.toMillis slice-duration)]
    (when (or (> slice-ms window-ms) (not (zero? (mod window-ms slice-ms))))
      (throw (ex-info "--window must be an exact multiple of --slice" {})))
    (let [start (.minus end window-duration)
          window-rows (vec (filter #(row-in-interval? % start end) rows))
          accounts (account-universe window-rows)
          persisted (attribute-provider-records (vec (account-log-records)) rows sessions)]
      {:report "economics" :scope "bounded-intervals"
       :policy "alert-only; no model is silently downgraded"
       :claim (str "token shares use exact observed managed tokens; native sessions and legacy "
                   "admission/provenance fields remain unknown rather than being inferred")
       :thresholds economics-thresholds
       :designGates economics-design-gates
       :window {:start (.toString start) :end (.toString end)
                :hours window-hours :sliceHours slice-hours
                :boundary "start-inclusive,end-exclusive"}
       :intervals (mapv (fn [index]
                          (let [interval-start (.plusMillis start (* index slice-ms))
                                interval-end (.plusMillis interval-start slice-ms)]
                            (assoc (economics-interval rows sessions persisted accounts
                                                      interval-start interval-end)
                                   :index (inc index))))
                        (range (quot window-ms slice-ms)))
       :cumulative (economics-interval rows sessions persisted accounts start end)})))

(defn promotion-variant-key [row]
  (if (seq (:legacyDebtReasons row))
    ;; Incomplete historical evidence is debt local to this run. Never let two
    ;; missing hashes manufacture semantic recurrence merely by sharing an ID.
    [:legacy (:entity row)]
    [:variant (:appliedFingerprintVersion row) (:appliedFingerprintDomain row)
     (:appliedContractSha256 row) (:appliedCapabilities row)
     (get-in row [:effectiveAxes :taskGrade])
     (get-in row [:effectiveAxes :domains])
     (get-in row [:effectiveAxes :topology])
     (get-in row [:effectiveAxes :tier])
     (get-in row [:effectiveAxes :reasoning])
     (get-in row [:effectiveAxes :posture])]))

(defn promotion-row [[_ rows]]
  (let [threads (set (keep :thread rows))
        ;; Managed lanes currently share one OS uid. Historical "verified"
        ;; projections used caller-controlled AGENT_ID and are display-only:
        ;; they cannot qualify a reusable staffing pattern for promotion.
        independently-verified 0
        qualified []
        qualified-threads (set (map :thread qualified))
        flagged (some :promotionCandidate rows)
        debt (vec (sort (set (mapcat :legacyDebtReasons rows))))
        legacy? (boolean (seq debt))
        recurrent (and (not legacy?) (>= (count qualified-threads) 2))
        review-status (cond
                        legacy? "legacy-debt"
                        (not flagged) "not-requested"
                        (not recurrent) "verification-boundary-unavailable"
                        :else "review-candidate")
        composition-ids (vec (sort (set (keep :compositionId rows))))
        labels (if legacy?
                 ["orchestration:legacy-debt"]
                 (mapv #(str "orchestration:bespoke:" %) composition-ids))
        representative (first rows)]
    {:compositionId (when (= 1 (count composition-ids)) (first composition-ids))
     :compositionIds composition-ids :compositionLabels labels
     :appliedContractSha256 (when-not legacy? (:appliedContractSha256 representative))
     :fingerprintVersion (when-not legacy? (:appliedFingerprintVersion representative))
     :fingerprintDomain (when-not legacy? (:appliedFingerprintDomain representative))
     :appliedDomainRequirementCount (when-not legacy?
                                      (:appliedDomainRequirementCount representative))
     :requestedAppliedIntegrity (vec (sort (set (map :requestedAppliedIntegrity rows))))
     :appliedCapabilities (when-not legacy? (:appliedCapabilities representative))
     :effectiveAxes (when-not legacy? (:effectiveAxes representative))
     :legacyDebt legacy? :legacyDebtReasons debt
     :runs (count rows) :distinctThreads (count threads)
     :qualifiedRuns (count qualified) :qualifiedThreads (count qualified-threads)
     :recurrent recurrent
     :nearestPresets (vec (sort (set (keep :nearestPreset rows))))
     :operationalRan (count (filter #(= "ran" (:processOutcome %)) rows))
     :independentlyVerified independently-verified
     :promotionRequested (boolean flagged)
     :reviewStatus review-status
     :note "recurrence is evidence for human review; this report never promotes a role"}))

(defn promotions-report [rows]
  (let [bespoke (filter #(= "bespoke" (:compositionKind %)) rows)
        groups (group-by promotion-variant-key bespoke)
        id-variants (reduce (fn [acc row]
                              (if (or (seq (:legacyDebtReasons row)) (nil? (:compositionId row))) acc
                                (update acc (:compositionId row) (fnil conj #{})
                                        (promotion-variant-key row))))
                            {} bespoke)
        variant-counts (into {} (map (fn [[id variants]] [id (count variants)]) id-variants))
        composition-rows
        (map (fn [group]
               (let [row (promotion-row group)
                     ids (:compositionIds row)
                     aliases (if (> (count ids) 1) ids [])
                     drifted (vec (filter #(> (get variant-counts % 0) 1) ids))]
                 (assoc row
                        :aliasCompositionIds aliases
                        :driftedCompositionIds drifted
                        :hasAliasEvidence (boolean (seq aliases))
                        :hasDriftEvidence (boolean (seq drifted)))))
             groups)]
    {:report "promotions"
     :fingerprintVersion bespoke-fingerprint-version
     :fingerprintDomain bespoke-fingerprint-domain
     :claim (str "observed bespoke variants grouped by applied canonical contract hash, canonical "
                 "capabilities, and effective routing axes (including normalized domains); "
                 "version/domain, explicit domain-count evidence, and requested/applied integrity "
                 "are checked; incomplete evidence remains per-run legacy debt; never automatic promotion")
     :compositions (->> composition-rows
                        (sort-by (juxt :legacyDebt (comp - :distinctThreads)
                                      #(or (:appliedContractSha256 %) "")
                                      #(str/join "," (:compositionIds %))))
                        vec)}))

(defn calibration-observation-valid? [row]
  (let [grade (:judgmentGrade row)
        status (:judgmentGradeStatus row)
        source (:judgmentGradeSource row)
        topology (:struggleTopology row)
        repeat-threshold (:struggleLoopRepeatThreshold row)
        loop-window (:struggleLoopWindow row)
        triggers (:struggleTriggers row)]
    (and (= "valid" status)
         (judgment-grade-values grade)
         (= "thread" source)
         (#{"worker" "orchestrator"} topology)
         (attributed? (:struggleDetectorPolicyVersion row))
         (every? pos-int?
                 [(:struggleErrorStreakThreshold row)
                  repeat-threshold loop-window
                  (:struggleNoProgressTurnThreshold row)])
         (<= repeat-threshold loop-window)
         (some? (:struggleErrorCount row))
         (not (neg? (:struggleErrorCount row)))
         (= (count triggers) (count (distinct triggers)))
         (every? struggle-trigger-values triggers))))

(defn calibration-cohort-key [row]
  [(:judgmentGrade row) (:struggleTopology row)
   (:struggleDetectorPolicyVersion row)
   (:struggleErrorStreakThreshold row)
   (:struggleLoopRepeatThreshold row)
   (:struggleLoopWindow row)
   (:struggleNoProgressTurnThreshold row)])

(defn calibration-row [[[grade topology version error-streak loop-repeat loop-window no-progress]
                        rows]]
  (let [triggers (mapcat :struggleTriggers rows)]
    {:judgmentGrade grade
     :topology topology
     :policyVersion version
     :thresholds {:errorStreak error-streak
                  :loopRepeat loop-repeat
                  :loopWindow loop-window
                  :noProgressTurns no-progress}
     :runs (count rows)
     :struggleRuns (count (filter #(seq (:struggleTriggers %)) rows))
     :triggerCounts (into (sorted-map) (frequencies triggers))
     :errorCount (reduce + (map :struggleErrorCount rows))}))

(defn calibration-report [rows]
  (let [all-rows (vec rows)
        valid-rows (vec (filter calibration-observation-valid? all-rows))
        exact-grade-counts (frequencies (map :judgmentGrade valid-rows))
        statuses (frequencies (map #(or (:judgmentGradeStatus %) "unrecorded") all-rows))]
    {:report "calibration"
     :claim (str "judgment grade and detector configuration are immutable run-local observations; "
                 "current thread facts are never calibration inputs")
     :runs (count all-rows)
     :eligibleRuns (count valid-rows)
     :excludedRuns (- (count all-rows) (count valid-rows))
     :gradeStatus {:valid (get statuses "valid" 0)
                   :unavailable (get statuses "unavailable" 0)
                   :invalid (get statuses "invalid" 0)
                   :unrecorded (get statuses "unrecorded" 0)}
     :gradeCounts {:s (get exact-grade-counts "s" 0)
                   :m (get exact-grade-counts "m" 0)
                   :l (get exact-grade-counts "l" 0)}
     :cohorts (->> valid-rows
                   (group-by calibration-cohort-key)
                   (map calibration-row)
                   (sort-by (juxt :judgmentGrade :topology :policyVersion))
                   vec)}))

(defn timing-observation-valid? [row]
  (let [estimate-hours (:estimateHours row)
        duration-ms (:durationMs row)
        estimate-ms (when (and (some? estimate-hours) (pos? estimate-hours))
                      (long (Math/round (* estimate-hours 60.0 60.0 1000.0))))
        expected-delta (when (and duration-ms estimate-ms)
                         (- duration-ms estimate-ms))
        expected-ratio (when (and duration-ms estimate-ms (pos? estimate-ms))
                         (/ (double duration-ms) (double estimate-ms)))
        expected-classification
        (when expected-delta
          (cond (neg? expected-delta) "under"
                (pos? expected-delta) "over"
                :else "on"))]
    (and estimate-ms (pos? estimate-ms)
         duration-ms
         (= expected-delta (:estimateDeltaMs row))
         (some? (:estimateRatio row)) (not (neg? (:estimateRatio row)))
         (<= (Math/abs (- expected-ratio (:estimateRatio row))) 0.0000005)
         (= expected-classification (:estimateClassification row)))))

(defn timing-report [rows]
  (let [all-rows (vec rows)
        eligible (->> all-rows
                      (filter timing-observation-valid?)
                      (sort-by (juxt :at :entity))
                      vec)
        counts (frequencies (map :estimateClassification eligible))]
    {:report "timing"
     :claim (str "estimate_hours is the immutable dispatch-time snapshot; durationMs is the "
                 "existing North-observed terminal wall time; deltaMs is actual minus estimate "
                 "and ratio is actual divided by estimate")
     :runs (count all-rows)
     :eligibleRuns (count eligible)
     :excludedRuns (- (count all-rows) (count eligible))
     :noEstimateRuns (count (filter #(nil? (:estimateHours %)) all-rows))
     :invalidTimingRuns
     (count (filter #(and (some? (:estimateHours %))
                          (not (timing-observation-valid? %)))
                    all-rows))
     :classifications {:under (get counts "under" 0)
                       :on (get counts "on" 0)
                       :over (get counts "over" 0)}
     :runsDetail (mapv #(select-keys % [:entity :thread :at :estimateHours
                                        :durationMs :estimateDeltaMs :estimateRatio
                                        :estimateClassification])
                       eligible)}))

(def learning-assignment-version "north-learning-assignment:v1")
(def learning-policy-version "north-learning-policy:v1")
(def learning-prompt-receipt-version "north-prompt-receipt:v1")
(def learning-environment-receipt-version "north-environment-receipt:v1")
(def learning-run-envelope-version "north-run-envelope:v1")
(def learning-axes #{"control" "model-tier" "effort" "prompt" "authoring" "history"})

(defn probability? [value]
  (and (some? value) (Double/isFinite (double value)) (<= 0.0 value 1.0)))

(defn learning-assignment-valid? [row]
  (let [arm (:learningArm row)
        axis (:learningAxis row)
        arm-id (:learningArmId row)]
    (and (= learning-assignment-version (:learningAssignmentVersion row))
         (= learning-policy-version (:learningPolicyVersion row))
         (re-matches sha256-pattern (or (:learningPolicySha256 row) ""))
         (#{"frozen" "learning"} (:learningMode row))
         (#{"discovery" "evaluation"} (:learningEvidenceMode row))
         (attributed? (:learningExperimentId row))
         (attributed? (:learningEpisodeId row))
         (re-matches sha256-pattern (or (:learningTaskSignatureSha256 row) ""))
         (#{"exact" "partial" "unknown"} (:learningTaskSignatureCoverage row))
         (#{"control" "explore"} arm)
         (or (and (= "control" arm) (= "control" axis) (= "control" arm-id))
             (and (= "explore" arm)
                  (and (learning-axes axis) (not= "control" axis))
                  (attributed? arm-id)
                  (not= "control" arm-id)))
         (probability? (:learningPropensity row))
         (probability? (:learningExplorePropensity row))
         (attributed? (:learningNarrowingReason row))
         (every? #(re-matches sha256-pattern (or (% row) ""))
                 [:learningBaselineSha256 :learningOptionsSha256
                  :learningAssignmentSha256]))))

(defn learning-receipts-exact? [row]
  (and (= learning-prompt-receipt-version (:promptReceiptVersion row))
       (= "exact" (:promptReceiptCoverage row))
       (re-matches sha256-pattern (or (:promptReceiptSha256 row) ""))
       (re-matches sha256-pattern (or (:promptWireSha256 row) ""))
       (= learning-environment-receipt-version (:environmentReceiptVersion row))
       (= "exact" (:environmentReceiptCoverage row))
       (every? #(re-matches sha256-pattern (or (% row) ""))
               [:environmentReceiptSha256 :availableSkillCatalogSha256
                :activatedResourceClosureSha256])
       (= learning-run-envelope-version (:runEnvelopeVersion row))
       (re-matches sha256-pattern (or (:runEnvelopeSha256 row) ""))))

(defn learning-bar-exact? [row]
  (let [{:keys [bars evidenced]} (:evidence row)]
    (and (pos? (or bars 0)) (= bars evidenced))))

(defn learning-exclusion-reasons [row]
  (cond-> []
    (not (learning-assignment-valid? row)) (conj "assignment-incomplete-or-invalid")
    (not= "evaluation" (:learningEvidenceMode row)) (conj "not-evaluation")
    (not= "exact" (:learningTaskSignatureCoverage row))
    (conj "task-signature-not-exact")
    (not= "exact" (:promptReceiptCoverage row)) (conj "prompt-receipt-not-exact")
    (not= "exact" (:environmentReceiptCoverage row))
    (conj "environment-receipt-not-exact")
    (not (learning-receipts-exact? row)) (conj "receipt-envelope-incomplete-or-invalid")
    (not (learning-bar-exact? row)) (conj "done-bar-not-evidenced")))

(defn learning-observation [row]
  (let [reasons (learning-exclusion-reasons row)]
    {:entity (:entity row)
     :thread (:thread row)
     :at (:at row)
     :eligible (empty? reasons)
     :exclusionReasons reasons
     :assignment {:experimentId (:learningExperimentId row)
                  :episodeId (:learningEpisodeId row)
                  :mode (:learningMode row)
                  :evidenceMode (:learningEvidenceMode row)
                  :taskSignatureSha256 (:learningTaskSignatureSha256 row)
                  :taskSignatureCoverage (:learningTaskSignatureCoverage row)
                  :risk (:learningRisk row)
                  :axis (:learningAxis row)
                  :arm (:learningArm row)
                  :armId (:learningArmId row)
                  :propensity (:learningPropensity row)
                  :explorePropensity (:learningExplorePropensity row)
                  :narrowingReason (:learningNarrowingReason row)
                  :policySha256 (:learningPolicySha256 row)
                  :assignmentSha256 (:learningAssignmentSha256 row)}
     :receipts {:promptSha256 (:promptReceiptSha256 row)
                :promptCoverage (:promptReceiptCoverage row)
                :environmentSha256 (:environmentReceiptSha256 row)
                :environmentCoverage (:environmentReceiptCoverage row)
                :runEnvelopeSha256 (:runEnvelopeSha256 row)}
     :metrics {:tokens (:tokens row)
               :durationMs (:durationMs row)
               :turns (:turns row)
               :processOutcome (:processOutcome row)
               :deliveryOutcome (:deliveryOutcome row)
               :deliveryReason (:deliveryReason row)
               :struggleTriggers (:struggleTriggers row)
               :struggleErrorCount (:struggleErrorCount row)
               :doneBars (get-in row [:evidence :bars])
               :evidencedBars (get-in row [:evidence :evidenced])}}))

(defn learning-arm-summary [[[_task-signature axis arm-id] rows]]
  (let [usage (usage-stats rows)]
    {:taskSignatureSha256 (:learningTaskSignatureSha256 (first rows))
     :axis axis
     :armId arm-id
     :assignmentArm (:learningArm (first rows))
     :runs (count rows)
     :runIds (mapv :entity (sort-by :entity rows))
     :tokens (:tokens usage)
     :tokenEvidence (:tokenEvidence usage)
     :tokenCoverage (:tokenCoverage usage)
     :wallMilliseconds (:wallMilliseconds usage)
     :durationEvidence (:durationEvidence usage)
     :durationCoverage (:durationCoverage usage)
     :turns (:turns usage)
     :turnEvidence (:turnEvidence usage)
     :turnCoverage (:turnCoverage usage)
     :processOutcomes (into (sorted-map) (frequencies (map :processOutcome rows)))
     :deliveryOutcomes (into (sorted-map) (frequencies (map :deliveryOutcome rows)))
     :struggleRuns (count (filter #(seq (:struggleTriggers %)) rows))
     ;; Quality evidence is surfaced only here, after exact done-bar admission.
     :barEvidencedRuns (count rows)}))

(defn learning-comparison-group [[[experiment-id task-signature] rows]]
  (let [controls (filter #(= "control" (:learningArm %)) rows)
        exploratory (filter #(= "explore" (:learningArm %)) rows)
        axes (sort (distinct (map :learningAxis exploratory)))]
    {:experimentId experiment-id
     :taskSignatureSha256 task-signature
     :controlRuns (count controls)
     :exploratoryRuns (count exploratory)
     :axes axes
     :comparable (boolean (and (seq controls) (seq exploratory)))
     :reason (cond
               (empty? controls) "no-control-observation"
               (empty? exploratory) "no-exploratory-observation"
               :else "evaluation-ready")}))

(defn learning-report [rows]
  (let [observed (->> rows
                      (filter #(some identity
                                     ((juxt :learningAssignmentVersion
                                            :learningPolicySha256
                                            :learningMode :learningAxis
                                            :learningArmId :learningAssignmentSha256) %)))
                      vec)
        observations (mapv learning-observation observed)
        eligible-ids (set (map :entity (filter :eligible observations)))
        eligible (filterv #(eligible-ids (:entity %)) observed)
        exclusions (mapcat :exclusionReasons observations)]
    {:report "learning"
     :claim (str "ordinary-operation assignments are grouped only when evaluation mode, "
                 "task identity, prompt/environment receipts, and done-bar evidence are exact; "
                 "discovery and incomplete evidence remain visible but are never comparative evidence")
     :runs (count observed)
     :eligibleRuns (count eligible)
     :excludedRuns (- (count observed) (count eligible))
     :exclusions (into (sorted-map) (frequencies exclusions))
     :cohorts (->> eligible
                   (group-by (juxt :learningTaskSignatureSha256
                                   :learningAxis :learningArmId))
                   (map learning-arm-summary)
                   (sort-by (juxt :taskSignatureSha256 :axis :armId)) vec)
     :comparisonGroups (->> eligible
                            (group-by (juxt :learningExperimentId
                                            :learningTaskSignatureSha256))
                            (map learning-comparison-group)
                            (sort-by (juxt :experimentId :taskSignatureSha256)) vec)
     :observations observations}))

(defn report [kind rows & [{:keys [all? by-model? by-effort?]
                            :or {all? false by-model? false by-effort? false}}]]
  (case kind
    "performance" (performance-report rows all?)
    "usage" (usage-report rows {:by-model? by-model? :by-effort? by-effort?})
    "waste" (waste-report rows)
    "economics" (throw (ex-info "economics requires bounded window options" {}))
    "promotions" (promotions-report rows)
    "calibration" (calibration-report rows)
    "timing" (timing-report rows)
    "learning" (learning-report rows)
    (throw (ex-info "usage: north routing report [performance|usage|waste|economics|promotions|calibration|timing|learning] [--json] [--all]" {}))))

(defn usage-table-line
  ([label row] (usage-table-line label row {}))
  ([label row {:keys [label-width] :or {label-width 14}}]
   (let [{token-exact :exactRuns token-runs :runs} (:tokenCoverage row)
         {duration-exact :exactRuns duration-runs :runs} (:durationCoverage row)
         {turn-exact :exactRuns turn-runs :runs} (:turnCoverage row)
         token-label (case (:tokenEvidence row)
                       "exact" (str (:tokens row))
                       "lower-bound" (str (:tokens row) "+")
                       "unobserved")
         wall-value (when-let [seconds (:wallSeconds row)]
                      (if (== seconds (Math/floor seconds))
                        (format "%.0f" seconds)
                        (format "%.3f" seconds)))
         wall-label (case (:durationEvidence row)
                      "exact" wall-value
                      "lower-bound" (str wall-value "+")
                      "unobserved")
         turn-label (case (:turnEvidence row)
                      "exact" (str (:turns row))
                      "lower-bound" (str (:turns row) "+")
                      "unobserved")]
     (format (str "%-" label-width "s %6d %16s %11s %14s %11s %12s %11s %9d %9d")
             label (:runs row)
             token-label (str token-exact "/" token-runs)
             wall-label (str duration-exact "/" duration-runs)
             turn-label (str turn-exact "/" turn-runs)
             (:fallbacks row)
             (:escalatedRuns row)))))

(defn observed-token-label [value]
  (if (some? value) (str value) "unobserved"))

(defn print-account-interval [label interval]
  (println)
  (println (format "%s  %s → %s  runs=%d exact=%d unknown=%d tokens=%s"
                   label (:start interval) (:end interval)
                   (:terminalRuns interval) (:exactTokenRuns interval)
                   (:unknownTokenRuns interval)
                   (observed-token-label (:exactObservedTokens interval))))
  (println (format "%-42s %-10s %6s %7s %7s %16s" "ACCOUNT" "PROVIDER"
                   "runs" "exact" "unknown" "tokens"))
  (doseq [account (:accounts interval)]
    (println (format "%-42s %-10s %6d %7d %7d %16s"
                     (:providerTarget account) (:provider account)
                     (:terminalRuns account) (:exactTokenRuns account)
                     (:unknownTokenRuns account)
                     (observed-token-label (:exactObservedTokens account))))
    (doseq [row (:breakdown account)]
      (let [pct (:percentageOfAccountExactObservedTokens row)]
        (println (format "  %-24s / %-8s runs=%d exact=%d unknown=%d tokens=%s account-share=%s"
                         (:model row) (:effort row) (:terminalRuns row)
                         (:exactTokenRuns row) (:unknownTokenRuns row)
                         (observed-token-label (:exactObservedTokens row))
                         (if (some? pct) (format "%.2f%%" pct) "unobserved"))))))
  (let [native (:nativeInteractiveActivity interval)]
    (println (format "  native interactive: sessions=%d tokens=unobserved account=unobserved (excluded from managed percentages)"
                     (:sessions native)))
    (doseq [group (:groups native)]
      (println (format "    %-10s %-24s / %-8s sessions=%d"
                       (:provider group) (:model group) (:effort group)
                       (:sessions group)))))
  (let [observed (:accountObserved interval)]
    (println (format "  account-observed: tokens=%s (%s)"
                     (observed-token-label (:exactObservedTokens observed))
                     (:tokenEvidence observed)))
    (doseq [account (:accounts observed)]
      (println (format "    %-42s %-10s provider-owned=%s managed=%s combined=unavailable overlap=%s"
                       (:providerTarget account) (:provider account)
                       (observed-token-label (:exactObservedTokens account))
                       (observed-token-label (get-in account [:managedLedger :exactObservedTokens]))
                       (:overlapStatus account)))
      (doseq [row (:breakdown account)]
        (println (format "      provider-owned %-24s / %-8s observations=%d tokens=%s"
                         (:model row) (or (:effort row) "unobserved")
                         (:observations row)
                         (observed-token-label (:exactObservedTokens row))))))))

(defn print-table [data]
  (case (:report data)
    "performance"
    (do (println (str "ROUTING PERFORMANCE — "
                      (if (= "all-history" (:scope data))
                        "all historical rows"
                        "complete current managed runs")))
        (println "Current rows require complete applied Orchestration evidence; reported delivery is exact run-scoped self-report, independent verification is unavailable under shared-UID lanes, and mutable thread evidence is not model quality.")
        (println "d-blk is a stable total, never a provider failure rate by itself: blk-pv is provider-caused (died/stalled/error/empty-result), blk-us is North's own preflight/spend-guard/cap/escalation/reconciliation refusals, blk-lp is presence-lapse deaths suspected manufactured by a terminal-publication bug (thread 019f9c3b) pending its fix, blk-ot is an unrecognized reason. blk-pv+blk-us+blk-lp+blk-ot always sums to d-blk.")
        (when (pos? (:excludedRuns data))
          (println (format "%d legacy/incomplete/unattributed row(s) excluded; use --all to inspect them."
                           (:excludedRuns data))))
        (println (format "%-38s %5s %5s %5s %5s %5s %5s %5s %5s %5s %5s %5s %5s %5s"
                         "COHORT provider/tier/role/grade" "runs" "ran"
                         "d-ver" "d-rpt" "d-unv" "d-blk" "blk-pv" "blk-us" "blk-lp" "blk-ot"
                         "t-cls" "t-part" "t-none" "esc"))
        (doseq [row (:cohorts data)]
          (println (format "%-38s %5d %5d %5d %5d %5d %5d %5d %5d %5d %5d %5d %5d %5d"
                           (:cohort row) (:runs row) (:operationalRan row)
                           (:deliveryVerified row) (:deliveryReported row)
                           (:deliveryUnverified row) (:deliveryBlocked row)
                           (:deliveryBlockedProviderCaused row)
                           (:deliveryBlockedNorthCaused row)
                           (:deliveryBlockedSuspectLapse row)
                           (:deliveryBlockedUnattributed row)
                           (:threadClosedEvidenced row)
                           (:threadPartialEvidence row)
                           (+ (:threadUnevidenced row) (:threadNoContract row))
                           (:escalated row))))
        (when (empty? (:cohorts data))
          (println "  (no complete current managed runs; use --all for historical rows)")))
    "usage"
    (if (= "bounded-intervals" (:scope data))
      (do
        (println "ROUTING USAGE — exact observed tokens (unknown usage is never zero)")
        (println (format "WINDOW %s → %s · %.3gh slices · %s"
                         (get-in data [:window :start]) (get-in data [:window :end])
                         (double (get-in data [:window :sliceHours]))
                         (get-in data [:window :boundary])))
        (doseq [interval (:intervals data)]
          (print-account-interval (str "INTERVAL " (:index interval)) interval))
        (print-account-interval "CUMULATIVE" (:cumulative data)))
      (do (println "ROUTING USAGE — observed work (never dollars or API credits)")
          (println (format "%-14s %6s %16s %11s %14s %11s %12s %11s %9s %9s"
                           "PROVIDER" "runs" "tokens" "tok exact" "wall-s"
                           "wall exact" "turns" "turn exact" "fallbacks" "escalated"))
          (doseq [row (:providers data)]
            (println (usage-table-line (:provider row) row)))
          (when-let [models (:models data)]
            (println)
            (println "MODEL — observed work (row per model, or model × effort with --by-effort)")
            (println (format "%-24s %6s %16s %11s %14s %11s %12s %11s %9s %9s"
                             "MODEL" "runs" "tokens" "tok exact" "wall-s"
                             "wall exact" "turns" "turn exact" "fallbacks" "escalated"))
            (doseq [row models]
              (println (usage-table-line
                        (if (:effort row) (str (:model row) "/" (:effort row)) (:model row))
                        row {:label-width 24}))))))
    "waste"
    (do
      (println "ROUTING WASTE — managed-dispatch machinery gate")
      (println (format "WINDOW tokens=%.0f exact-observed=%d target=%d runs=%d"
                       (double (:windowTokenTotal data)) (:exactObservedTokens data)
                       (:windowTokenLimit data) (:runCount data)))
      (println (format "WASTE ratio=%.2f%% tokens=%.0f runs=%d"
                       (double (:wasteRatioPercent data))
                       (double (:machineryWastedTokens data)) (:wasteRuns data)))
      (println (format "%-34s %6s %8s %14s %14s"
                       "BUCKET" "runs" "unknown" "exact-tokens" "gating-waste"))
      (doseq [row (:buckets data)]
        (println (format "%-34s %6d %8d %14d %14.0f"
                         (:bucket row) (:runs row) (:unknownTokenRuns row)
                         (:exactTokens row) (double (:gatingWasteTokens row)))))
      (let [unknown (:unknownCoverage data)]
        (println (format (str "unknown-coverage runs=%d/%d gating-waste-tokens=%.0f "
                              "exact-coverage=%.2f%% required=%.2f%%")
                         (:runs unknown) (:totalRuns unknown)
                         (double (:gatingWasteTokens unknown))
                         (double (:exactCoveragePercent unknown))
                         (double (:requiredExactCoveragePercent unknown)))))
      (println (str "VERDICT " (:verdict data))))
    "promotions"
    (do (println "BESPOKE PATTERNS — stock-template review candidates")
        (println "Variants use applied canonical hash + capabilities + effective axes; missing hashes are per-run legacy debt.")
        (println "Recurrence nominates human review; it never adds or changes a stock template.")
        (if (empty? (:compositions data)) (println "  (no bespoke compositions observed)")
          (doseq [row (:compositions data)]
            (let [label (str/join "," (:compositionLabels row))
                  hash (or (:appliedContractSha256 row) "missing")
                  capabilities (str/join "," (or (:appliedCapabilities row) []))]
              (println (format "%-34s threads=%d runs=%d verified=%d  %s"
                               label (:distinctThreads row) (:runs row)
                               (:independentlyVerified row) (:reviewStatus row)))
              (println (str "  hash=" hash " capabilities=" capabilities))
              (println "  requested↔applied="
                       (str/join "," (:requestedAppliedIntegrity row)))
              (when-let [axes (:effectiveAxes row)] (println "  axes=" (pr-str axes)))
              (when (:hasAliasEvidence row)
                (println "  aliases=" (str/join "," (:aliasCompositionIds row))))
              (when (:hasDriftEvidence row)
                (println "  drift=" (str/join "," (:driftedCompositionIds row))))
              (when (:legacyDebt row)
                (println "  debt=" (str/join "," (:legacyDebtReasons row))))))))
    "calibration"
    (do
      (println "ROUTING CALIBRATION — immutable run-local judgment + struggle evidence")
      (println (format "runs=%d eligible=%d excluded=%d status=%s grades=%s"
                       (:runs data) (:eligibleRuns data) (:excludedRuns data)
                       (pr-str (:gradeStatus data)) (pr-str (:gradeCounts data))))
      (if (empty? (:cohorts data))
        (println "  (no complete run-local calibration observations)")
        (doseq [row (:cohorts data)]
          (println (format "%s/%s runs=%d struggle=%d errors=%d thresholds=%s triggers=%s"
                           (:judgmentGrade row) (:topology row) (:runs row)
                           (:struggleRuns row) (:errorCount row)
                           (pr-str (:thresholds row))
                           (pr-str (:triggerCounts row)))))))
    "timing"
    (do
      (println "RUN TIMING — dispatch estimate versus North-observed terminal wall time")
      (println (format "runs=%d eligible=%d excluded=%d no-estimate=%d invalid=%d classes=%s"
                       (:runs data) (:eligibleRuns data) (:excludedRuns data)
                       (:noEstimateRuns data) (:invalidTimingRuns data)
                       (pr-str (:classifications data))))
      (println (format "%-36s %-24s %10s %12s %12s %10s %6s"
                       "RUN" "THREAD" "EST-H" "ACTUAL-MS" "DELTA-MS" "RATIO" "CLASS"))
      (doseq [row (:runsDetail data)]
        (println (format "%-36s %-24s %10s %12d %+12d %10.6f %6s"
                         (:entity row) (:thread row) (str (:estimateHours row))
                         (:durationMs row) (:estimateDeltaMs row)
                         (:estimateRatio row) (:estimateClassification row)))))
    "learning"
    (do
      (println "LEARNING REGIME — bounded ordinary-operation evaluation")
      (println "Discovery and incomplete construction evidence are visible below but never enter comparison cohorts.")
      (println (format "runs=%d eligible=%d excluded=%d exclusions=%s"
                       (:runs data) (:eligibleRuns data) (:excludedRuns data)
                       (pr-str (:exclusions data))))
      (println (format "%-12s %-12s %5s %14s %12s %14s %12s %9s"
                       "AXIS" "ARM" "runs" "tokens" "tok exact"
                       "wall-ms" "wall exact" "struggle"))
      (doseq [row (:cohorts data)]
        (println (format "%-12s %-12s %5d %14s %12s %14s %12s %9d"
                         (:axis row) (:armId row) (:runs row)
                         (observed-token-label (:tokens row))
                         (str (get-in row [:tokenCoverage :exactRuns]) "/"
                              (get-in row [:tokenCoverage :runs]))
                         (observed-token-label (:wallMilliseconds row))
                         (str (get-in row [:durationCoverage :exactRuns]) "/"
                              (get-in row [:durationCoverage :runs]))
                         (:struggleRuns row))))
      (doseq [group (:comparisonGroups data)]
        (println (format "  %s/%s control=%d explore=%d axes=%s status=%s"
                         (:experimentId group)
                         (subs (:taskSignatureSha256 group) 0 12)
                         (:controlRuns group) (:exploratoryRuns group)
                         (str/join "," (:axes group)) (:reason group))))
      (when (empty? (:cohorts data))
        (println "  (no evaluation-ready cohorts)")))
    "economics"
    (do
      (println "ROUTING ECONOMICS — bounded exact observations, alert-only policy")
      (println (format "WINDOW %s → %s · %.3gh slices"
                       (get-in data [:window :start]) (get-in data [:window :end])
                       (double (get-in data [:window :sliceHours]))))
      (doseq [interval (concat (:intervals data) [(:cumulative data)])]
        (println (format "%s %s → %s managed=%d tokens=%s premium=%s assessment-current=%s pins=%s latest-native-high=%d latest-native-ultra=%d"
                         (if (:index interval) (str "INTERVAL " (:index interval)) "CUMULATIVE")
                         (:start interval) (:end interval)
                         (get-in interval [:managed :runs])
                         (observed-token-label (get-in interval [:managed :exactObservedTokens]))
                         (if-let [pct (get-in interval [:managed :premiumTokenSharePercent])]
                           (format "%.2f%%" pct) "unobserved")
                         (if-let [pct (get-in interval [:managed :assessmentCoverage :percentOfCurrent])]
                           (format "%.2f%%" pct) "unobserved")
                         (if-let [pct (get-in interval [:managed :pins :percent])]
                           (format "%.2f%%" pct) "unobserved")
                         (or (get-in interval [:native :latestSessionEffortSnapshot
                                               :highEffortSessions]) 0)
                         (or (get-in interval [:native :latestSessionEffortSnapshot
                                               :ultraSessions]) 0)))
        (print-account-interval "  ACCOUNT LEDGER" (:usage interval))
        (let [headroom (:headroomAttribution interval)]
          (println (format "  HEADROOM prompt=%d/%d coverage=%s exact-token=%d/%d savings=%s reasons=%s"
                           (:promptEvidenceRuns headroom) (:runs headroom)
                           (if-let [pct (:promptEvidenceCoveragePercent headroom)]
                             (format "%.2f%%" pct) "unobserved")
                           (:exactTokenRuns headroom) (:runs headroom)
                           (get-in headroom [:savingsVerdict :status])
                           (str/join "," (get-in headroom [:savingsVerdict :reasons]))))
          (doseq [row (:groups headroom)]
            (println (format "    %s/%s runs=%d prompt-bytes=%s cache-read=%s cache-create=%s compactions=%s"
                             (:promptCompositionVersion row) (:capabilityClass row) (:runs row)
                             (or (:totalPromptBytes row) "unobserved")
                             (or (:cacheReadTokens row) "unobserved")
                             (or (:cacheCreateTokens row) "unobserved")
                             (or (:compactions row) "unobserved")))))
        (doseq [{:keys [code status reason observed threshold
                        eligibleRuns evidenceCoveragePercent]} (:findings interval)]
          (if (= "cannot-determine" status)
            (println (format "  CANNOT-DETERMINE %-34s reason=%s eligible=%s coverage=%s"
                             code reason eligibleRuns
                             (if (some? evidenceCoveragePercent)
                               (format "%.2f%%" evidenceCoveragePercent) "unobserved")))
            (println (format "  ALERT %-42s observed=%s threshold=%s"
                             code observed threshold))))))))

(def usage-help "usage: north routing report [performance|usage|waste|economics|promotions|calibration|timing|learning] [--json] [--all] [--by-model] [--by-effort] [--window 24h --slice 12h] [--now ISO-INSTANT]")

(defn parse-options [args]
  (loop [remaining args options {:flags #{}}]
    (if (empty? remaining) options
      (let [[arg value & more] remaining]
        (cond
          (#{"--json" "--all" "--by-model" "--by-effort"} arg)
          (recur (rest remaining) (update options :flags conj arg))

          (#{"--window" "--slice" "--now"} arg)
          (if (or (nil? value) (str/starts-with? value "--"))
            (throw (ex-info (str arg " requires a value") {}))
            (recur more (assoc options (keyword (subs arg 2)) value)))

          :else (throw (ex-info (str "unknown routing report option: " arg) {})))))))

(defn -main [& args]
  (let [[verb kind & raw-flags] args
        parsed (try (parse-options raw-flags)
                    (catch Exception error
                      (binding [*out* *err*]
                        (println (.getMessage error))
                        (println usage-help))
                      (System/exit 2)))
        flags (:flags parsed)
        all? (some #{"--all"} flags)
        by-model? (some #{"--by-model"} flags)
        by-effort? (some #{"--by-effort"} flags)
        window? (or (:window parsed) (:slice parsed) (:now parsed))]
    (when-not (= verb "report")
      (binding [*out* *err*] (println usage-help))
      (System/exit 2))
    (when (and all? (not= (or kind "performance") "performance"))
      (binding [*out* *err*] (println "--all applies only to the performance report"))
      (System/exit 2))
    (when (and (or by-model? by-effort?) (not= (or kind "performance") "usage"))
      (binding [*out* *err*] (println "--by-model/--by-effort apply only to the usage report"))
      (System/exit 2))
    (when (and window? (not (#{"usage" "economics"} (or kind "performance"))))
      (binding [*out* *err*] (println "--window/--slice/--now apply only to usage/economics reports"))
      (System/exit 2))
    (let [facts (fold-facts (read-ops (default-paths)
                                      (when (= "learning" kind)
                                        learning-fast-predicate?)))
          rows (vec (run-rows facts))
          report-rows (if (= kind "waste") (waste-attempt-rows facts rows) rows)
          sessions (native-session-rows facts)
          data (try
                 (if (or window? (= kind "economics"))
                   ((if (= kind "economics") windowed-economics-report windowed-usage-report)
                    rows sessions
                    {:window-hours (parse-hours (or (:window parsed) "24h") "--window")
                     :slice-hours (parse-hours (or (:slice parsed) "12h") "--slice")
                     :now (:now parsed)})
                   (report (or kind "performance") report-rows
                           {:all? all? :by-model? by-model? :by-effort? by-effort?}))
                 (catch Exception error
                   (binding [*out* *err*]
                     (println (.getMessage error))
                     (println usage-help))
                   (System/exit 2)))]
      (if (some #{"--json"} flags)
        (println (json/generate-string data))
        (print-table data)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
