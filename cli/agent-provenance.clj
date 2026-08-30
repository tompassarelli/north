(ns north.agent-provenance
  (:require [cheshire.core :as json]
            [clojure.string :as str]))

(def routing-override-fields #{"taskGrade" "domainRequirements" "tier" "reasoning" "posture"})

(def identity-predicates #{"kind" "role" "model" "provider" "provider_target" "live_input" "live_input_state" "live_input_epoch" "effort" "shadow_reviewer_note_capability_sha256" "composition_kind" "composition_id" "composition_overrides" "composition_override_reason" "nearest_template" "bespoke_reason" "promotion_candidate" "composition_contract_sha256" "composition_contract_fingerprint_version" "composition_contract_fingerprint_domain" "repo" "goal" "worktree" "branch" "retry_of_agent" "coordinator" "spawned_at"})

(def required-identity-predicates ["kind" "role" "goal" "provider" "provider_target" "live_input" "live_input_state" "live_input_epoch" "model" "effort" "composition_kind" "composition_id" "repo" "spawned_at" "display_handle" "display_name" "identity_manifest_sha256"])

(def terminal-predicates #{"process_outcome" "delivery_outcome" "delivery_reason" "delivery_evidence" "delivery_evidence_sha256" "terminal_manifest_sha256"})

(def conflict-key "__identity_conflicts")

(def values-key "__identity_values")

(def observation-sentinels #{"unknown" "unobserved"})

(defn known [value]
  (let [s (some-> value str str/trim)]
  (if (seq s) (do
  s))))

(defn- fold-terminal-value [prior value]
  (cond
  (nil? prior) #{value}
  (set? prior) (conj prior value)
  (and (sequential? prior) (not (string? prior))) (conj (set prior) value)
  :else #{prior value}))

(defn- fold-observed-value [prior value]
  (let [prior (cond
  (set? prior) prior
  (and (sequential? prior) (not (string? prior))) (set prior)
  (nil? prior) #{}
  :else #{prior})]
  (conj prior value)))

(defn fold-fact
  "Fold one graph row without hiding a second live managed value. Identity\n  conflicts retain their explicit defect marker; terminal values remain sets so\n  terminal projection validation rejects ambiguity instead of accepting the\n  graph row that happened to arrive last." [facts predicate value]
  (if (terminal-predicates predicate) (update facts predicate fold-terminal-value value) (let [managed-predicate? (or (identity-predicates predicate) (= "identity_manifest_sha256" predicate))
   prior (get facts predicate)]
  (cond-> (-> facts (assoc predicate value) (update-in [values-key predicate] fold-observed-value value)) (and managed-predicate? (some? prior) (not= prior value)) (update conflict-key (fnil conj #{}) predicate)))))

(defn observed-values
  "All distinct raw values seen for one non-terminal predicate. Plain fact maps\n  remain supported so tests and callers that did not come through fold-fact keep\n  the same projection." [facts predicate]
  (or (seq (get-in facts [values-key predicate])) (let [bind__4 (known (get facts predicate))]
  (if bind__4 (let [value bind__4]
  (do
  [value])))) []))

(defn native-axis
  "Order-independent selection for provider/model/effort observations on native\n  provider sessions. A concrete observation supersedes only historical\n  unknown/unobserved sentinels. Two concrete observations are an honest\n  conflict; neither row order nor a stored display string gets to elect one." [facts predicate]
  (let [values (set (keep known (observed-values facts predicate)))
   concrete (vec (sort (remove observation-sentinels values)))]
  (cond
  (= 1 (count concrete)) {:value (first concrete) :conflict false}
  (> (count concrete) 1) {:value nil :conflict true}
  (contains? values "unobserved") {:value "unobserved" :conflict false}
  (contains? values "unknown") {:value "unknown" :conflict false}
  :else {:value nil :conflict false})))

(defn safe-role-id? [value]
  (boolean (and (string? value) (re-matches #"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$" value))))

(defn composition-overrides [facts]
  (let [raw (known (get facts "composition_overrides"))]
  (if raw (try
  (let [value (json/parse-string raw)]
  (if (and (sequential? value) (every? routing-override-fields value) (= (count value) (count (set value)))) {:valid true :value (vec value)} {:valid false :value []}))
  (catch Exception _
    {:valid false :value []})) {:valid false :value []})))

(defn canonical-identity [facts]
  (->> identity-predicates (keep (fn [predicate] (let [bind__5 (known (get facts predicate))]
  (if bind__5 (let [value bind__5]
  (do
  [predicate value])))))) (sort-by first) (map (fn [[predicate value]] (str predicate "\u0000" value "\n"))) (apply str)))

(defn sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8))]
  (format "%064x" (java.math.BigInteger. 1 digest))))

(defn manifest-sha256 [facts]
  (sha256 (canonical-identity facts)))

(defn preset-evidence-defects [facts]
  (let [raw (get facts "composition_overrides")
   reason (known (get facts "composition_override_reason"))
   {:keys [valid value]} (composition-overrides facts)]
  (cond-> [] (nil? raw) (conj "composition_overrides(required for template)") (and (some? raw) (not valid)) (conj "composition_overrides(valid unique routing axes)") (and valid (not= (boolean (seq value)) (boolean reason))) (conj "composition_override_reason(exactly when overrides nonempty)"))))

(defn bespoke-evidence-defects [facts]
  (let [reason (known (get facts "bespoke_reason"))
   promotion (get facts "promotion_candidate")
   fingerprint (known (get facts "composition_contract_sha256"))
   version (known (get facts "composition_contract_fingerprint_version"))
   domain (known (get facts "composition_contract_fingerprint_domain"))]
  (cond-> [] (nil? reason) (conj "bespoke_reason") (not (contains? #{"true" "false"} promotion)) (conj "promotion_candidate(boolean)") (not (boolean (and fingerprint (re-matches #"^[0-9a-f]{64}$" fingerprint)))) (conj "composition_contract_sha256") (not= "v1" version) (conj "composition_contract_fingerprint_version(v1)") (not= "north:bespoke-contract:v1" domain) (conj "composition_contract_fingerprint_domain(north:bespoke-contract:v1)"))))

(defn identity-defects
  "Return every missing or contradictory proof for a managed lane, including a\n  commit marker matching the current canonical projection." [facts]
  (let [missing (remove (fn [__north_anon_1] (known (get facts __north_anon_1))) required-identity-predicates)
   kind (get facts "kind")
   composition-kind (get facts "composition_kind")
   role (get facts "role")
   live-input (known (get facts "live_input"))
   live-input-state (known (get facts "live_input_state"))
   live-input-epoch (known (get facts "live_input_epoch"))
   composition-id (get facts "composition_id")
   marker (known (get facts "identity_manifest_sha256"))
   conflicts (seq (get facts conflict-key))]
  (vec (distinct (mapcat identity [missing (if conflicts (do
  [(str "single-valued identity predicates(" (str/join "," (sort conflicts)) ")")])) (if (and (some? kind) (not= "lane" kind)) (do
  ["kind(lane)"])) (if (and (some? composition-kind) (not (contains? #{"template" "bespoke"} composition-kind))) (do
  ["composition_kind(template|bespoke)"])) (if (and (some? role) (not (safe-role-id? role))) (do
  ["role(safe Agent Machinery id)"])) (if (and live-input (not (contains? #{"streaming" "turn-messages" "unsupported"} live-input))) (do
  ["live_input(streaming|turn-messages|unsupported)"])) (if (and live-input-state (not (contains? #{"pending" "armed" "frozen"} live-input-state))) (do
  ["live_input_state(pending|armed|frozen)"])) (if (and (= "unsupported" live-input) live-input-state (not= "frozen" live-input-state)) (do
  ["live_input_state(frozen when unsupported)"])) (if (and live-input-epoch (nil? (re-matches #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$" live-input-epoch))) (do
  ["live_input_epoch(UUIDv4)"])) (if (and (some? composition-id) (not (safe-role-id? composition-id))) (do
  ["composition_id(safe Agent Machinery id)"])) (case composition-kind
    "template" (preset-evidence-defects facts)
    "bespoke" (bespoke-evidence-defects facts)
    []) (if (and marker (not= marker (manifest-sha256 facts))) (do
  ["identity_manifest_sha256(matches current projection)"]))])))))

(defn managed-valid? [facts]
  (empty? (identity-defects facts)))

(defn orchestration-provenance
  "Exact public composition-provenance ABI. Native provider sessions are honest\n  absence; malformed or uncommitted lanes are migration/corruption debt." [facts]
  (let [kind (get facts "kind")
   composition-kind (get facts "composition_kind")
   composition-id (get facts "composition_id")]
  (cond
  (= kind "session") "orchestration:not-selected"
  (not (managed-valid? facts)) "orchestration:legacy-debt"
  (= composition-kind "template") (let [{:keys [value]} (composition-overrides facts)
   base (str "orchestration:" composition-id)]
  (if (seq value) (str base "+override(" (str/join "," value) ")") base))
  (= composition-kind "bespoke") (str "orchestration:bespoke:" composition-id)
  :else "orchestration:legacy-debt")))

(defn provenance-detail [facts]
  (let [kind (get facts "composition_kind")
   {:keys [value]} (composition-overrides facts)]
  (cond-> {:label (orchestration-provenance facts) :kind kind} (= kind "template") (assoc :overrides value :override-reason (known (get facts "composition_override_reason"))) (= kind "bespoke") (assoc :why (known (get facts "bespoke_reason")) :nearest-reference-only (known (get facts "nearest_template")) :promotion-candidate (get facts "promotion_candidate") :contract-sha256 (known (get facts "composition_contract_sha256")) :contract-fingerprint-version (known (get facts "composition_contract_fingerprint_version")) :contract-fingerprint-domain (known (get facts "composition_contract_fingerprint_domain"))))))
