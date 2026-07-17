#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def tmp (.toFile (java.nio.file.Files/createTempDirectory "north-routing-report" (make-array java.nio.file.attribute.FileAttribute 0))))
(def coord (io/file tmp "coordination.log"))
(def telem (io/file tmp "telemetry.log"))

(defn fact [file l p r]
  (spit file (str (pr-str {:op "assert" :l l :p p :r r}) "\n") :append true))

(defn run! [kind]
  (let [result (proc/shell {:out :string :err :string :extra-env {"FRAM_LOG" (.getPath coord)
                                                                  "FRAM_TELEMETRY_LOG" (.getPath telem)}}
                           "bb" (str root "/cli/routing-report.clj") "report" kind "--json")]
    (when-not (zero? (:exit result)) (throw (ex-info (:err result) result)))
    (json/parse-string (str/trim (:out result)) true)))

(defn check [label ok]
  (println (str (if ok "ok:   " "FAIL: ") label))
  (when-not ok (System/exit 1)))

(def fingerprint-version "v1")
(def fingerprint-domain "north:bespoke-contract:v1")
(def hash-a (apply str (repeat 64 "a")))
(def hash-b (apply str (repeat 64 "b")))
(def hash-c (apply str (repeat 64 "c")))
(def hash-d (apply str (repeat 64 "d")))
(def hash-e (apply str (repeat 64 "e")))
(def hash-f (apply str (repeat 64 "f")))

(defn verified-thread! [id]
  (fact coord id "title" (str "Thread " id))
  (fact coord id "outcome" "landed")
  (fact coord id "done_when" "tests pass")
  (fact coord id "bar_evidence" "tests pass → exit 0"))

(doseq [id ["@thread-a" "@thread-b" "@thread-d" "@thread-e" "@thread-f"
            "@thread-g" "@thread-h" "@thread-i" "@thread-j"
            "@thread-legacy-a" "@thread-legacy-b"]]
  (verified-thread! id))

(defn run-facts! [run thread provider outcome tokens]
  (fact telem run "kind" "run")
  (fact telem run "agent" (str "agent-" (subs run 5)))
  (fact telem run "thread" thread)
  (fact telem run "provider" provider)
  (fact telem run "requested_tier" "senior")
  (fact telem run "role" "migration-forensics")
  (fact telem run "task_grade" "staff")
  (fact telem run "outcome" outcome)
  (when tokens (fact telem run "tokens" tokens))
  (fact telem run "duration_ms" "1000"))

(defn requested-fingerprint! [run hash version domain]
  (let [identity (str "@agent:agent-" (subs run 5))]
    (when hash (fact coord identity "composition_contract_sha256" hash))
    (when version (fact coord identity "composition_contract_fingerprint_version" version))
    (when domain (fact coord identity "composition_contract_fingerprint_domain" domain))))

(defn applied-bespoke!
  ([run composition-id hash domains] (applied-bespoke! run composition-id hash domains true))
  ([run composition-id hash domains emit-domain-count?]
   (fact telem run "composition_kind" "bespoke")
   (fact telem run "composition_id" composition-id)
   (fact telem run "nearest_preset" "analyst")
   (fact telem run "bespoke_reason" "PRIVATE RATIONALE CANARY: provenance plus schema recovery")
   (fact telem run "promotion_candidate" "true")
   (when hash
     (fact telem run "applied_bespoke_contract_sha256" hash)
     (fact telem run "applied_bespoke_contract_fingerprint_version" fingerprint-version)
     (fact telem run "applied_bespoke_contract_fingerprint_domain" fingerprint-domain)
     (requested-fingerprint! run hash fingerprint-version fingerprint-domain))
   ;; Reverse input order on selected fixtures; the report must restore Gaffer's
   ;; canonical vocabulary order before building the semantic variant key.
   (doseq [capability (if (= run "@run-b")
                        ["web" "filesystem.read" "filesystem.search"]
                        ["filesystem.search" "filesystem.read" "web"])]
     (fact telem run "applied_capability" capability))
   (fact telem run "applied_task_grade" "staff")
   (fact telem run "applied_topology" "worker")
   (fact telem run "applied_routing_tier" "senior")
   (fact telem run "applied_reasoning" "high")
   (fact telem run "applied_posture" "preserve")
   (doseq [domain domains] (fact telem run "applied_domain_requirement" domain))
   (when emit-domain-count?
     (fact telem run "applied_domain_requirement_count" (str (count domains))))))

