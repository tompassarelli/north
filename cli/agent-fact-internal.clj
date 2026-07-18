#!/usr/bin/env bb
;; Harness-owned agent identity writer. This file is deliberately not routed by
;; `north` or MCP. Its surface is typed and fail-closed: one safe @agent subject,
;; five exact operations, and an exhaustive predicate vocabulary. It is an
;; application-integrity boundary, not same-UID hostile-process isolation.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))

(def marker-predicate "identity_manifest_sha256")
(def terminal-marker-predicate "terminal_manifest_sha256")
(def base-terminal-predicates
  #{"outcome" "process_outcome" "delivery_outcome" "delivery_reason"})
(def delivery-proof-predicates
  #{"delivery_evidence" "delivery_evidence_sha256"
    "delivery_attestation" "delivery_attestation_sha256"})
(def terminal-predicates
  (into base-terminal-predicates delivery-proof-predicates))
(def terminal-publication-order
  ;; Readers treat process_outcome without the marker as a partial new-style
  ;; publication. Keep the legacy outcome alias last so it cannot masquerade as
  ;; a complete legacy terminal while a new projection is still being written.
  ["process_outcome" "delivery_evidence" "delivery_evidence_sha256"
   "delivery_attestation" "delivery_attestation_sha256"
   "delivery_outcome" "delivery_reason" "outcome"])
(def terminal-retraction-order
  ;; Once the marker is gone, remove the legacy alias before process_outcome.
  ;; The remaining process_outcome forces modern validation to fail closed; if
  ;; process_outcome disappeared first, a crash could expose stale outcome as a
  ;; valid legacy singleton.
  ["outcome" "process_outcome" "delivery_outcome" "delivery_reason"
   "delivery_attestation_sha256" "delivery_attestation"
   "delivery_evidence_sha256" "delivery_evidence"])
(def route-authority-predicates #{"provider" "provider_target" "model" "effort"})
(def projection-predicates #{"display_handle" "display_name"})
(def route-predicates (into route-authority-predicates projection-predicates))
(def identity-predicates
  (into route-authority-predicates
        #{"kind" "role" "composition_kind" "composition_id"
          "composition_overrides" "composition_override_reason" "nearest_preset"
          "bespoke_reason" "promotion_candidate" "composition_contract_sha256"
          "composition_contract_fingerprint_version" "composition_contract_fingerprint_domain"
          "repo" "goal" "coordinator" "spawned_at"}))
(def publish-predicates (into identity-predicates projection-predicates))
(def required-identity-predicates
  #{"kind" "role" "model" "provider" "provider_target" "effort"
    "composition_kind" "composition_id" "repo" "goal" "spawned_at"
    "display_handle" "display_name"})

(defn fail! [message data]
  (throw (ex-info message data)))

(defn checked! [result operation]
  (when (:reject result) (fail! "coordinator rejected harness identity write" {:operation operation}))
  result)

