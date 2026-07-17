#!/usr/bin/env bb
;; Evidence-aware routing feedback. Operational completion is reported separately
;; from thread verification; evidence coverage is never presented as model quality.

(require '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def multi-preds #{"done_when" "bar_evidence" "domain_requirement"
                   "applied_capability" "applied_domain_requirement"})

(def canonical-gaffer-capabilities
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
  {:taskGrade #{"novice" "junior" "mid" "senior" "staff" "principal" "research-grade"}
   :topology #{"worker" "orchestrator"}
   :tier #{"economy" "standard" "senior" "frontier"}
   :reasoning #{"low" "medium" "high" "xhigh" "max"}
   :posture #{"explore" "deliver" "preserve"}})
(def sha256-pattern #"^[0-9a-f]{64}$")
(def safe-role-id-pattern #"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")

(defn default-paths []
  (let [home (System/getenv "HOME")
        dir (str home "/.local/state/north")
        split (io/file dir "coordination.log")]
    [(or (System/getenv "FRAM_LOG")
         (if (.exists split) (.getPath split) (str dir "/facts.log")))
     (or (System/getenv "FRAM_TELEMETRY_LOG")
         (let [path (str dir "/telemetry.log")] (when (.exists (io/file path)) path)))]))

(defn read-ops [paths]
  (mapcat (fn [path]
            (if (and path (.exists (io/file path)))
              (with-open [reader (io/reader path)]
                (doall
                 (keep (fn [line]
                         (try (edn/read-string line) (catch Exception _ nil)))
                       (line-seq reader))))
              []))
          (distinct (remove nil? paths))))

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
(defn thread-ref [value]
  (when (and value (not= value "(ad-hoc)"))
    (if (str/starts-with? value "@") value (str "@" value))))

(defn normalized-token [value]
  (let [token (some-> value str str/trim)] (when (seq token) token)))

(defn normalized-domain [value]
  (some-> (normalized-token value)
          (java.text.Normalizer/normalize java.text.Normalizer$Form/NFC)
          (.toLowerCase java.util.Locale/ROOT)))

(defn normalized-domains [values]
  (->> values (keep normalized-domain) distinct sort vec))

(defn capability-summary [values]
  (let [normalized (->> values (keep normalized-token) distinct vec)
        requested (set normalized)
        unknown (->> normalized (remove (set canonical-gaffer-capabilities)) sort vec)]
    {:canonical (vec (filter requested canonical-gaffer-capabilities))
     :unknown unknown}))

(defn evidence [facts thread]
  (if-not thread
    {:status "no-contract" :bars 0 :evidenced 0 :hasOutcome false}
    (let [bars (many facts thread "done_when")
          evs (many facts thread "bar_evidence")
          outcome? (boolean (one facts thread "outcome"))
          evidenced (count (filter (fn [bar] (some #(str/includes? (str %) (str bar)) evs)) bars))
          total (count bars)
          status (cond
                   (zero? total) "no-contract"
                   (and outcome? (= evidenced total)) "verified"
                   (= evidenced total) "evidenced-open"
                   (pos? evidenced) "partial"
                   :else "unevidenced")]
      {:status status :bars total :evidenced evidenced :hasOutcome outcome?})))

(defn run-rows [facts]
  (for [[entity predicates] facts
        :when (and (= "run" (one facts entity "kind"))
                   (str/starts-with? entity "@run-"))]
    (let [agent (one facts entity "agent")
          identity (str "@agent:" agent)
          get' (fn [pred fallback] (or (one facts entity pred) (one facts identity pred) fallback))
          thread (thread-ref (one facts entity "thread"))
          composition-kind (get' "composition_kind" nil)
          composition-id (normalized-token (get' "composition_id" nil))
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
          applied-domain-values (many facts entity "applied_domain_requirement")
          applied-domain-count-raw (one facts entity "applied_domain_requirement_count")
          applied-domain-count (maybe-long applied-domain-count-raw)
          effective-axes (assoc
                          (into {} (map (fn [[axis pred]]
                                         [axis (normalized-token (one facts entity pred))])
                                       applied-axis-preds))
                          :domains (normalized-domains
                                    applied-domain-values))
          missing-axes (->> applied-axis-preds
                            (keep (fn [[axis _]] (when-not (get effective-axes axis) (name axis))))
                            vec)
          invalid-axes (->> applied-axis-values
                            (keep (fn [[axis values]]
                                    (let [value (get effective-axes axis)]
                                      (when (and value (not (values value))) (name axis)))))
                            sort vec)
          legacy-debt (vec
                       (concat
                        (cond
                          (nil? applied-hash) ["missing-applied-hash"]
                          (not (re-matches sha256-pattern applied-hash)) ["invalid-applied-hash"]
                          :else [])
                        (when (not= bespoke-fingerprint-version applied-version)
                          ["missing-or-unsupported-applied-fingerprint-version"])
                        (when (not= bespoke-fingerprint-domain applied-domain)
                          ["missing-or-unsupported-applied-fingerprint-domain"])
                        (cond
                          (nil? applied-domain-count-raw) ["missing-applied-domain-count"]
                          (or (nil? applied-domain-count) (neg? applied-domain-count))
                          ["invalid-applied-domain-count"]
                          (not= applied-domain-count (count applied-domain-values))
                          ["applied-domain-count-mismatch"]
                          :else [])
                        (when (contains? #{"mismatch" "incomplete-requested-evidence"}
                                         requested-integrity)
                          [(str "requested-applied-fingerprint-" requested-integrity)])
                        (when (empty? applied-capabilities) ["missing-applied-capabilities"])
                        (when (seq (:unknown capability-evidence)) ["noncanonical-applied-capabilities"])
                        (when (seq missing-axes)
                          [(str "missing-applied-axes:" (str/join "," missing-axes))])
                        (when (seq invalid-axes)
                          [(str "invalid-applied-axes:" (str/join "," invalid-axes))])
                        (when (and (= "bespoke" composition-kind)
                                   (not (and composition-id
                                             (re-matches safe-role-id-pattern composition-id))))
                          ["missing-or-invalid-bespoke-composition-id"])))]
      {:entity entity :thread thread
       :provider (get' "provider" "?") :tier (get' "requested_tier" "?")
       :model (get' "model" "?") :effort (get' "effort" "?")
       :role (get' "role" "?") :taskGrade (get' "task_grade" "?")
       :outcome (get' "outcome" "?") :tokens (maybe-long (get' "tokens" nil))
       :durationMs (long' (get' "duration_ms" 0)) :turns (long' (get' "num_turns" 0))
       :fallbacks (long' (get' "fallback_count" 0))
       :escalations (long' (get' "escalation_count" 0))
       :compositionKind composition-kind
       :compositionId composition-id
       :nearestPreset (get' "nearest_preset" nil)
       :bespokeReason (get' "bespoke_reason" nil)
       :promotionCandidate (= "true" (get' "promotion_candidate" "false"))
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
       :appliedDomainRequirementCount applied-domain-count
       :effectiveAxes effective-axes
       :legacyDebtReasons legacy-debt
       :evidence (evidence facts thread)})))

(def cohort-fields [:provider :tier :role :taskGrade])
(defn cohort-label [row] (str/join "/" (map #(get row %) cohort-fields)))

(defn performance-row [[label rows]]
  (let [statuses (frequencies (map #(get-in % [:evidence :status]) rows))]
    {:cohort label :runs (count rows) :operationalRan (count (filter #(= "ran" (:outcome %)) rows))
     :threadOutcomes (count (filter #(get-in % [:evidence :hasOutcome]) rows))
     :verifiedEvidence (get statuses "verified" 0) :partialEvidence (get statuses "partial" 0)
     :unevidenced (get statuses "unevidenced" 0) :noContract (get statuses "no-contract" 0)
     :evidencedOpen (get statuses "evidenced-open" 0)
     :escalated (count (filter #(pos? (:escalations %)) rows))}))

(defn performance-report [rows]
  {:report "performance" :claim "operational outcomes plus current thread evidence coverage; not causal model quality"
   :runs (count rows)
   :cohorts (->> rows (group-by cohort-label) (map performance-row) (sort-by :cohort) vec)})

(defn usage-row [[provider rows]]
  (let [tokens (keep :tokens rows)]
    {:provider provider :runs (count rows)
     :tokens (when (seq tokens) (reduce + tokens)) :tokenRuns (count tokens)
     :wallSeconds (long (/ (reduce + (map :durationMs rows)) 1000))
     :turns (reduce + (map :turns rows)) :fallbacks (reduce + (map :fallbacks rows))
     :escalatedRuns (count (filter #(pos? (:escalations %)) rows))}))

(defn usage-report [rows]
  {:report "usage" :unit "observed work, never dollars or API credits"
   :runs (count rows) :providers (->> rows (group-by :provider) (map usage-row) (sort-by :provider) vec)})

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
        verified (count (filter #(= "verified" (get-in % [:evidence :status])) rows))
        qualified (filter #(and (= "ran" (:outcome %))
                                (= "verified" (get-in % [:evidence :status]))
                                (:thread %))
                          rows)
        qualified-threads (set (map :thread qualified))
        flagged (some :promotionCandidate rows)
        debt (vec (sort (set (mapcat :legacyDebtReasons rows))))
        legacy? (boolean (seq debt))
        recurrent (and (not legacy?) (>= (count qualified-threads) 2))
        review-status (cond
                        legacy? "legacy-debt"
                        (not flagged) "not-requested"
                        (not recurrent) "insufficient-ran-verified-recurrence"
                        :else "review-candidate")
        composition-ids (vec (sort (set (keep :compositionId rows))))
        labels (if legacy?
                 ["gaffer:legacy-debt"]
                 (mapv #(str "gaffer:bespoke:" %) composition-ids))
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
     :operationalRan (count (filter #(= "ran" (:outcome %)) rows))
     :verifiedEvidence verified :promotionRequested (boolean flagged)
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

(defn report [kind rows]
  (case kind
    "performance" (performance-report rows)
    "usage" (usage-report rows)
    "promotions" (promotions-report rows)
    (throw (ex-info "usage: north routing report [performance|usage|promotions] [--json]" {}))))

(defn print-table [data]
  (case (:report data)
    "performance"
    (do (println "ROUTING PERFORMANCE — operational outcomes + verification coverage")
        (println "Evidence is joined from current thread done_when/bar_evidence/outcome facts; it is not causal model quality.")
        (println (format "%-38s %5s %5s %5s %5s %5s %5s" "COHORT provider/tier/role/grade" "runs" "ran" "ver" "part" "none" "esc"))
        (doseq [row (:cohorts data)]
          (println (format "%-38s %5d %5d %5d %5d %5d %5d" (:cohort row) (:runs row)
                           (:operationalRan row) (:verifiedEvidence row) (:partialEvidence row)
                           (+ (:unevidenced row) (:noContract row)) (:escalated row)))))
    "usage"
    (do (println "ROUTING USAGE — observed work (never dollars or API credits)")
        (println (format "%-12s %6s %12s %10s %8s %9s %9s" "PROVIDER" "runs" "tokens" "wall-s" "turns" "fallbacks" "escalated"))
        (doseq [row (:providers data)]
          (println (format "%-12s %6d %12s %10d %8d %9d %9d" (:provider row) (:runs row)
                           (if (some? (:tokens row)) (str (:tokens row)) "?")
                           (:wallSeconds row) (:turns row) (:fallbacks row) (:escalatedRuns row)))))
    "promotions"
    (do (println "ROUTING PROMOTIONS — bespoke recurrence for human review")
        (println "Variants use applied canonical hash + capabilities + effective axes; missing hashes are per-run legacy debt.")
        (println "This report never changes Gaffer's standard library.")
        (if (empty? (:compositions data)) (println "  (no bespoke compositions observed)")
          (doseq [row (:compositions data)]
            (let [label (str/join "," (:compositionLabels row))
                  hash (or (:appliedContractSha256 row) "missing")
                  capabilities (str/join "," (or (:appliedCapabilities row) []))]
              (println (format "%-34s threads=%d runs=%d verified=%d  %s"
                               label (:distinctThreads row) (:runs row)
                               (:verifiedEvidence row) (:reviewStatus row)))
              (println (str "  hash=" hash " capabilities=" capabilities))
              (println "  requested↔applied="
                       (str/join "," (:requestedAppliedIntegrity row)))
              (when-let [axes (:effectiveAxes row)] (println "  axes=" (pr-str axes)))
              (when (:hasAliasEvidence row)
                (println "  aliases=" (str/join "," (:aliasCompositionIds row))))
              (when (:hasDriftEvidence row)
                (println "  drift=" (str/join "," (:driftedCompositionIds row))))
              (when (:legacyDebt row)
                (println "  debt=" (str/join "," (:legacyDebtReasons row))))))))))

(defn -main [& args]
  (let [[verb kind & flags] args]
    (when-not (= verb "report")
      (binding [*out* *err*] (println "usage: north routing report [performance|usage|promotions] [--json]"))
      (System/exit 2))
    (let [facts (fold-facts (read-ops (default-paths)))
          data (report (or kind "performance") (run-rows facts))]
      (if (some #{"--json"} flags)
        (println (json/generate-string data))
        (print-table data)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