(doseq [[run thread provider outcome tokens] [["@run-a" "thread-a" "openai" "ran" "100"]
                                               ["@run-b" "thread-b" "anthropic" "ran" "200"]
                                               ["@run-c" "(ad-hoc)" "openai" "died" "50"]
                                               ["@run-unknown" "(ad-hoc)" "anthropic" "died" nil]]]
  (run-facts! run thread provider outcome tokens))

;; Same applied contract + effective axes, deliberately different improvised IDs.
(applied-bespoke! "@run-a" "migration-forensics" hash-a [" NIX " "Beagle"])
(applied-bespoke! "@run-b" "schema-archaeologist" hash-a ["beagle " "nix"])

(let [performance (run! "performance")
      usage (run! "usage")
      promotions (run! "promotions")
      cohorts (:cohorts performance)
      candidate (first (:compositions promotions))]
  (check "performance separates operational runs from verified evidence"
         (and (= 4 (:runs performance)) (= 2 (reduce + (map :verifiedEvidence cohorts)))
              (= 4 (reduce + (map :runs cohorts)))))
  (check "performance carries an explicit non-causal quality disclaimer"
         (str/includes? (:claim performance) "not causal model quality"))
  (check "usage is subscription-safe and contains no dollar measure"
         (and (= "observed work, never dollars or API credits" (:unit usage))
              (= 350 (reduce + (keep :tokens (:providers usage))))
              (= 3 (reduce + (map :tokenRuns (:providers usage))))
              (nil? (:cost usage))))
  (check "bespoke recurrence is surfaced only as a human review candidate"
         (and (= ["migration-forensics" "schema-archaeologist"] (:compositionIds candidate))
              (= 2 (:distinctThreads candidate)) (= 2 (:qualifiedThreads candidate))
              (= "review-candidate" (:reviewStatus candidate))
              (= hash-a (:appliedContractSha256 candidate))
              (= ["filesystem.read" "filesystem.search" "web"] (:appliedCapabilities candidate))
              (= ["beagle" "nix"] (get-in candidate [:effectiveAxes :domains]))
              (= 2 (:appliedDomainRequirementCount candidate))
              (= ["matched"] (:requestedAppliedIntegrity candidate))
              (:hasAliasEvidence candidate)
              (str/includes? (:note candidate) "never promotes")))
  (check "promotion JSON declares the canonical fingerprint version/domain"
         (and (= fingerprint-version (:fingerprintVersion promotions))
              (= fingerprint-domain (:fingerprintDomain promotions))
              (str/includes? (:claim promotions) "effective routing axes")))
  (check "promotion output never exposes bespoke rationale text"
         (not (str/includes? (json/generate-string promotions) "PRIVATE RATIONALE CANARY"))))

;; Same ID, three different effective variants: hash drift and domain drift must
;; split rather than borrow recurrence from the established hash-a/nix+beagle row.
(run-facts! "@run-d" "thread-d" "openai" "ran" nil)
(applied-bespoke! "@run-d" "migration-forensics" hash-b ["nix" "beagle"])
(run-facts! "@run-e" "thread-e" "openai" "ran" nil)
(applied-bespoke! "@run-e" "migration-forensics" hash-a ["database"])

;; Two apparently recurrent legacy rows share an ID but lack applied hashes.
;; They must remain two one-run debt records, never one recurrent composition.
(run-facts! "@run-legacy-a" "thread-legacy-a" "anthropic" "ran" nil)
(applied-bespoke! "@run-legacy-a" "old-improvisation" nil ["nix"])
(run-facts! "@run-legacy-b" "thread-legacy-b" "anthropic" "ran" nil)
(applied-bespoke! "@run-legacy-b" "old-improvisation" nil ["nix"])

;; Two threads are not qualified recurrence when only one run both ran and has
;; verified thread evidence.
(run-facts! "@run-f" "thread-f" "openai" "ran" nil)
(applied-bespoke! "@run-f" "partial-recurrence" hash-c ["nix"])
(run-facts! "@run-g" "thread-g" "openai" "died" nil)
(applied-bespoke! "@run-g" "partial-recurrence" hash-c ["nix"])