(defn entity [subject]
  (let [raw (str/replace (str subject) #"^@?agent:" "")
        canonical (str "@agent:" raw)]
    (when-not (north.terminal-projection/valid-agent-entity? canonical)
      (fail! "invalid managed agent id" {:subject subject}))
    canonical))

(defn payload [raw]
  (let [parsed (try (json/parse-string (str raw))
                    (catch Exception e
                      (fail! "invalid managed identity JSON" {:cause (.getMessage e)})))]
    (when-not (map? parsed) (fail! "managed identity payload must be an object" {}))
    (into (sorted-map)
          (map (fn [[predicate value]]
                 (when-not (and (string? predicate) (string? value) (not (str/blank? value)))
                   (fail! "managed identity facts must be nonblank strings"
                          {:predicate predicate :value-type (type value)}))
                 [predicate value]))
          parsed)))

(defn facts-of [port subject]
  (let [rows (:ok (north.coord/send-op
                   port {:op :query
                         :query {:find "identity_fact"
                                 :rules [{:head {:rel "identity_fact"
                                                 :args [{:var "p"} {:var "r"}]}
                                          :body [{:rel "triple"
                                                  :args [subject {:var "p"} {:var "r"}]}]}]}}))]
    (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))

(defn canonical-record [record]
  (json/generate-string (into (sorted-map) record)))

(defn retract-values! [port subject predicate values]
  (doseq [value values]
    (checked! (north.coord/retract! port subject predicate value)
              [:retract subject predicate value])))

(defn put-facts! [port subject facts]
  (doseq [[predicate value] facts]
    (checked! (north.coord/put! port subject predicate value)
              [:put subject predicate value])))

(defn exact-projection [facts predicates]
  (into (sorted-map)
        (keep (fn [predicate]
                (when-let [values (seq (get facts predicate))]
                  [predicate (set values)])))
        predicates))

(defn desired-projection [facts]
  (into (sorted-map) (map (fn [[predicate value]] [predicate #{value}])) facts))

(defn canonical [facts]
  (apply str (map (fn [[predicate value]] (str predicate "\u0000" value "\n")) facts)))

(defn sha256 [s]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256")
                        (.getBytes (str s) java.nio.charset.StandardCharsets/UTF_8))]
    (format "%064x" (java.math.BigInteger. 1 digest))))

(defn verify-exact! [port subject desired predicates]
  (let [actual (exact-projection (facts-of port subject) predicates)
        expected (desired-projection desired)]
    (when-not (= expected actual)
      (fail! "managed identity readback did not match the published projection"
             {:expected expected :actual actual}))))

(defn singleton-projection! [facts predicates]
  (doseq [predicate predicates
          :let [values (get facts predicate #{})]
          :when (> (count values) 1)]
    (fail! "managed identity contains multiple live values for one predicate"
           {:predicate predicate :values values})))

(defn validate-publish! [facts]
  (let [unknown (seq (remove publish-predicates (keys facts)))
        missing (seq (remove #(contains? facts %) required-identity-predicates))]
    (when unknown (fail! "unsupported managed identity predicate" {:predicates unknown}))
    (when missing (fail! "incomplete managed identity projection" {:predicates missing}))
    (when-not (= "lane" (get facts "kind"))
      (fail! "managed SDK identity kind must be lane" {:kind (get facts "kind")}))
    (when-not (= (get facts "role") (get facts "composition_id"))
      (fail! "managed role and Gaffer composition id must agree"
             {:role (get facts "role") :composition-id (get facts "composition_id")}))
    (case (get facts "composition_kind")
      "preset" (do
                 (when-not (contains? facts "composition_overrides")
                   (fail! "preset identity requires composition_overrides" {}))
                 (when (some #(contains? facts %)
                             ["bespoke_reason" "promotion_candidate" "composition_contract_sha256"
                              "composition_contract_fingerprint_version"
                              "composition_contract_fingerprint_domain"])
                   (fail! "preset identity carries bespoke-only evidence" {})))
      "bespoke" (do
                  (doseq [predicate ["bespoke_reason" "promotion_candidate"
                                     "composition_contract_sha256"
                                     "composition_contract_fingerprint_version"
                                     "composition_contract_fingerprint_domain"]]
                    (when-not (contains? facts predicate)
                      (fail! "bespoke identity is missing authority evidence"
                             {:predicate predicate})))
                  (when-not (contains? #{"true" "false"} (get facts "promotion_candidate"))
                    (fail! "invalid bespoke promotion_candidate" {}))
                  (when-not (re-matches #"^[0-9a-f]{64}$"
                                        (get facts "composition_contract_sha256"))
                    (fail! "invalid bespoke contract fingerprint" {}))
                  (when-not (= "v1" (get facts "composition_contract_fingerprint_version"))
                    (fail! "unsupported bespoke contract fingerprint version" {}))
                  (when-not (= "north:bespoke-contract:v1"
                               (get facts "composition_contract_fingerprint_domain"))
                    (fail! "unsupported bespoke contract fingerprint domain" {})))
      (fail! "managed identity composition_kind must be preset or bespoke"
             {:composition-kind (get facts "composition_kind")}))))

(defn commit-marker! [port subject facts]
  ;; Display strings are derived caches, never authority. Roster/trace rebuild
  ;; names from provider/model/effort/composition, and legacy UI writers cannot
  ;; invalidate a committed identity by decorating a cached label.
  (let [authoritative (into (sorted-map) (select-keys facts identity-predicates))
        marker (sha256 (canonical authoritative))]
    (checked!
     (north.coord/assert-after-read!
      port subject marker-predicate marker
      (fn []
        ;; Every fact hashed by the marker is re-read after the global BASE is
        ;; captured. Display caches are intentionally outside this authority
        ;; projection. An identical concurrent marker is idempotent; a distinct
        ;; one proves competing generation publication and fails closed.
        (verify-exact! port subject authoritative identity-predicates)
        (let [markers (get (facts-of port subject) marker-predicate #{})]
          (when-not (or (empty? markers) (= #{marker} markers))
            (fail! "managed identity has a competing generation marker"
                   {:subject subject})))))
     [:assert-after-read subject marker-predicate marker])
    (when-not (= #{marker} (get (facts-of port subject) marker-predicate))
      (fail! "managed identity commit marker was not acknowledged"
             {:marker marker}))
    marker))

(defn terminal-marker! [port subject facts validate-current!]
  (let [marker (north.terminal-projection/terminal-manifest-sha256 facts)]
    (when-not marker
      (fail! "cannot commit an incomplete managed terminal projection" {}))
    (checked! (north.coord/assert-after-read!
               port subject terminal-marker-predicate marker validate-current!)
              [:assert-after-read subject terminal-marker-predicate marker])
    (when-not (= marker (north.coord/resolved port subject terminal-marker-predicate))
      (fail! "managed terminal commit marker was not acknowledged" {:marker marker}))
    marker))

(defn publish! [port subject facts]
  (validate-publish! facts)
  (let [before (facts-of port subject)]
    ;; A previous generation may have left any optional shape field or outcome.
    ;; Withdraw both generation markers deterministically before touching either
    ;; projection body; readers cannot mistake a partial identity rewrite or a
    ;; stale terminal for a committed current generation. Simultaneous reuse of
    ;; one id is unsupported.
    (retract-values! port subject marker-predicate (get before marker-predicate #{}))
    (retract-values! port subject terminal-marker-predicate
                     (get before terminal-marker-predicate #{}))
    (doseq [predicate terminal-retraction-order]
      (retract-values! port subject predicate (get before predicate #{})))
    (doseq [predicate (sort publish-predicates)]
      (retract-values! port subject predicate (get before predicate #{})))
    (put-facts! port subject facts)
    (verify-exact! port subject facts publish-predicates)
    (commit-marker! port subject facts)))

(defn validate-terminal! [subject facts]
  (let [predicates (set (keys facts))
        unknown (seq (remove terminal-predicates predicates))
        missing (seq (remove predicates base-terminal-predicates))]
    (when unknown
      (fail! "terminal carries unsupported predicates" {:predicates unknown}))
    (when missing
      (fail! "terminal is missing base process/delivery predicates" {:predicates missing})))
  (when-not (= (get facts "outcome") (get facts "process_outcome"))
    (fail! "legacy outcome must equal process_outcome" {}))
  (when-not (contains? #{"unverified" "blocked" "reported" "verified"}
                       (get facts "delivery_outcome"))
    (fail! "invalid delivery_outcome" {:delivery-outcome (get facts "delivery_outcome")}))
  (when-not (north.terminal-projection/delivery-projection-valid? facts)
    (fail! "delivery outcome lacks a valid proof projection"
           {:delivery-outcome (get facts "delivery_outcome")}))
  (when-let [evidence-raw (get facts "delivery_evidence")]
    (let [evidence (json/parse-string evidence-raw)]
      (when-not (= subject (get evidence "reporter"))
        (fail! "delivery reporter must be the managed terminal subject"
               {:subject subject :reporter (get evidence "reporter")}))))
  (when-let [attestation-raw (get facts "delivery_attestation")]
    (let [attestation (json/parse-string attestation-raw)]
      (when-not (= subject (get attestation "target"))
        (fail! "delivery attestation target must be the managed terminal subject"
               {:subject subject :target (get attestation "target")})))))

(defn validate-reported-run!
  "A syntactically valid snapshot is not proof by itself. Before exposing a
  reported lane terminal, bind it to the committed reservation and exact
  writer-scoped self-report already present on the named run subject."
  [port subject facts]
  (when (= "reported" (get facts "delivery_outcome"))
    (let [evidence (json/parse-string (get facts "delivery_evidence"))
          run (get evidence "run")
          thread (get evidence "thread")
          run-facts (facts-of port run)
          reservation-origin
          (north.terminal-projection/singleton-value
           run-facts "run_reservation_contract_origin")
          reservation-baseline
          (north.terminal-projection/run-reservation-done-when run-facts)
          current-bars
          (north.terminal-projection/canonical-done-when
           (facts-of port thread))
          cited-records
          (set
           (mapcat (fn [match]
                     (map canonical-record (get match "evidence")))
                   (get evidence "matches")))
          evidence-state
          (north.terminal-projection/run-evidence-state
           run-facts run thread subject)
          stored-records (:raws evidence-state)]
      (when-not (north.terminal-projection/run-reservation-valid? run-facts)
        (fail! "reported delivery requires a committed run reservation"
               {:subject subject :run run}))
      (when-not (= #{subject} (get run-facts "run_reservation_agent"))
        (fail! "reported delivery reservation agent mismatch"
               {:subject subject :run run}))
      (when-not (= #{thread} (get run-facts "run_reservation_thread"))
        (fail! "reported delivery reservation thread mismatch"
               {:subject subject :run run :thread thread}))
      (when-not (= reservation-origin (get evidence "contractOrigin"))
        (fail! "reported delivery contract origin differs from its reservation"
               {:subject subject :run run}))
      (when-not (= reservation-baseline (get evidence "baselineDoneWhen"))
        (fail! "reported delivery baseline differs from its reservation"
               {:subject subject :run run}))
      (when-not (= current-bars (get evidence "doneWhen"))
        (fail! "reported delivery contract changed before terminal publication"
               {:subject subject :run run :thread thread}))
      (when-not (:valid? evidence-state)
        (fail! "reported delivery run contains malformed, cross-scoped, duplicate, or excessive evidence"
               {:subject subject :run run}))
      (when-not (= stored-records cited-records)
        (fail! "reported delivery snapshot must cite the exact reserved-run evidence set"
               {:subject subject :run run
                :missing (vec (remove stored-records cited-records))
                :uncited (vec (remove cited-records stored-records))})))))

(defn publish-terminal! [port subject facts]
  (let [before (facts-of port subject)]
    (retract-values! port subject terminal-marker-predicate
                     (get before terminal-marker-predicate #{}))
    (doseq [predicate terminal-retraction-order]
      (retract-values! port subject predicate (get before predicate #{})))
    (doseq [predicate terminal-publication-order
            :let [value (get facts predicate)]]
      (when value
        (checked! (north.coord/put! port subject predicate value)
                  [:put subject predicate value])))
    ;; Capture the coordinator version before the load-bearing reads, then commit
    ;; the marker only against that exact version. Any concurrent done-bar, run,
    ;; or terminal mutation rejects the marker and re-runs both checks.
    (terminal-marker!
     port subject facts
     (fn []
       (verify-exact! port subject facts terminal-predicates)
       (validate-reported-run! port subject facts)))))

(defn terminal! [port subject facts]
  (validate-terminal! subject facts)
  (validate-reported-run! port subject facts)
  (publish-terminal! port subject facts))

(defn committed-managed-actor!
  [port raw-actor]
  (let [actor (entity raw-actor)
        before (facts-of port actor)]
    (singleton-projection! before (conj publish-predicates marker-predicate))
    (let [identity (into (sorted-map)
                         (keep (fn [predicate]
                                 (when-let [value (first (get before predicate))]
                                   [predicate value])))
                         publish-predicates)
          marker (first (get before marker-predicate))]
      (validate-publish! identity)
      (when-not (= marker
                   (sha256 (canonical (into (sorted-map)
                                             (select-keys identity identity-predicates)))))
        (fail! "attesting actor has no committed managed identity" {:actor actor}))
      (when-not (#{"verifier" "judge"} (get identity "role"))
        (fail! "delivery attestation requires a verifier or judge lane"
               {:actor actor :role (get identity "role")}))
      {:actor actor :role (get identity "role")})))

(defn attest!
  "Fail closed until verifier identity is backed by an isolation boundary rather
  than caller-controlled same-UID environment provenance."
  [port subject request]
  (fail! "independent delivery attestation unavailable under shared-UID lanes"
         {:subject subject :request-keys (set (keys request))
          :highest-supported-delivery-state "reported"}))

(defn update-route! [port subject facts]
  (let [unknown (seq (remove route-predicates (keys facts)))
        missing (seq (remove #(contains? facts %) route-predicates))]
    (when unknown (fail! "unsupported managed route predicate" {:predicates unknown}))
    (when missing (fail! "incomplete managed route projection" {:predicates missing})))
  ;; Route fallback is still pre-provider-side-effect. Withdraw the previous
  ;; commit marker before changing any route axis; a crash cannot leave a mixed
  ;; route looking like an acknowledged identity generation.
  (let [before (facts-of port subject)
        current (into (sorted-map)
                      (keep (fn [predicate]
                              (when-let [value (first (get before predicate))]
                                [predicate value])))
                      publish-predicates)
        marker (first (get before marker-predicate))]
    (singleton-projection! before (conj publish-predicates marker-predicate))
    (validate-publish! current)
    (when-not (= marker (sha256 (canonical (into (sorted-map)
                                                   (select-keys current identity-predicates)))))
      (fail! "cannot update an uncommitted or corrupted managed route" {}))
    (retract-values! port subject marker-predicate (get before marker-predicate #{}))
    ;; The coordinator defaults undeclared predicates to multi cardinality.
    ;; Never rely on descriptive pred registry rows to supersede executable
    ;; facts: explicitly clear every old route value before asserting the new
    ;; exact projection.
    (doseq [predicate route-predicates]
      (retract-values! port subject predicate (get before predicate #{}))))
  (put-facts! port subject facts)
  (verify-exact! port subject facts route-predicates)
  (let [current (facts-of port subject)
        identity (into (sorted-map)
                       (keep (fn [predicate]
                               (when-let [value (first (get current predicate))]
                                 [predicate value])))
                       publish-predicates)]
    (validate-publish! identity)
    (commit-marker! port subject identity)))

(defn retask! [port subject facts]
  (when-not (= #{"goal" "display_name"} (set (keys facts)))
    (fail! "retask requires exactly goal and display_name" {:predicates (keys facts)}))
  (let [before (facts-of port subject)
        current (into (sorted-map)
                      (keep (fn [predicate]
                              (when-let [value (first (get before predicate))]
                                [predicate value])))
                      publish-predicates)
        marker (first (get before marker-predicate))]
    (singleton-projection! before (conj publish-predicates marker-predicate))
    (validate-publish! current)
    (when-not (= marker (sha256 (canonical (into (sorted-map)
                                                   (select-keys current identity-predicates)))))
      (fail! "cannot retask an uncommitted or corrupted managed identity" {}))
    (retract-values! port subject marker-predicate (get before marker-predicate #{}))
    (doseq [predicate ["goal" "display_name"]]
      (retract-values! port subject predicate (get before predicate #{})))
    (put-facts! port subject facts)
    (verify-exact! port subject facts #{"goal" "display_name"})
    (let [after (facts-of port subject)
          projection (into (sorted-map)
                           (keep (fn [predicate]
                                   (when-let [value (first (get after predicate))]
                                     [predicate value])))
                           publish-predicates)]
      (validate-publish! projection)
      (commit-marker! port subject projection))))

(let [[port-s operation subject raw] *command-line-args*
      port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
      subject (entity subject)
      result (case operation
               "publish" (publish! port subject (payload raw))
               "route" (update-route! port subject (payload raw))
               "retask" (retask! port subject (payload raw))
               "terminal" (terminal! port subject (payload raw))
               "attest" (attest! port subject (payload raw))
               (fail! "internal agent fact operation must be publish, route, retask, terminal, or attest"
                      {:operation operation}))]
  (println (json/generate-string {:ok true :result result})))
