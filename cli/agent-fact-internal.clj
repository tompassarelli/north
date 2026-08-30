(ns user
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def writer-root (.getParent (io/file (or *file* (System/getProperty "babashka.file")))))

(load-file (str writer-root "/coord.clj"))

(load-file (str writer-root "/agent-provenance.clj"))

(load-file (str writer-root "/terminal-projection.clj"))

(load-file (str writer-root "/lifecycle-projection.clj"))

(def marker-predicate "identity_manifest_sha256")

(def terminal-marker-predicate "terminal_manifest_sha256")

(def base-terminal-predicates (set north.terminal-projection/terminal-predicates))

(def delivery-proof-predicates (set north.terminal-projection/delivery-proof-predicates))

(def terminal-predicates (disj north.agent-provenance/terminal-predicates terminal-marker-predicate))

(def terminal-publication-order ["process_outcome" "delivery_evidence" "delivery_evidence_sha256" "delivery_outcome" "delivery_reason"])

(def terminal-retraction-order ["process_outcome" "delivery_outcome" "delivery_reason" "delivery_evidence_sha256" "delivery_evidence"])

(def route-authority-predicates #{"provider" "provider_target" "live_input" "live_input_state" "live_input_epoch" "model" "effort"})

(def projection-predicates #{"display_handle" "display_name"})

(def route-predicates (into route-authority-predicates projection-predicates))

(def goal-overlay-predicates #{"goal" "display_name"})

(def route-generation-predicates (disj route-predicates "display_name"))

(def identity-predicates north.agent-provenance/identity-predicates)

(def publish-predicates (into identity-predicates projection-predicates))

(def managed-projection-predicates (set north.lifecycle-projection/managed-agent-predicates))

(def required-identity-predicates (disj (set north.agent-provenance/required-identity-predicates) marker-predicate))

(def writer-timeout-bound-ms (let [parsed (parse-long (or (System/getenv "NORTH_IDENTITY_WRITER_TIMEOUT_MS") "10000"))]
  (if (nil? parsed) -1 parsed)))

(def write-lease-ttl-ms (let [parsed (parse-long (or (System/getenv "NORTH_IDENTITY_WRITE_LEASE_TTL_MS") "60000"))]
  (if (nil? parsed) -1 parsed)))

(def max-write-lease-wait-ms 5000)

(def ^:dynamic *write-lease* nil)

(defn fail! [message data]
  (throw (ex-info message data)))

(defn checked! [result operation]
  (if (:reject result) (do
  (fail! "coordinator rejected harness identity write" {:operation operation :reject (:reject result) :version (:version result)})))
  result)

(defn entity! [subject]
  (let [raw (str/replace (str subject) #"^@?agent:" "")
   canonical (str "@agent:" raw)]
  (if (not (north.terminal-projection/valid-agent-entity? canonical)) (do
  (fail! "invalid managed agent id" {:subject subject})))
  canonical))

(defn payload! [raw]
  (let [parsed (try
  (json/parse-string (str raw))
  (catch Exception e
    (fail! "invalid managed identity JSON" {:cause (.getMessage e)})))]
  (if (not (map? parsed)) (do
  (fail! "managed identity payload must be an object" {})))
  (into (sorted-map) (map (fn [[predicate value]] (if (not (and (string? predicate) (string? value) (not (str/blank? value)))) (do
  (fail! "managed identity facts must be nonblank strings" {:predicate predicate :value-type (type value)})))
  (if (not (= value (str/trim value))) (do
  (fail! "managed identity facts may not carry boundary whitespace" {:predicate predicate})))
  [predicate value])) parsed)))

(defn facts-of
  ([port subject]
    (facts-of port subject north.lifecycle-projection/managed-agent-predicates))
  ([port subject predicates]
    (north.lifecycle-projection/raw-point-facts (fn [entity predicate] (north.coord/many! port entity predicate)) subject predicates)))

(defn write-lease-resource [subject]
  (str "managed-agent-write:" (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes (str subject) java.nio.charset.StandardCharsets/UTF_8))]
  (format "%064x" (java.math.BigInteger. 1 digest)))))

(defn write-fence-valid? [port]
  (let [{:keys [resource holder epoch]} *write-lease*]
  (and resource holder epoch (:valid? (north.coord/check-lease! port *write-lease*)))))

(defn canonical-record [record]
  (json/generate-string (into (sorted-map) record)))

(defn retract-values! [port subject predicate values]
  (doseq [value values]
  (checked! (north.coord/retract-with-fence! port *write-lease* subject predicate value) [:retract-with-fence subject predicate value])))

(defn put-facts! [port subject facts]
  (doseq [[predicate value] facts]
  (checked! (north.coord/put-with-fence! port *write-lease* subject predicate value) [:put-with-fence subject predicate value])))

(defn put-values! [port subject predicate values]
  (doseq [value (sort values)]
  (checked! (north.coord/put-with-fence! port *write-lease* subject predicate value) [:put-with-fence subject predicate value])))

(defn exact-projection [facts predicates]
  (into (sorted-map) (keep (fn [predicate] (let [bind__0 (seq (get facts predicate))]
  (if bind__0 (let [values bind__0]
  (do
  [predicate (set values)])))))) predicates))

(defn managed-projection [facts]
  (exact-projection facts managed-projection-predicates))

(defn singleton-facts [facts predicates]
  (into (sorted-map) (keep (fn [predicate] (let [bind__1 (first (get facts predicate))]
  (if bind__1 (let [value bind__1]
  (do
  [predicate value])))))) predicates))

(defn desired-projection [facts]
  (into (sorted-map) (map (fn [[predicate value]] [predicate #{value}])) facts))

(defn canonical [facts]
  (apply str (map (fn [[predicate value]] (str predicate "\u0000" value "\n")) facts)))

(defn sha256 [s]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes (str s) java.nio.charset.StandardCharsets/UTF_8))]
  (format "%064x" (java.math.BigInteger. 1 digest))))

(defn verify-exact-snapshot! [snapshot desired predicates]
  (let [actual (exact-projection snapshot predicates)
   expected (desired-projection desired)]
  (if (not (= expected actual)) (do
  (fail! "managed identity readback did not match the published projection" {:expected expected :actual actual})))))

(defn verify-exact! [port subject desired predicates]
  (verify-exact-snapshot! (facts-of port subject) desired predicates))

(defn singleton-projection! [facts predicates]
  (doseq [predicate predicates
   :let [values (get facts predicate #{})]
   :when (> (count values) 1)]
  (fail! "managed identity contains multiple live values for one predicate" {:predicate predicate :values values})))

(defn validate-publish! [facts]
  (let [unknown (seq (remove publish-predicates (keys facts)))
   missing (seq (remove (fn [__north_anon_1] (contains? facts __north_anon_1)) required-identity-predicates))]
  (if unknown (do
  (fail! "unsupported managed identity predicate" {:predicates unknown})))
  (if missing (do
  (fail! "incomplete managed identity projection" {:predicates missing})))
  (if (not (= "lane" (get facts "kind"))) (do
  (fail! "managed SDK identity kind must be lane" {:kind (get facts "kind")})))
  (if (not (contains? #{"streaming" "turn-messages" "unsupported"} (get facts "live_input"))) (do
  (fail! "managed SDK identity has invalid live_input" {:live-input (get facts "live_input")})))
  (if (not (contains? #{"pending" "armed" "frozen"} (get facts "live_input_state"))) (do
  (fail! "managed SDK identity has invalid live_input_state" {:live-input-state (get facts "live_input_state")})))
  (if (not (some? (re-matches #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$" (get facts "live_input_epoch")))) (do
  (fail! "managed SDK identity has invalid live_input_epoch" {:live-input-epoch (get facts "live_input_epoch")})))
  (let [bind__2 (get facts "shadow_reviewer_note_capability_sha256")]
  (if bind__2 (let [digest bind__2]
  (do
  (if (nil? (re-matches #"^[0-9a-f]{64}$" digest)) (do
  (fail! "managed SDK identity has invalid shadow reviewer note capability digest" {})))))))
  (if (and (= "unsupported" (get facts "live_input")) (not= "frozen" (get facts "live_input_state"))) (do
  (fail! "unsupported live_input must remain frozen" {:live-input-state (get facts "live_input_state")})))
  (case (get facts "composition_kind")
    "template" (do
  (if (not (contains? facts "composition_overrides")) (do
  (fail! "template identity requires composition_overrides" {})))
  (if (some (fn [__north_anon_1] (contains? facts __north_anon_1)) ["bespoke_reason" "promotion_candidate" "composition_contract_sha256" "composition_contract_fingerprint_version" "composition_contract_fingerprint_domain"]) (do
  (fail! "template identity carries bespoke-only evidence" {}))))
    "bespoke" (do
  (doseq [predicate ["bespoke_reason" "promotion_candidate" "composition_contract_sha256" "composition_contract_fingerprint_version" "composition_contract_fingerprint_domain"]]
  (if (not (contains? facts predicate)) (do
  (fail! "bespoke identity is missing authority evidence" {:predicate predicate}))))
  (if (not (contains? #{"false" "true"} (get facts "promotion_candidate"))) (do
  (fail! "invalid bespoke promotion_candidate" {})))
  (if (nil? (re-matches #"^[0-9a-f]{64}$" (get facts "composition_contract_sha256"))) (do
  (fail! "invalid bespoke contract fingerprint" {})))
  (if (not (= "v1" (get facts "composition_contract_fingerprint_version"))) (do
  (fail! "unsupported bespoke contract fingerprint version" {})))
  (if (not (= "north:bespoke-contract:v1" (get facts "composition_contract_fingerprint_domain"))) (do
  (fail! "unsupported bespoke contract fingerprint domain" {}))))
    (fail! "managed identity composition_kind must be template or bespoke" {:composition-kind (get facts "composition_kind")}))))

(defn commit-marker! [port subject facts]
  (let [authoritative (into (sorted-map) (select-keys facts identity-predicates))
   marker (sha256 (canonical authoritative))]
  (let [snapshot (facts-of port subject)
   markers (get snapshot marker-predicate #{})]
  (verify-exact-snapshot! snapshot authoritative identity-predicates)
  (if (not (or (empty? markers) (= #{marker} markers))) (do
  (fail! "managed identity has a competing generation marker" {:subject subject}))))
  (checked! (north.coord/put-with-fence! port *write-lease* subject marker-predicate marker) [:put-subject-local-marker subject marker-predicate marker])
  (let [snapshot (facts-of port subject)]
  (verify-exact-snapshot! snapshot authoritative identity-predicates)
  (if (not (= #{marker} (get snapshot marker-predicate))) (do
  (fail! "managed identity commit marker was not acknowledged" {:marker marker}))))
  marker))

(defn terminal-marker! [port subject facts validate-current!]
  (let [marker (north.terminal-projection/terminal-manifest-sha256 facts)]
  (if (not marker) (do
  (fail! "cannot commit an incomplete managed terminal projection" {})))
  (checked! (north.coord/assert-after-read-with-fence! port *write-lease* subject terminal-marker-predicate marker validate-current!) [:assert-after-read-with-fence subject terminal-marker-predicate marker])
  (if (not (= marker (north.coord/resolved! port subject terminal-marker-predicate))) (do
  (fail! "managed terminal commit marker was not acknowledged" {:marker marker})))
  marker))

(declare validate-terminal!)

(defn validate-existing-managed-projection! [subject snapshot]
  (singleton-projection! snapshot managed-projection-predicates)
  (let [identity (singleton-facts snapshot publish-predicates)
   identity-marker (first (get snapshot marker-predicate))]
  (validate-publish! identity)
  (if (not (= identity-marker (sha256 (canonical (into (sorted-map) (select-keys identity identity-predicates)))))) (do
  (fail! "existing managed identity is not an exact committed generation" {:subject subject}))))
  (let [terminal-view (exact-projection snapshot (conj terminal-predicates terminal-marker-predicate))]
  (if (seq terminal-view) (do
  (let [terminal (singleton-facts snapshot terminal-predicates)
   terminal-marker (first (get snapshot terminal-marker-predicate))]
  (validate-terminal! subject terminal)
  (if (not (= terminal-marker (north.terminal-projection/terminal-manifest-sha256 terminal))) (do
  (fail! "existing managed terminal is not an exact committed projection" {:subject subject}))))))))

(defn clear-managed-projection! [port subject]
  (let [current (facts-of port subject)]
  (retract-values! port subject marker-predicate (get current marker-predicate #{}))
  (retract-values! port subject terminal-marker-predicate (get current terminal-marker-predicate #{}))
  (doseq [predicate terminal-retraction-order]
  (retract-values! port subject predicate (get current predicate #{})))
  (doseq [predicate (sort publish-predicates)]
  (retract-values! port subject predicate (get current predicate #{})))))

(defn restore-managed-projection! [port subject snapshot]
  (let [expected (managed-projection snapshot)]
  (clear-managed-projection! port subject)
  (doseq [predicate (sort publish-predicates)]
  (put-values! port subject predicate (get snapshot predicate #{})))
  (doseq [predicate terminal-publication-order]
  (put-values! port subject predicate (get snapshot predicate #{})))
  (put-values! port subject marker-predicate (get snapshot marker-predicate #{}))
  (put-values! port subject terminal-marker-predicate (get snapshot terminal-marker-predicate #{}))
  (let [actual (managed-projection (facts-of port subject))]
  (if (not (= expected actual)) (do
  (fail! "managed projection rollback readback mismatch" {:subject subject :expected expected :actual actual}))))))

(defn rollback-managed-projection! [port subject snapshot original-error]
  (try
  (if (write-fence-valid? port) (do
  (restore-managed-projection! port subject snapshot)
  nil))
  (catch Throwable rollback-error
    (.addSuppressed ^Throwable original-error ^Throwable rollback-error))))

(defn with-managed-rollback! [port subject snapshot operation!]
  (try
  (operation!)
  (catch Throwable operation-error
    (rollback-managed-projection! port subject snapshot operation-error)
    (throw operation-error))))

(defn publish! [port subject facts]
  (validate-publish! facts)
  (let [before (facts-of port subject)
   fresh? (empty? (managed-projection before))
   exact-uncommitted-retry? (and (nil? (get before marker-predicate)) (empty? (exact-projection before (conj terminal-predicates terminal-marker-predicate))) (= (desired-projection facts) (exact-projection before publish-predicates)))
   mutating? (atom false)]
  (if (not (or fresh? exact-uncommitted-retry?)) (do
  (validate-existing-managed-projection! subject before)))
  (try
  (if exact-uncommitted-retry? (do
  (verify-exact-snapshot! before facts publish-predicates)
  (commit-marker! port subject facts)) (do
  (reset! mutating? true)
  (clear-managed-projection! port subject)
  (put-facts! port subject facts)
  (verify-exact! port subject facts publish-predicates)
  (commit-marker! port subject facts)))
  (catch Throwable publication-error
    (if (deref mutating?) (do
  (rollback-managed-projection! port subject before publication-error)))
    (throw publication-error)))))

(defn validate-terminal! [subject facts]
  (let [predicates (set (keys facts))
   unknown (seq (remove terminal-predicates predicates))
   missing (seq (remove predicates base-terminal-predicates))]
  (if unknown (do
  (fail! "terminal carries unsupported predicates" {:predicates unknown})))
  (if missing (do
  (fail! "terminal is missing base process/delivery predicates" {:predicates missing}))))
  (if (not (contains? #{"unverified" "blocked" "reported"} (get facts "delivery_outcome"))) (do
  (fail! "invalid delivery_outcome" {:delivery-outcome (get facts "delivery_outcome")})))
  (if (not (north.terminal-projection/delivery-projection-valid? facts)) (do
  (fail! "delivery outcome lacks a valid proof projection" {:delivery-outcome (get facts "delivery_outcome")})))
  (let [bind__3 (get facts "delivery_evidence")]
  (if bind__3 (let [evidence-raw bind__3]
  (do
  (let [evidence (json/parse-string evidence-raw)]
  (if (not (= subject (get evidence "reporter"))) (do
  (fail! "delivery reporter must be the managed terminal subject" {:subject subject :reporter (get evidence "reporter")})))))))))

(defn validate-reported-run-with!
  "A syntactically valid snapshot is not proof by itself. Before exposing a\n  reported lane terminal, bind it to the committed reservation and exact\n  writer-scoped self-report already present on the named run subject." [read-facts subject facts]
  (if (= "reported" (get facts "delivery_outcome")) (do
  (let [evidence (json/parse-string (get facts "delivery_evidence"))
   run (get evidence "run")
   thread (get evidence "thread")
   run-facts (read-facts run north.lifecycle-projection/reported-run-predicates)
   reservation-origin (north.terminal-projection/singleton-value run-facts "run_reservation_contract_origin")
   reservation-baseline (north.terminal-projection/run-reservation-done-when run-facts)
   current-bars (north.terminal-projection/canonical-done-when (read-facts thread north.lifecycle-projection/reported-thread-predicates))
   cited-records (set (mapcat (fn [match] (map canonical-record (get match "evidence"))) (get evidence "matches")))
   evidence-state (north.terminal-projection/run-evidence-state run-facts run thread subject)
   stored-records (:raws evidence-state)]
  (if (not (north.terminal-projection/run-reservation-valid? run-facts)) (do
  (fail! "reported delivery requires a committed run reservation" {:subject subject :run run})))
  (if (not (= #{subject} (get run-facts "run_reservation_agent"))) (do
  (fail! "reported delivery reservation agent mismatch" {:subject subject :run run})))
  (if (not (= #{thread} (get run-facts "run_reservation_thread"))) (do
  (fail! "reported delivery reservation thread mismatch" {:subject subject :run run :thread thread})))
  (if (not (= reservation-origin (get evidence "contractOrigin"))) (do
  (fail! "reported delivery contract origin differs from its reservation" {:subject subject :run run})))
  (if (not (= reservation-baseline (get evidence "baselineDoneWhen"))) (do
  (fail! "reported delivery baseline differs from its reservation" {:subject subject :run run})))
  (if (not (= current-bars (get evidence "doneWhen"))) (do
  (fail! "reported delivery contract changed before terminal publication" {:subject subject :run run :thread thread})))
  (if (not (:valid? evidence-state)) (do
  (fail! "reported delivery run contains malformed, cross-scoped, duplicate, or excessive evidence" {:subject subject :run run})))
  (if (not (= stored-records cited-records)) (do
  (fail! "reported delivery snapshot must cite the exact reserved-run evidence set" {:subject subject :run run :missing (vec (remove stored-records cited-records)) :uncited (vec (remove cited-records stored-records))})))))))

(defn validate-reported-run! [port subject facts]
  (validate-reported-run-with! (fn [entity predicates] (facts-of port entity predicates)) subject facts))

(defn publish-terminal! [port subject facts]
  (let [before (facts-of port subject)]
  (with-managed-rollback! port subject before (fn [] (retract-values! port subject terminal-marker-predicate (get before terminal-marker-predicate #{}))
  (doseq [predicate terminal-retraction-order]
  (retract-values! port subject predicate (get before predicate #{})))
  (doseq [predicate terminal-publication-order
   :let [value (get facts predicate)]]
  (if value (do
  (checked! (north.coord/put-with-fence! port *write-lease* subject predicate value) [:put-with-fence subject predicate value]))))
  (terminal-marker! port subject facts (fn [] (verify-exact! port subject facts terminal-predicates)
  (validate-reported-run! port subject facts)))))))

(defn terminal! [port subject facts]
  (validate-terminal! subject facts)
  (validate-reported-run! port subject facts)
  (publish-terminal! port subject facts))

(defn terminal-thread! [raw]
  (if (not (str/blank? raw)) (do
  (let [bare (str/replace-first raw #"^@" "")]
  (if (not (and (= raw (str/trim raw)) (<= (count bare) 512) (re-matches #"[A-Za-z0-9][A-Za-z0-9._:-]*" bare))) (do
  (fail! "invalid managed terminal thread id" {:thread raw})))
  (str "@" bare)))))

(defn identity-marker [facts]
  (sha256 (canonical (into (sorted-map) (select-keys facts identity-predicates)))))

(defn exact-committed-identity?! [snapshot desired]
  (and desired (try
  (validate-publish! desired)
  (singleton-projection! snapshot (conj publish-predicates marker-predicate))
  (and (= (desired-projection desired) (exact-projection snapshot publish-predicates)) (= #{(identity-marker desired)} (get snapshot marker-predicate #{})))
  (catch Throwable _
    false))))

(defn committed-identity!
  "Return the exact committed identity map, or nil for a partial, malformed, or\n  multiply-valued generation. Terminal facts are deliberately orthogonal." [snapshot]
  (try
  (singleton-projection! snapshot (conj publish-predicates marker-predicate))
  (let [identity (singleton-facts snapshot publish-predicates)]
  (validate-publish! identity)
  (if (= #{(identity-marker identity)} (get snapshot marker-predicate #{})) (do
  identity)))
  (catch Throwable _
    nil)))

(defn identity-matches-except? [actual expected ignored]
  (= (apply dissoc actual ignored) (apply dissoc expected ignored)))

(defn goal-drift? [actual expected]
  (not= (get actual "goal") (get expected "goal")))

(defn effective-route-desired
  "Rebase caller-owned route axes onto the graph's current goal overlay. Goal\n  is authoritative and independently mutable. display_name is a cross-derived\n  cache: update it from the caller only when goal did not move; otherwise keep\n  the goal writer's cache instead of restoring stale text." [actual expected desired]
  (let [goal-drifted? (goal-drift? actual expected)]
  (cond-> (assoc desired "goal" (get actual "goal")) goal-drifted? (assoc "display_name" (get actual "display_name")))))

(defn route-mutation-predicates [actual expected]
  (cond-> route-generation-predicates (not (goal-drift? actual expected)) (conj "display_name")))

(declare values-compatible-with-transition?)

(defn route-prefix-compatible?
  "Recognize only a killed route prefix. Route publication never mutates the\n  goal-verb-owned goal and preserves display_name when goal has advanced, so that\n  overlay remains recoverable even when the SDK carried a stale full identity." [snapshot expected desired]
  (let [actual (singleton-facts snapshot publish-predicates)
   goal-drifted? (goal-drift? actual expected)
   mutation-predicates (route-mutation-predicates actual expected)
   stable-predicates (apply disj publish-predicates mutation-predicates)
   stable-expected (effective-route-desired actual expected expected)]
  (and (empty? (get snapshot marker-predicate #{})) (empty? (exact-projection snapshot (conj terminal-predicates terminal-marker-predicate))) (every? (fn [predicate] (let [values (get snapshot predicate #{})
   expected-value (get stable-expected predicate)]
  (if expected-value (= #{expected-value} values) (empty? values)))) stable-predicates) (or (not goal-drifted?) (and (not (str/blank? (get actual "goal"))) (not (str/blank? (get actual "display_name"))))) (values-compatible-with-transition? snapshot expected desired mutation-predicates))))

(defn replace-route-projection! [port subject before expected desired]
  (let [actual (singleton-facts before publish-predicates)
   effective (effective-route-desired actual expected desired)
   mutation-predicates (route-mutation-predicates actual expected)]
  (retract-values! port subject marker-predicate (get before marker-predicate #{}))
  (doseq [predicate mutation-predicates]
  (retract-values! port subject predicate (get before predicate #{})))
  (put-facts! port subject (select-keys effective mutation-predicates))
  (verify-exact! port subject (select-keys effective mutation-predicates) mutation-predicates)
  (let [identity (singleton-facts (facts-of port subject) publish-predicates)]
  (validate-publish! identity)
  (commit-marker! port subject identity))))

(defn exact-committed-terminal?! [subject snapshot desired]
  (and desired (try
  (validate-terminal! subject desired)
  (singleton-projection! snapshot (conj terminal-predicates terminal-marker-predicate))
  (and (= (desired-projection desired) (exact-projection snapshot terminal-predicates)) (= #{(north.terminal-projection/terminal-manifest-sha256 desired)} (get snapshot terminal-marker-predicate #{})))
  (catch Throwable _
    false))))

(defn valid-committed-terminal?! [subject snapshot]
  (try
  (let [terminal (singleton-facts snapshot terminal-predicates)]
  (validate-terminal! subject terminal)
  (= #{(north.terminal-projection/terminal-manifest-sha256 terminal)} (get snapshot terminal-marker-predicate #{})))
  (catch Throwable _
    false)))

(defn terminal-projection-present? [snapshot]
  (boolean (seq (exact-projection snapshot (conj terminal-predicates terminal-marker-predicate)))))

(defn values-compatible-with-transition?
  "True only for a markerless killed prefix made entirely from the caller's\n  expected and desired exact projections. The stable holder/rotating epoch\n  establishes who may repair; this value check prevents that owner from\n  blessing an unrelated mixed generation." [snapshot expected desired predicates]
  (every? (fn [predicate] (let [actual (get snapshot predicate #{})
   allowed (set (keep identity [(get expected predicate) (get desired predicate)]))]
  (and (<= (count actual) 1) (every? allowed actual)))) predicates))

(defn replace-identity-projection! [port subject desired]
  (clear-managed-projection! port subject)
  (put-facts! port subject desired)
  (verify-exact! port subject desired publish-predicates)
  (commit-marker! port subject desired))

(defn committed-result [operation-id & $beagle$rest$host]
  (let [reasons (vec $beagle$rest$host)]
  (let [reason (first reasons)]
  (cond-> {:status "committed" :operation_id operation-id} reason (assoc :reason reason)))))

(defn unresolved-result [status operation-id reason]
  {:status status :operation_id operation-id :reason reason})

(defn identity-generation-divergence
  "Name why a marked generation is invalid: which publish predicates diverge from\n  desired, and whether the marker is the digest of its own durable body." [snapshot desired]
  (let [divergent (sort (for [predicate publish-predicates
   :let [durable (get snapshot predicate #{})
   wanted (get desired predicate)]
   :when (not= durable (if wanted #{wanted} #{}))]
  predicate))
   durable-marker (first (get snapshot marker-predicate #{}))
   body-marker (try
  (identity-marker (singleton-facts snapshot publish-predicates))
  (catch Throwable _
    nil))]
  (str/join "; " (cond-> [] (seq divergent) (conj (str "durable " (str/join "," (take 8 divergent)) " differ from desired")) (and durable-marker body-marker (not= durable-marker body-marker)) (conj (str "marker " durable-marker " is not the digest of its own durable body " body-marker))))))

(defn recover-identity-write!
  "Apply or recover one caller-owned publish/route transition. `expected` and\n  `desired` are complete projections. A replay may repair only an exact prior\n  generation or a markerless prefix composed from those two projections." [port subject operation operation-id delta desired expected]
  (if (not desired) (do
  (fail! "managed identity recovery requires a complete desired projection" {:operation-id operation-id})))
  (validate-publish! desired)
  (if expected (do
  (validate-publish! expected)))
  (case operation
    "publish" (if (not (= desired delta)) (do
  (fail! "managed publish payload must equal its complete desired projection" {:operation-id operation-id})))
    "route" (do
  (if (not expected) (do
  (fail! "managed route recovery requires a complete expected projection" {:operation-id operation-id})))
  (if (not (= route-predicates (set (keys delta)))) (do
  (fail! "managed route operation requires the exact route predicate set" {:operation-id operation-id :predicates (set (keys delta))})))
  (if (not (= delta (select-keys desired route-predicates))) (do
  (fail! "managed route delta disagrees with desired projection" {:operation-id operation-id})))
  (if (not (= (apply dissoc expected route-predicates) (apply dissoc desired route-predicates))) (do
  (fail! "managed route operation changed non-route identity authority" {:operation-id operation-id}))))
    (fail! "unsupported recoverable managed identity operation" {:operation operation}))
  (let [before (facts-of port subject)
   current (committed-identity! before)
   desired-committed? (if (= "route" operation) (and current (identity-matches-except? current desired goal-overlay-predicates)) (exact-committed-identity?! before desired))]
  (cond
  desired-committed? (committed-result operation-id "exact_replay")
  (terminal-projection-present? before) (if (valid-committed-terminal?! subject before) (unresolved-result "not_committed" operation-id "terminal_committed") (unresolved-result "indeterminate" operation-id "partial_or_invalid_terminal"))
  (and (= "publish" operation) expected (exact-committed-identity?! before expected)) (do
  (replace-identity-projection! port subject desired)
  (committed-result operation-id))
  (and (= "route" operation) current (identity-matches-except? current expected goal-overlay-predicates)) (do
  (replace-route-projection! port subject before expected desired)
  (committed-result operation-id (if (goal-drift? current expected) (do
  "rebased_goal_overlay"))))
  (and (= "publish" operation) (nil? expected) (empty? (managed-projection before))) (do
  (replace-identity-projection! port subject desired)
  (committed-result operation-id))
  (and (= "route" operation) (route-prefix-compatible? before expected desired)) (do
  (replace-route-projection! port subject before expected desired)
  (committed-result operation-id "recovered_killed_prefix"))
  (and (= "publish" operation) (empty? (get before marker-predicate #{})) (empty? (exact-projection before (conj terminal-predicates terminal-marker-predicate))) (values-compatible-with-transition? before expected desired publish-predicates)) (do
  (replace-identity-projection! port subject desired)
  (committed-result operation-id "recovered_killed_prefix"))
  current (unresolved-result "not_committed" operation-id "conflicting_generation")
  (seq (get before marker-predicate #{})) (unresolved-result "indeterminate" operation-id (let [detail (identity-generation-divergence before desired)]
  (cond-> "invalid_identity_generation" (seq detail) (str " (" detail ")"))))
  :else (unresolved-result "indeterminate" operation-id "unrecognized_partial_generation"))))

(defn terminal-prefix-compatible? [snapshot desired]
  (and (empty? (get snapshot terminal-marker-predicate #{})) (every? (fn [predicate] (let [actual (get snapshot predicate #{})
   wanted (get desired predicate)]
  (and (<= (count actual) 1) (or (empty? actual) (= #{wanted} actual))))) terminal-predicates)))

(defn update-route! [port subject facts]
  (let [unknown (seq (remove route-predicates (keys facts)))
   missing (seq (remove (fn [__north_anon_1] (contains? facts __north_anon_1)) route-predicates))]
  (if unknown (do
  (fail! "unsupported managed route predicate" {:predicates unknown})))
  (if missing (do
  (fail! "incomplete managed route projection" {:predicates missing}))))
  (let [before (facts-of port subject)
   current (singleton-facts before publish-predicates)
   marker (first (get before marker-predicate))]
  (singleton-projection! before (conj publish-predicates marker-predicate))
  (validate-publish! current)
  (if (not (= marker (sha256 (canonical (into (sorted-map) (select-keys current identity-predicates)))))) (do
  (fail! "cannot update an uncommitted or corrupted managed route" {})))
  (with-managed-rollback! port subject before (fn [] (retract-values! port subject marker-predicate (get before marker-predicate #{}))
  (doseq [predicate route-predicates]
  (retract-values! port subject predicate (get before predicate #{})))
  (put-facts! port subject facts)
  (verify-exact! port subject facts route-predicates)
  (let [identity (singleton-facts (facts-of port subject) publish-predicates)]
  (validate-publish! identity)
  (commit-marker! port subject identity))))))

(defn goal! [port subject facts]
  (if (not (= #{"goal" "display_name"} (set (keys facts)))) (do
  (fail! "north goal requires exactly goal and display_name" {:predicates (keys facts)})))
  (let [before (facts-of port subject)
   current (singleton-facts before publish-predicates)
   marker (first (get before marker-predicate))]
  (singleton-projection! before (conj publish-predicates marker-predicate))
  (validate-publish! current)
  (if (not (= marker (sha256 (canonical (into (sorted-map) (select-keys current identity-predicates)))))) (do
  (fail! "cannot set the goal of an uncommitted or corrupted managed identity" {})))
  (with-managed-rollback! port subject before (fn [] (retract-values! port subject marker-predicate (get before marker-predicate #{}))
  (doseq [predicate ["goal" "display_name"]]
  (retract-values! port subject predicate (get before predicate #{})))
  (put-facts! port subject facts)
  (verify-exact! port subject facts #{"goal" "display_name"})
  (let [projection (singleton-facts (facts-of port subject) publish-predicates)]
  (validate-publish! projection)
  (commit-marker! port subject projection))))))

(def uuid-v4-pattern #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")

(defn require-write-lease-policy! []
  (if (not (and (integer? writer-timeout-bound-ms) (pos? writer-timeout-bound-ms) (integer? write-lease-ttl-ms) (> write-lease-ttl-ms writer-timeout-bound-ms))) (do
  (fail! "managed agent write lease must outlive the writer process timeout" {:writer-timeout-ms writer-timeout-bound-ms :lease-ttl-ms write-lease-ttl-ms}))))

(defn validated-writer-holder! [supplied-holder]
  (let [holder (or supplied-holder (str "managed-agent-writer:" (java.util.UUID/randomUUID)))
   holder-id (str/replace holder #"^managed-agent-writer:" "")]
  (if (not (and (str/starts-with? holder "managed-agent-writer:") (re-matches uuid-v4-pattern holder-id))) (do
  (fail! "invalid managed agent writer holder" {:holder holder})))
  holder))

(defn optional-payload! [raw]
  (if (not (str/blank? raw)) (do
  (payload! raw))))

(defn ensure-native-client! []
  (if (not (find-ns 'north.store-rpc-client)) (do
  (load-file (str writer-root "/store-rpc-client.clj")))))

(defn native-rpc! [operation & $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (ensure-native-client!)
  (apply (or (ns-resolve 'north.store-rpc-client operation) (fail! "native writer primitive is unavailable" {:operation operation})) args)))

(defn native-triple-predicate! [triple]
  ((or (ns-resolve 'store.types 'triple-t2) (fail! "native triple accessor is unavailable" {})) triple))

(defn native-connect! [port]
  (native-rpc! 'connect (or (not-empty (System/getenv "NORTH_STORE_HOST")) "127.0.0.1") port (or (not-empty (System/getenv "BEAGLE_STORE_SPACE_ID")) "north-coordination") {:connect-timeout-ms 2000 :read-timeout-ms writer-timeout-bound-ms :max-attempts 1}))

(defn presence-fence! [raw subject]
  (if (str/blank? raw) (do
  (fail! "managed lifecycle operation requires its exact liveness fence" {:subject subject})))
  (let [parsed (try
  (json/parse-string raw)
  (catch Throwable error
    (fail! "managed liveness fence must be valid JSON" {:subject subject :cause (.getMessage error)})))
   expected-holder (subs subject (count "@agent:"))
   expected-resource (str "session:" expected-holder)]
  (if (not (and (map? parsed) (= #{"resource" "holder" "epoch"} (set (keys parsed))) (= expected-resource (get parsed "resource")) (= expected-holder (get parsed "holder")) (integer? (get parsed "epoch")) (pos? (get parsed "epoch")))) (do
  (fail! "managed liveness fence does not exactly match its subject" {:subject subject :fence parsed})))
  {:resource expected-resource :holder expected-holder :epoch (get parsed "epoch")}))

(defn native-fence-term! [fence]
  (let [constructor (ns-resolve 'north.coord 'lease-fence)]
  (if (not constructor) (do
  (fail! "canonical Store RPC lease-fence constructor is unavailable" {})))
  (constructor (:resource fence) (:holder fence) (:epoch fence))))

(defn native-presence-valid?! [client fence]
  (:valid? (native-rpc! 'lease-check! client (native-fence-term! fence))))

(defn native-release-presence! [client fence]
  (:released? (native-rpc! 'lease-release! client (native-fence-term! fence))))

(defn occurrence-snapshot [occurrences]
  (into {} (keep (fn [[predicate values]] (if (seq values) (do
  [predicate (set (keys values))])))) occurrences))

(defn identity-occurrences [facts]
  (let [identity (into (sorted-map) (select-keys facts publish-predicates))]
  (reduce (fn [projection [predicate value]] (assoc projection predicate {value 1})) (into {} (map (fn [predicate] [predicate {}]) managed-projection-predicates)) (assoc identity marker-predicate (identity-marker identity)))))

(defn terminal-occurrences! [facts]
  (let [marker (north.terminal-projection/terminal-manifest-sha256 facts)]
  (if (not marker) (do
  (fail! "cannot encode an incomplete managed terminal projection" {})))
  (into {} (map (fn [predicate] (let [value (if (= terminal-marker-predicate predicate) marker (get facts predicate))]
  [predicate (if value {value 1} {})]))) (conj terminal-predicates terminal-marker-predicate))))

(defn native-facts-of!
  ([client subject]
    (native-facts-of! client subject managed-projection-predicates))
  ([client subject predicates]
    (let [projection (native-rpc! 'subject-projection! client subject)
   snapshot (occurrence-snapshot (:occurrences projection))]
  {:served-version (:served-version projection) :facts (into {} (keep (fn [predicate] (let [bind__15 (seq (get snapshot predicate))]
  (if bind__15 (let [values bind__15]
  (do
  [predicate (set values)])))))) predicates)})))

(defn native-facts-at-version! [client served-version subject predicates]
  (let [projection (native-facts-of! client subject predicates)]
  (if (not (= served-version (:served-version projection))) (do
  (throw (ex-info "Store RPC validation snapshot changed" {:type :native/snapshot-conflict :expected-version served-version :served-version (:served-version projection)}))))
  (:facts projection)))

(defn exact-native-identity? [occurrences identity]
  (= (into {} (remove (comp empty? val) (identity-occurrences identity))) (into {} (keep (fn [predicate] (if (seq (get occurrences predicate)) (do
  [predicate (get occurrences predicate)])))) managed-projection-predicates)))

(defn validate-native-recovery! [operation operation-id delta desired expected]
  (if (not desired) (do
  (fail! "managed identity recovery requires a complete desired projection" {:operation-id operation-id})))
  (validate-publish! desired)
  (if expected (do
  (validate-publish! expected)))
  (case operation
    "publish" (if (not (= desired delta)) (do
  (fail! "managed publish payload must equal its complete desired projection" {:operation-id operation-id})))
    "route" (do
  (if (not expected) (do
  (fail! "managed route recovery requires a complete expected projection" {:operation-id operation-id})))
  (if (not (= route-predicates (set (keys delta)))) (do
  (fail! "managed route operation requires the exact route predicate set" {:operation-id operation-id :predicates (set (keys delta))})))
  (if (not (= delta (select-keys desired route-predicates))) (do
  (fail! "managed route delta disagrees with desired projection" {:operation-id operation-id})))
  (if (not (= (apply dissoc expected route-predicates) (apply dissoc desired route-predicates))) (do
  (fail! "managed route operation changed non-route identity authority" {:operation-id operation-id}))))
    (fail! "unsupported recoverable managed identity operation" {:operation operation})))

(defn native-classify-identity!
  "Pure expected->desired classifier over one native subject occurrence snapshot." [subject operation operation-id delta desired expected occurrences]
  (validate-native-recovery! operation operation-id delta desired expected)
  (let [before (occurrence-snapshot occurrences)
   current (committed-identity! before)
   desired-committed? (if (= "route" operation) (and current (identity-matches-except? current desired goal-overlay-predicates) (exact-native-identity? occurrences current)) (exact-native-identity? occurrences desired))]
  (cond
  desired-committed? {:result (committed-result operation-id "exact_replay")}
  (terminal-projection-present? before) {:result (if (valid-committed-terminal?! subject before) (unresolved-result "not_committed" operation-id "terminal_committed") (unresolved-result "indeterminate" operation-id "partial_or_invalid_terminal"))}
  (and (= "publish" operation) expected (exact-committed-identity?! before expected)) {:desired desired :reason nil}
  (and (= "route" operation) current (identity-matches-except? current expected goal-overlay-predicates)) {:desired (effective-route-desired current expected desired) :reason (if (goal-drift? current expected) (do
  "rebased_goal_overlay"))}
  (and (= "publish" operation) (nil? expected) (empty? (managed-projection before))) {:desired desired :reason nil}
  (and (= "route" operation) (route-prefix-compatible? before expected desired)) (let [actual (singleton-facts before publish-predicates)]
  {:desired (effective-route-desired actual expected desired) :reason "recovered_killed_prefix"})
  (and (= "publish" operation) (empty? (get before marker-predicate #{})) (empty? (exact-projection before (conj terminal-predicates terminal-marker-predicate))) (values-compatible-with-transition? before expected desired publish-predicates)) {:desired desired :reason "recovered_killed_prefix"}
  current {:result (unresolved-result "not_committed" operation-id "conflicting_generation")}
  (seq (get before marker-predicate #{})) {:result (unresolved-result "indeterminate" operation-id (let [detail (identity-generation-divergence before desired)]
  (cond-> "invalid_identity_generation" (seq detail) (str " (" detail ")"))))}
  :else {:result (unresolved-result "indeterminate" operation-id "unrecognized_partial_generation")})))

(defn native-plan-with-marker! [subject before desired marker]
  (native-rpc! 'plan-subject-actions subject before (identity-occurrences desired) {:rank (fn [action] (if (= marker (native-triple-predicate! (:proposition action))) 1 0))}))

(defn native-plan! [subject before desired]
  (native-plan-with-marker! subject before desired marker-predicate))

(defn native-plan-occurrences! [subject before desired marker]
  (native-rpc! 'plan-subject-actions subject before desired {:rank (fn [action] (if (= marker (native-triple-predicate! (:proposition action))) 1 0))}))

(defn native-submit-scoped-batch! [client subject before desired actions fence expected-version]
  (let [outcome (native-rpc! 'fenced-batch! client actions {:fence fence :expected-version expected-version})]
  (if (contains? #{:applied :no-op :sent-ambiguous} (:outcome outcome)) (assoc outcome :readback (native-rpc! 'subject-readback! client subject desired {:before before})) outcome)))

(defn native-readback! [client subject before desired]
  (native-rpc! 'subject-readback! client subject (identity-occurrences desired) {:before before}))

(defn native-result-after-readback [operation-id reason readback]
  (case (:state readback)
    :committed (committed-result operation-id reason)
    :absent (unresolved-result "not_committed" operation-id "batch_absent")
    :foreign-writer (unresolved-result "indeterminate" operation-id "foreign_writer_after_batch")
    (unresolved-result "indeterminate" operation-id "readback_mismatch")))

(defn native-acquire-lease! [client subject holder]
  (let [deadline (+ (System/nanoTime) (* writer-timeout-bound-ms 1000000))
   resource (write-lease-resource subject)]
  (loop []
  (let [version (:served-version (native-rpc! 'version! client))
   outcome (native-rpc! 'lease-acquire-at-version! client resource holder write-lease-ttl-ms version)]
  (case (:outcome outcome)
    :applied outcome
    :conflict (if (< (System/nanoTime) deadline) (recur) {:result (unresolved-result "not_committed" nil "lease_conflict_retry_exhausted")})
    :sent-ambiguous (let [check (try
  (native-rpc! 'lease-check! client (:candidate-fence outcome))
  (catch Throwable _
    nil))]
  (if (:valid? check) (assoc outcome :outcome :applied :fence (:candidate-fence outcome)) (if (< (System/nanoTime) deadline) (recur) {:result (unresolved-result "indeterminate" nil "lease_acquire_ambiguous")})))
    :durability-ambiguous {:result (unresolved-result "indeterminate" nil "restart_required_durability_ambiguous")}
    :lease-held {:result (unresolved-result "not_committed" nil "write_lease_held")}
    {:result (unresolved-result "indeterminate" nil (str "lease_acquire_" (name (:outcome outcome))))})))))

(defn with-operation-id [result operation-id]
  (assoc result :operation_id operation-id))

(defn native-publish-identity! [client subject operation operation-id delta desired expected holder]
  (let [deadline (+ (System/nanoTime) (* writer-timeout-bound-ms 1000000))
   acquire (native-acquire-lease! client subject holder)]
  (let [bind__18 (:result acquire)]
  (if bind__18 (let [result bind__18]
  (with-operation-id result operation-id)) (let [fence (:fence acquire)]
  (try
  (loop [conflicts 0]
  (let [{:keys [served-version occurrences]} (native-rpc! 'subject-projection! client subject)
   classification (native-classify-identity! subject operation operation-id delta desired expected occurrences)]
  (let [bind__19 (:result classification)]
  (if bind__19 (let [result bind__19]
  result) (let [target (:desired classification)
   actions (native-plan! subject occurrences target)
   outcome (native-rpc! 'fenced-batch! client actions {:fence fence :expected-version served-version})]
  (case (:outcome outcome)
    :durability-ambiguous (unresolved-result "indeterminate" operation-id "restart_required_durability_ambiguous")
    :conflict (if (and (< conflicts 32) (< (System/nanoTime) deadline)) (recur (inc conflicts)) (unresolved-result "not_committed" operation-id "conflict_retry_exhausted"))
    :fence-mismatch (let [final-projection (native-rpc! 'subject-projection! client subject)
   final-classification (native-classify-identity! subject operation operation-id delta desired expected (:occurrences final-projection))]
  (or (:result final-classification) (unresolved-result "not_committed" operation-id "lease_fence_mismatch")))
    :sent-ambiguous (native-result-after-readback operation-id (:reason classification) (native-readback! client subject occurrences target))
    :applied (native-result-after-readback operation-id (:reason classification) (native-readback! client subject occurrences target))
    :no-op (native-result-after-readback operation-id (:reason classification) (native-readback! client subject occurrences target))
    :not-sent (unresolved-result "not_committed" operation-id "batch_not_sent")
    (unresolved-result "indeterminate" operation-id (str "batch_" (name (:outcome outcome))))))))))
  (finally
    (try
  (native-rpc! 'lease-release! client fence)
  (catch Throwable _
    nil)))))))))

(defn validate-goal! [facts]
  (if (not (= #{"goal" "display_name"} (set (keys facts)))) (do
  (fail! "north goal requires exactly goal and display_name" {:predicates (set (keys facts))}))))

(defn native-goal! [client subject operation-id facts holder presence-fence]
  (validate-goal! facts)
  (let [deadline (+ (System/nanoTime) (* writer-timeout-bound-ms 1000000))
   acquire (native-acquire-lease! client subject holder)]
  (let [bind__20 (:result acquire)]
  (if bind__20 (let [result bind__20]
  (with-operation-id result operation-id)) (let [fence (:fence acquire)]
  (try
  (loop [conflicts 0]
  (if (native-presence-valid?! client presence-fence) (let [{:keys [served-version occurrences]} (native-rpc! 'subject-projection! client subject)
   before (occurrence-snapshot occurrences)
   current (committed-identity! before)]
  (cond
  (terminal-projection-present? before) (if (valid-committed-terminal?! subject before) (unresolved-result "not_committed" operation-id "terminal_committed") (unresolved-result "indeterminate" operation-id "partial_or_invalid_terminal"))
  (nil? current) (unresolved-result "indeterminate" operation-id "invalid_identity_generation")
  :else (let [desired (merge current facts)
   _ (validate-publish! desired)
   desired-occurrences (identity-occurrences desired)]
  (if (exact-native-identity? occurrences desired) (committed-result operation-id "exact_replay") (let [mutation-predicates #{"goal" "display_name" marker-predicate}
   target (select-keys desired-occurrences mutation-predicates)
   actions (native-plan-occurrences! subject occurrences target marker-predicate)
   outcome (native-submit-scoped-batch! client subject occurrences desired-occurrences actions fence served-version)]
  (case (:outcome outcome)
    :conflict (if (and (< conflicts 32) (< (System/nanoTime) deadline)) (recur (inc conflicts)) (unresolved-result "not_committed" operation-id "conflict_retry_exhausted"))
    :durability-ambiguous (unresolved-result "indeterminate" operation-id "restart_required_durability_ambiguous")
    :fence-mismatch (unresolved-result "not_committed" operation-id "lease_fence_mismatch")
    :not-sent (unresolved-result "not_committed" operation-id "batch_not_sent")
    (native-result-after-readback operation-id nil (:readback outcome)))))))) (unresolved-result "not_committed" operation-id "liveness_fence_mismatch")))
  (finally
    (try
  (native-rpc! 'lease-release! client fence)
  (catch Throwable _
    nil)))))))))

(defn native-classify-terminal! [subject operation-id desired expected occurrences]
  (validate-terminal! subject desired)
  (if expected (do
  (validate-publish! expected)))
  (let [before (occurrence-snapshot occurrences)
   current (committed-identity! before)]
  (cond
  (exact-committed-terminal?! subject before desired) {:result (committed-result operation-id "exact_replay")}
  (valid-committed-terminal?! subject before) {:result (unresolved-result "not_committed" operation-id "conflicting_terminal")}
  (not (and current (or (nil? expected) (identity-matches-except? current expected goal-overlay-predicates)))) {:result (unresolved-result "indeterminate" operation-id "identity_generation_changed")}
  (terminal-prefix-compatible? before desired) {:desired desired :prefix? (terminal-projection-present? before)}
  :else {:result (unresolved-result "indeterminate" operation-id "unrecognized_partial_terminal")})))

(defn native-stage-terminal-body! [client subject operation-id desired expected fence deadline]
  (let [target (assoc (terminal-occurrences! desired) terminal-marker-predicate {})]
  (loop [conflicts 0]
  (let [{:keys [served-version occurrences]} (native-rpc! 'subject-projection! client subject)
   classification (native-classify-terminal! subject operation-id desired expected occurrences)]
  (let [bind__21 (:result classification)]
  (if bind__21 (let [result bind__21]
  {:result result}) (let [actions (native-plan-occurrences! subject occurrences target terminal-marker-predicate)
   preexisting? (:prefix? classification)
   outcome (native-submit-scoped-batch! client subject occurrences target actions fence served-version)]
  (case (:outcome outcome)
    :conflict (if (and (< conflicts 32) (< (System/nanoTime) deadline)) (recur (inc conflicts)) {:result (unresolved-result "not_committed" operation-id "terminal_body_conflict_retry_exhausted")})
    :durability-ambiguous {:result (unresolved-result "indeterminate" operation-id "restart_required_durability_ambiguous")}
    :fence-mismatch {:result (unresolved-result "not_committed" operation-id "lease_fence_mismatch")}
    :not-sent {:result (unresolved-result "not_committed" operation-id "terminal_body_not_sent")}
    (case (get-in outcome [:readback :state])
    :committed {:body-ready? true :preexisting? preexisting?}
    :absent (if (< (System/nanoTime) deadline) (recur (inc conflicts)) {:result (unresolved-result "not_committed" operation-id "terminal_body_absent")})
    :foreign-writer {:result (unresolved-result "indeterminate" operation-id "foreign_writer_after_terminal_body")}
    {:result (unresolved-result "indeterminate" operation-id "terminal_body_readback_mismatch")})))))))))

(defn native-driver-actions-at-version! [client served-version thread subject]
  (if thread (let [projection (native-rpc! 'subject-projection! client thread)]
  (if (not (= served-version (:served-version projection))) (do
  (throw (ex-info "Store RPC driver snapshot changed" {:type :native/snapshot-conflict}))))
  (let [driver (str "@" (subs subject (count "@agent:")))
   before (:occurrences projection)
   desired {"driver" (dissoc (get before "driver" {}) driver)}]
  (native-rpc! 'plan-subject-actions thread before desired {}))) []))

(defn native-driver-released?! [client thread subject]
  (or (nil? thread) (let [driver (str "@" (subs subject (count "@agent:")))
   projection (native-rpc! 'subject-projection! client thread)]
  (not (contains? (get (:occurrences projection) "driver" {}) driver)))))

(defn native-commit-terminal! [client subject operation-id desired expected thread fence deadline]
  (let [target (terminal-occurrences! desired)]
  (loop [conflicts 0]
  (let [base (:served-version (native-rpc! 'version! client))
   projection (native-rpc! 'subject-projection! client subject)]
  (if (= base (:served-version projection)) (let [classification (native-classify-terminal! subject operation-id desired expected (:occurrences projection))]
  (let [bind__22 (:result classification)]
  (if bind__22 (let [result bind__22]
  result) (let [validation (try
  (validate-reported-run-with! (fn [entity predicates] (native-facts-at-version! client base entity predicates)) subject desired)
  :valid
  (catch Throwable error
    (if (= :native/snapshot-conflict (:type (ex-data error))) :retry (throw error))))]
  (if (= :retry validation) (if (< (System/nanoTime) deadline) (recur (inc conflicts)) (unresolved-result "not_committed" operation-id "terminal_validation_retry_exhausted")) (let [driver-actions (try
  (native-driver-actions-at-version! client base thread subject)
  (catch Throwable error
    (if (= :native/snapshot-conflict (:type (ex-data error))) ::retry (throw error))))]
  (if (= ::retry driver-actions) (if (< (System/nanoTime) deadline) (recur (inc conflicts)) (unresolved-result "not_committed" operation-id "terminal_driver_retry_exhausted")) (let [terminal-actions (native-plan-occurrences! subject (:occurrences projection) target terminal-marker-predicate)
   actions (vec (concat driver-actions terminal-actions))
   outcome (native-submit-scoped-batch! client subject (:occurrences projection) target actions fence base)]
  (case (:outcome outcome)
    :conflict (if (and (< conflicts 32) (< (System/nanoTime) deadline)) (recur (inc conflicts)) (unresolved-result "not_committed" operation-id "terminal_conflict_retry_exhausted"))
    :durability-ambiguous (unresolved-result "indeterminate" operation-id "restart_required_durability_ambiguous")
    :fence-mismatch (unresolved-result "not_committed" operation-id "lease_fence_mismatch")
    :not-sent (unresolved-result "not_committed" operation-id "terminal_batch_not_sent")
    (let [result (native-result-after-readback operation-id (if (:prefix? classification) (do
  "recovered_killed_prefix")) (:readback outcome))]
  (if (and (= "committed" (:status result)) (not (native-driver-released?! client thread subject))) (unresolved-result "indeterminate" operation-id "terminal_driver_readback_mismatch") result))))))))))) (if (< (System/nanoTime) deadline) (recur (inc conflicts)) (unresolved-result "not_committed" operation-id "terminal_snapshot_retry_exhausted")))))))

(defn native-terminal! [client subject operation-id desired expected thread holder presence-fence]
  (validate-terminal! subject desired)
  (let [deadline (+ (System/nanoTime) (* writer-timeout-bound-ms 1000000))
   acquire (native-acquire-lease! client subject holder)]
  (let [bind__23 (:result acquire)]
  (if bind__23 (let [result bind__23]
  (with-operation-id result operation-id)) (let [fence (:fence acquire)]
  (try
  (let [staged (native-stage-terminal-body! client subject operation-id desired expected fence deadline)]
  (let [bind__24 (:result staged)]
  (if bind__24 (let [result bind__24]
  result) (if (or (native-release-presence! client presence-fence) (:preexisting? staged)) (native-commit-terminal! client subject operation-id desired expected thread fence deadline) (unresolved-result "indeterminate" operation-id "liveness_fence_not_released")))))
  (finally
    (try
  (native-rpc! 'lease-release! client fence)
  (catch Throwable _
    nil)))))))))

(defn native-main! [args]
  (let [[port-s operation raw-subject raw supplied-holder supplied-operation-id desired-raw expected-raw terminal-thread-raw presence-fence-raw] args
   port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
   subject (entity! raw-subject)
   supplied-holder (not-empty supplied-holder)
   supplied-operation-id (not-empty supplied-operation-id)
   operation-id (or supplied-operation-id (str (java.util.UUID/randomUUID)))
   _ (if (nil? (re-matches uuid-v4-pattern operation-id)) (do
  (fail! "invalid managed agent logical operation id" {:operation-id operation-id})))
   managed-recovery? (not (str/blank? supplied-operation-id))
   delta (payload! raw)
   desired (if managed-recovery? (optional-payload! desired-raw) delta)
   expected (optional-payload! expected-raw)
   terminal-thread (terminal-thread! terminal-thread-raw)
   presence-fence (if (contains? #{"goal" "terminal"} operation) (do
  (presence-fence! presence-fence-raw subject)))]
  (if (not (contains? #{"publish" "route" "goal" "terminal"} operation)) (do
  (fail! "unsupported managed agent fact operation" {:operation operation})))
  (require-write-lease-policy!)
  (let [holder (validated-writer-holder! supplied-holder)
   client (native-connect! port)]
  (try
  (println (json/generate-string {:ok true :result (case operation
    ("publish" "route") (native-publish-identity! client subject operation operation-id delta desired expected holder)
    "goal" (native-goal! client subject operation-id delta holder presence-fence)
    "terminal" (native-terminal! client subject operation-id delta expected terminal-thread holder presence-fence)
    (throw (IllegalArgumentException. (str "No matching clause: " operation))))}))
  (finally
    (try
  (native-rpc! 'close! client)
  (catch Throwable _
    nil)))))))

(if (= *file* (System/getProperty "babashka.file")) (do
  (native-main! *command-line-args*)))