;; Requested identity and applied prompt evidence must agree. A mismatch is hard
;; debt even when every other applied field is complete.
(run-facts! "@run-h" "thread-h" "openai" "ran" nil)
(applied-bespoke! "@run-h" "fingerprint-mismatch" hash-d [])
(requested-fingerprint! "@run-h" hash-a fingerprint-version fingerprint-domain)

;; Empty domains are valid only with explicit zero proof; missing or inconsistent
;; counts are evidence debt, not an empty semantic axis.
(run-facts! "@run-i" "thread-i" "openai" "ran" nil)
(applied-bespoke! "@run-i" "domain-count-mismatch" hash-e ["nix"])
(fact telem "@run-i" "applied_domain_requirement_count" "2")
(run-facts! "@run-j" "thread-j" "openai" "ran" nil)
(applied-bespoke! "@run-j" "domain-count-missing" hash-f [] false)

(let [promotions (run! "promotions")
      rows (:compositions promotions)
      recurrent (first (filter #(= hash-a (:appliedContractSha256 %)) rows))
      hash-drift (first (filter #(= hash-b (:appliedContractSha256 %)) rows))
      domain-drift (first (filter #(= ["database"] (get-in % [:effectiveAxes :domains])) rows))
      legacy (filter :legacyDebt rows)
      missing-hash-debt (filter #(some #{"missing-applied-hash"} (:legacyDebtReasons %)) rows)
      partial (first (filter #(= hash-c (:appliedContractSha256 %)) rows))
      fingerprint-debt (first (filter #(some #{"requested-applied-fingerprint-mismatch"}
                                             (:legacyDebtReasons %)) rows))
      count-mismatch (first (filter #(some #{"applied-domain-count-mismatch"}
                                          (:legacyDebtReasons %)) rows))
      count-missing (first (filter #(some #{"missing-applied-domain-count"}
                                         (:legacyDebtReasons %)) rows))]
  (check "applied hash and normalized domains split semantic variants"
         (and (= 1 (:qualifiedThreads hash-drift))
              (= 1 (:qualifiedThreads domain-drift))
              (= "insufficient-ran-verified-recurrence" (:reviewStatus hash-drift))
              (= "insufficient-ran-verified-recurrence" (:reviewStatus domain-drift))))
  (check "same semantic variant exposes alias evidence while reused IDs expose drift"
         (and (:hasAliasEvidence recurrent)
              (:hasDriftEvidence recurrent)
              (= ["migration-forensics"] (:driftedCompositionIds recurrent))))
  (check "missing applied hashes are isolated as per-run legacy debt"
         (and (= 2 (count missing-hash-debt))
              (every? #(and (= 1 (:runs %)) (false? (:recurrent %))
                            (= ["gaffer:legacy-debt"] (:compositionLabels %))
                            (= "legacy-debt" (:reviewStatus %)))
                      missing-hash-debt)))
  (check "failed or unverified rows cannot manufacture qualified recurrence"
         (and (= 2 (:distinctThreads partial)) (= 1 (:qualifiedThreads partial))
              (= "insufficient-ran-verified-recurrence" (:reviewStatus partial))))
  (check "requested/applied fingerprint mismatch is hard per-run debt"
         (and fingerprint-debt (= ["mismatch"] (:requestedAppliedIntegrity fingerprint-debt))
              (= "legacy-debt" (:reviewStatus fingerprint-debt))))
  (check "missing and inconsistent domain-count proofs are hard debt"
         (and count-mismatch count-missing
              (= "legacy-debt" (:reviewStatus count-mismatch))
              (= "legacy-debt" (:reviewStatus count-missing)))))

(try
  (let [out (:out (proc/shell {:out :string :err :string :extra-env {"FRAM_LOG" (.getPath coord)
                                                                      "FRAM_TELEMETRY_LOG" (.getPath telem)}}
                               (str root "/bin/north") "routing" "report" "performance"))]
    (check "north routing report is wired through the public CLI" (str/includes? out "ROUTING PERFORMANCE")))
  (finally
    (doseq [file (reverse (file-seq tmp))] (io/delete-file file true))))
