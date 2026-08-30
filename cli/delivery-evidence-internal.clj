(ns north.delivery-evidence-internal
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.set :as set]
            [clojure.string :as str]))

(load-file (str (.getParent (io/file *file*)) "/coord.clj"))

(load-file (str (.getParent (io/file *file*)) "/terminal-projection.clj"))

(defn fail! [message data]
  (throw (ex-info message data)))

(defn checked! [result operation]
  (if (:reject result) (do
  (fail! (if (or (= :deadline (:reject result)) (:deadline result)) "delivery evidence publication deadline exceeded" "Beagle Store rejected delivery evidence write") {:operation operation})))
  result)

(defn parse-request! [raw]
  (let [raw (str raw)]
  (if (not (and (north.terminal-projection/valid-unicode-scalars? raw) (<= (north.terminal-projection/utf8-byte-count raw) north.terminal-projection/max-delivery-writer-request-utf8-bytes))) (do
  (fail! "delivery evidence request exceeds its UTF-8 byte limit" {})))
  (try
  (let [parsed (json/parse-string raw)]
  (if (not (map? parsed)) (do
  (fail! "delivery evidence request must be an object" {})))
  parsed)
  (catch clojure.lang.ExceptionInfo error
    (throw error))
  (catch Exception error
    (fail! "invalid delivery evidence JSON" {:cause (.getMessage error)})))))

(defn run-entity! [raw]
  (let [value (str raw)
   canonical (if (str/starts-with? value "@") value (str "@" value))]
  (if (not (north.terminal-projection/valid-run-entity? canonical)) (do
  (fail! "invalid delivery evidence run id" {:run raw})))
  canonical))

(defn agent-entity! [raw]
  (let [value (str raw)
   canonical (if (str/starts-with? value "@") value (str "@" value))]
  (if (not (north.terminal-projection/valid-agent-entity? canonical)) (do
  (fail! "invalid delivery evidence reporter" {:reporter raw})))
  canonical))

(defn thread-entity! [raw]
  (let [value (str raw)
   canonical (if (str/starts-with? value "@") value (str "@" value))]
  (if (not (north.terminal-projection/valid-thread-entity? canonical)) (do
  (fail! "invalid delivery evidence thread" {:thread raw})))
  canonical))

(defn query-rows!
  "Rows of one typed Store RPC query. A transport or evaluation failure is never\n   converted into an empty subject." [port subject query]
  (try
  (north.coord/query-rows! port query)
  (catch Exception error
    (fail! "Beagle Store did not answer a delivery evidence read" {:subject subject :cause (.getMessage error)}))))

(defn facts-of! [port subject]
  (let [rows (query-rows! port subject {:find "delivery_evidence_fact" :rules [{:head {:rel "delivery_evidence_fact" :args [{:var "p"} {:var "r"}]} :body [{:rel "triple" :args [subject {:var "p"} {:var "r"}]}]}]})]
  (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))

(defn exact-request! [request expected-keys]
  (if (not (= expected-keys (set (keys request)))) (do
  (fail! "delivery evidence request has an invalid shape" {:expected expected-keys :actual (set (keys request))}))))

(defn title-bearing-thread? [facts]
  (let [titles (get facts "title" #{})]
  (and (= 1 (count titles)) (string? (first titles)) (not (str/blank? (first titles))))))

(defn done-bar-limit-failure! [message thread facts extra]
  (let [bars (north.terminal-projection/done-bar-values facts)
   longest (reduce max 0 (keep (fn [__north_anon_1] (north.terminal-projection/utf8-byte-count (str __north_anon_1))) bars))]
  (fail! (str message " (" (count bars) " done_when facts, limit " north.terminal-projection/max-delivery-bars "; longest bar " longest " bytes, limit " north.terminal-projection/max-delivery-bar-utf8-bytes "); active bars: " (north.terminal-projection/done-bar-diagnostic bars) "; retire stale bars in one step with: north bars prune " (str/replace-first (str thread) #"^@" "")) (assoc extra :thread thread :bars (count bars)))))

(defn ensure-reservable-contract!
  "Fail with the MOST SPECIFIC cause a caller can act on. Too many bars and one\n   over-long bar are both recoverable by grooming, and both used to surface as\n   the same opaque line — or, past the raw cap, as 'invalid proof text', which\n   sent readers hunting for a malformed value that did not exist." [thread facts canonical label extra]
  (let [raw (north.terminal-projection/done-bar-values facts)]
  (if (> (count raw) north.terminal-projection/max-delivery-bars) (do
  (done-bar-limit-failure! (str label " exceeds delivery evidence limits") thread facts extra)))
  (if (not (vector? canonical)) (do
  (fail! (str label " contains invalid proof text") (assoc extra :thread thread))))
  (if (not (north.terminal-projection/bounded-done-bars? canonical true)) (do
  (done-bar-limit-failure! (str label " exceeds delivery evidence limits") thread facts extra)))))

(defn run-reservation-refusal!
  "Name the immutable subject and the reservation already holding it. A valid\n   reservation has an exact holder and content-addressed receipt; a partial or\n   unrelated run subject remains fail-closed but says that attribution is\n   unavailable instead of inventing a concurrent holder." [run facts]
  (let [holder (or (north.terminal-projection/singleton-value facts "run_reservation_agent") "unattributed")
   receipt (or (north.terminal-projection/singleton-value facts "run_reservation_manifest_sha256") "unavailable")
   reason (if (north.terminal-projection/run-reservation-valid? facts) "existing-reservation" "run-subject-not-fresh")]
  (fail! (str "run reservation refused: run=" run " holder=" holder " receipt=" receipt " reason=" reason) {:run run :holder holder :receipt receipt :reason reason})))

(defn exact-reservation-replay?
  "Is FACTS this exact caller's OWN already-published reservation?\n\n   The acknowledgement is the only thing that can be lost once publication is\n   atomic: the writer either committed all eight facts or none, so a same-run\n   retry that finds a COMPLETE, digest-valid reservation naming this exact\n   thread, reporter, and capability is replaying its own success and must be\n   handed the canonical receipt rather than a refusal. Exactness is the whole\n   guard — the subject must carry exactly the eight reservation predicates and\n   nothing else. A subset, an extra predicate (a run that already carries\n   telemetry or evidence is past reservation), or any different binding is a\n   refusal, never a replay." [facts thread reporter capability-digest]
  (and (north.terminal-projection/run-reservation-valid? facts) (= (set north.terminal-projection/run-reservation-predicates) (set (keys facts))) (= thread (north.terminal-projection/singleton-value facts "run_reservation_thread")) (= reporter (north.terminal-projection/singleton-value facts "run_reservation_agent")) (= capability-digest (north.terminal-projection/singleton-value facts "run_capability_sha256"))))

(def reservation-publication-deadline-ms 60000)

(defn reserve! [port request]
  (exact-request! request #{"run" "thread" "reporter" "capabilitySha256"})
  (let [run (run-entity! (get request "run"))
   thread (thread-entity! (get request "thread"))
   reporter (agent-entity! (get request "reporter"))
   capability-digest (get request "capabilitySha256")]
  (if (not (and (string? capability-digest) (re-matches #"^[0-9a-f]{64}$" capability-digest))) (do
  (fail! "invalid run capability digest" {})))
  (let [published (atom nil)
   plan! (fn [] (let [thread-facts (facts-of! port thread)
   baseline (north.terminal-projection/canonical-done-when thread-facts)
   contract-origin (if (seq baseline) "accepted" "worker-defined")]
  (if (not (title-bearing-thread? thread-facts)) (do
  (fail! "cannot reserve delivery evidence for a non-thread subject" {:thread thread :titles (get thread-facts "title" #{})})))
  (ensure-reservable-contract! thread thread-facts baseline "thread done_when contract" {:run run})
  (let [run-facts (facts-of! port run)
   reserved? (some (fn [__north_anon_1] (contains? run-facts __north_anon_1)) north.terminal-projection/run-reservation-predicates)]
  (if (and (seq run-facts) reserved?) (if (exact-reservation-replay? run-facts thread reporter capability-digest) {:done {:baseline (north.terminal-projection/run-reservation-done-when run-facts) :contract-origin (north.terminal-projection/singleton-value run-facts "run_reservation_contract_origin")}} (run-reservation-refusal! run run-facts)) (let [projection (sorted-map "run_capability_sha256" capability-digest "run_reservation_agent" reporter "run_reservation_contract_origin" contract-origin "run_reservation_done_when" (json/generate-string baseline) "run_reservation_thread" thread "run_reservation_version" north.terminal-projection/run-reservation-version "run_reserved_at" (str (java.time.Instant/now)))
   marker (north.terminal-projection/run-reservation-manifest-sha256 projection)]
  (reset! published {:baseline baseline :contract-origin contract-origin})
  {:facts (conj (mapv (fn [[predicate value]] {:p predicate :r value}) projection) {:p "run_reservation_manifest_sha256" :r marker})})))))
   outcome (north.coord/assert-batch-after-read! port run plan! Integer/MAX_VALUE (north.coord/retry-deadline-ns reservation-publication-deadline-ms))
   replay (:done outcome)]
  (checked! outcome [:assert-batch-at-version run])
  (let [own-reservation? (fn [facts] (and (north.terminal-projection/run-reservation-valid? facts) (= thread (north.terminal-projection/singleton-value facts "run_reservation_thread")) (= reporter (north.terminal-projection/singleton-value facts "run_reservation_agent")) (= capability-digest (north.terminal-projection/singleton-value facts "run_capability_sha256"))))
   stored (loop [attempt 1]
  (let [observed (facts-of! port run)]
  (if (or (own-reservation? observed) (>= attempt 5)) observed (do
  (Thread/sleep (* attempt 200))
  (recur (inc attempt))))))]
  (if (not (own-reservation? stored)) (do
  (fail! "run reservation lost singleton/freshness race" {:run run :stored stored}))))
  (let [{:keys [baseline contract-origin]} (or replay (deref published))]
  (println (json/generate-string (sorted-map "baselineDoneWhen" baseline "contractOrigin" contract-origin "ok" true "reporter" reporter "run" run "thread" thread)))))))

(defn validate-record-context! [port run thread reporter capability bar observed & $beagle$rest$host]
  (let [options (vec $beagle$rest$host)]
  (let [require-open-context? (first options)
   reservation (facts-of! port run)
   evidence-state (north.terminal-projection/run-evidence-state reservation run thread reporter)]
  (if (not (north.terminal-projection/run-reservation-valid? reservation)) (do
  (fail! "run has no valid committed reservation" {:run run})))
  (if (not (= #{reporter} (get reservation "run_reservation_agent"))) (do
  (fail! "run reservation reporter mismatch" {:run run :reporter reporter})))
  (if (not (= #{thread} (get reservation "run_reservation_thread"))) (do
  (fail! "run reservation thread mismatch" {:run run :thread thread})))
  (if (not (= #{(north.terminal-projection/sha256 capability)} (get reservation "run_capability_sha256"))) (do
  (fail! "run evidence capability mismatch" {:run run})))
  (if (not (:valid? evidence-state)) (do
  (fail! "run contains malformed, cross-scoped, duplicate, or excessive evidence" {:run run})))
  (let [stored (:entries evidence-state)
   existing (first (filter (fn [__north_anon_1] (= bar (get (second __north_anon_1) "bar"))) stored))]
  (if (and existing (= observed (get (second existing) "observed")) (not require-open-context?)) {:existing (first existing) :stored stored} (let [thread-facts (facts-of! port thread)
   active-bars (north.terminal-projection/canonical-done-when thread-facts)
   baseline (north.terminal-projection/run-reservation-done-when reservation)
   origin (north.terminal-projection/singleton-value reservation "run_reservation_contract_origin")]
  (if (contains? reservation "kind") (do
  (fail! "run evidence is closed after terminal publication" {:run run})))
  (ensure-reservable-contract! thread thread-facts active-bars "active done_when contract" {:run run})
  (if (and (= "accepted" origin) (not= baseline active-bars)) (do
  (fail! "accepted done_when contract changed during the run" {:run run :thread thread})))
  (if (not (contains? (set active-bars) bar)) (do
  (fail! "evidence bar is not an active done_when on the reserved thread" {:run run :thread thread :bar bar})))
  (if (and existing (not= observed (get (second existing) "observed"))) {:supersede (first existing) :superseded-observed (get (second existing) "observed") :stored stored} (do
  (if (>= (count stored) north.terminal-projection/max-delivery-bars) (do
  (fail! "run evidence record cap reached" {:run run})))
  (if existing {:existing (first existing) :stored stored} {:stored stored})))))))))

(def evidence-lease-wait-budget-ms 15000)

(def evidence-lease-ttl-ms 30000)

(defn proof-transport-failure! [run bar phase detail]
  (fail! (str "PROOF_TRANSPORT_FAILURE: run-bound proof publication was not " "acknowledged; do not repeat the task") {:type :proof-transport-failure :retryable false :run run :bar bar :phase phase :detail detail}))

(defn evidence-lease-resource [run bar]
  (str "delivery-evidence:" (north.terminal-projection/sha256 (str run "\u0000" bar))))

(defn acquire-evidence-lease! [port run bar]
  (let [resource (evidence-lease-resource run bar)
   holder (str "delivery-evidence-writer:" (java.util.UUID/randomUUID))
   attempt! (fn [] (let [result (try
  (north.coord/acquire-lease! port resource holder evidence-lease-ttl-ms)
  (catch Exception error
    (proof-transport-failure! run bar :acquire (.getMessage error))))]
  (cond
  (:epoch result) {:done {:resource resource :holder holder :epoch (:epoch result)}}
  (= :held (:reject result)) {:reject :conflict}
  :else (proof-transport-failure! run bar :acquire (pr-str result)))))
   outcome (north.coord/retry-conflicts-until! (north.coord/retry-deadline-ns evidence-lease-wait-budget-ms) attempt!)]
  (let [bind__0 (:done outcome)]
  (if bind__0 (let [lease bind__0]
  lease) (proof-transport-failure! run bar :acquire (str "run/bar evidence lease unavailable after " evidence-lease-wait-budget-ms "ms"))))))

(defn release-evidence-lease! [port {:keys [resource holder epoch]}]
  (try
  (north.coord/release-lease! port {:resource resource :holder holder :epoch epoch})
  (catch Exception _
    nil)))

(defn fenced-proof-write! [port lease run bar phase operation]
  (let [result (try
  (let [action-op (:op operation)]
  (if (not (contains? #{:assert :retract} action-op)) (do
  (throw (ex-info "unsupported fenced proof action" {:operation action-op}))))
  (north.coord/transact! port [{:op action-op :subject (:subject operation) :predicate (:predicate operation) :value (:value operation)}] {:fence lease}))
  (catch Exception error
    (proof-transport-failure! run bar phase (.getMessage error))))]
  (if (not (:ok result)) (do
  (proof-transport-failure! run bar phase (pr-str result))))
  result))

(defn confirm-proof-context! [port run thread reporter capability bar observed raw]
  (let [confirmed (try
  (validate-record-context! port run thread reporter capability bar observed true)
  (catch Exception error
    (if (str/starts-with? (.getMessage error) "Beagle Store did not answer a delivery evidence read") (proof-transport-failure! run bar :confirm (.getMessage error)) (throw error))))]
  (if (not (= raw (:existing confirmed))) (do
  (fail! "run evidence context changed during fenced publication" {:run run :bar bar})))
  confirmed))

(defn commit-record-once!
  "Commit one observation for RUN/BAR and report which observations it replaced.\n   Supersession retracts the stale record BEFORE asserting the correction: the\n   opposite order would leave a two-records-for-one-bar window, and every reader\n   (run-evidence-state, terminal publication) treats a duplicate bar as a\n   tampered set — a transient gap in one bar is recoverable, an invalid set is\n   not. The run/bar lease keeps supported writers outside that gap.\n\n   A coordinator-owned run/bar lease serializes validation plus the fence-checked\n   mutation. This is deliberately narrower than a global graph version: writes\n   unrelated to RUN cannot make valid proof publication lose a race." [port run thread reporter capability bar observed raw]
  (let [lease (acquire-evidence-lease! port run bar)]
  (try
  (let [context (validate-record-context! port run thread reporter capability bar observed)
   superseded (let [bind__1 (:supersede context)]
  (if bind__1 (let [stale bind__1]
  (do
  (fenced-proof-write! port lease run bar :retract {:op :retract :subject run :predicate "run_bar_evidence" :value stale})
  [(:superseded-observed context)])) []))]
  (let [bind__2 (:existing context)]
  (if bind__2 (let [existing bind__2]
  {:raw existing :superseded []}) (do
  (fenced-proof-write! port lease run bar :assert {:op :assert :subject run :predicate "run_bar_evidence" :value raw})
  (confirm-proof-context! port run thread reporter capability bar observed raw)
  {:raw raw :superseded superseded}))))
  (finally
    (release-evidence-lease! port lease)))))

(defn best-effort-thread-projection! [port thread bar observed superseded]
  (doseq [stale superseded
   :when (not= stale observed)]
  (try
  (north.coord/retract! port thread "bar_evidence" (str bar " → " stale))
  (catch Exception _
    nil)))
  (try
  (north.coord/append! port thread "bar_evidence" (str bar " → " observed))
  (catch Exception _
    nil))
  nil)

(defn record! [port request]
  (exact-request! request #{"run" "thread" "reporter" "capability" "bar" "observed"})
  (let [run (run-entity! (get request "run"))
   thread (thread-entity! (get request "thread"))
   reporter (agent-entity! (get request "reporter"))
   capability (get request "capability")
   raw-bar (get request "bar")
   raw-observed (get request "observed")
   bar (north.terminal-projection/canonical-evidence-text raw-bar)
   observed (north.terminal-projection/canonical-evidence-text raw-observed)]
  (if (not (and (string? capability) (not (str/blank? capability)))) (do
  (fail! "run evidence capability is missing" {})))
  (if (not (north.terminal-projection/bounded-nonblank-text? bar north.terminal-projection/max-delivery-bar-utf8-bytes)) (do
  (fail! "done-bar must be nonblank and within its UTF-8 byte limit" {})))
  (if (not (north.terminal-projection/bounded-nonblank-text? observed north.terminal-projection/max-delivery-observed-utf8-bytes)) (do
  (fail! "observed result must be nonblank and within its UTF-8 byte limit" {})))
  (let [record (sorted-map "bar" bar "observed" observed "recordedAt" (str (java.time.Instant/now)) "reporter" reporter "run" run "thread" thread "version" north.terminal-projection/run-bar-evidence-version)
   raw (json/generate-string record)]
  (if (not (north.terminal-projection/run-bar-evidence-valid? record)) (do
  (fail! "internal run evidence record failed validation" {:record record})))
  (let [{:keys [raw superseded]} (commit-record-once! port run thread reporter capability bar observed raw)
   committed raw]
  (best-effort-thread-projection! port thread bar observed superseded)
  (println committed)))))

(defn record-unreserved! [port request]
  (exact-request! request #{"thread" "bar" "observed"})
  (let [thread (thread-entity! (get request "thread"))
   bar (north.terminal-projection/canonical-evidence-text (get request "bar"))
   observed (north.terminal-projection/canonical-evidence-text (get request "observed"))]
  (if (not (north.terminal-projection/bounded-nonblank-text? bar north.terminal-projection/max-unreserved-bar-utf8-bytes)) (do
  (fail! "done-bar must be nonblank and within its UTF-8 byte limit" {})))
  (if (not (north.terminal-projection/bounded-nonblank-text? observed north.terminal-projection/max-delivery-observed-utf8-bytes)) (do
  (fail! "observed result must be nonblank and within its UTF-8 byte limit" {})))
  (let [facts (facts-of! port thread)
   active (north.terminal-projection/active-done-bar-texts facts)]
  (if (not (title-bearing-thread? facts)) (do
  (fail! "cannot record unreserved evidence for a non-thread subject" {:thread thread :titles (get facts "title" #{})})))
  (if (not (contains? (set active) bar)) (do
  (fail! (str "evidence bar is not an active done_when on the thread; " "active bars: " (north.terminal-projection/done-bar-diagnostic active)) {:thread thread :bar bar})))
  (let [literal (north.terminal-projection/unreserved-bar-evidence-literal bar observed)
   prefix (north.terminal-projection/unreserved-bar-evidence-prefix bar)
   stale (filter (fn [__north_anon_1] (and (string? __north_anon_1) (str/starts-with? __north_anon_1 prefix) (not= __north_anon_1 literal))) (get facts north.terminal-projection/unreserved-bar-evidence-predicate #{}))]
  (if (not literal) (do
  (fail! "internal unreserved evidence literal failed validation" {})))
  (checked! (north.coord/append! port thread "bar_evidence_unreserved" literal) [:append thread "bar_evidence_unreserved" literal])
  (doseq [value stale]
  (try
  (north.coord/retract! port thread "bar_evidence_unreserved" value)
  (catch Exception _
    nil)))
  (println (json/generate-string (sorted-map "bar" bar "observed" observed "recordedAt" (str (java.time.Instant/now)) "scope" "unreserved" "superseded" (count stale) "thread" thread "version" north.terminal-projection/unreserved-bar-evidence-version)))))))

(def execution-attempt-version "north:execution-attempt:v1")

(def execution-attempt-launch-version "north:execution-attempt-launch-intent:v1")

(def execution-attempt-provider-start-version "north:execution-attempt-provider-start:v1")

(def execution-attempt-unsent-version "north:execution-attempt-unsent:v1")

(defn attempt-digest? [value]
  (and (string? value) (boolean (re-matches #"^[0-9a-f]{64}$" value))))

(defn canonical-json [value]
  (json/generate-string (cond
  (map? value) (into (sorted-map) (map (fn [[key item]] [(str key) item])) value)
  :else value)))

(defn execution-attempt-subject [manifest]
  (str "@attempt:" manifest))

(defn attempt-lease! [value resource]
  (if (not (and (map? value) (= #{"resource" "holder" "epoch"} (set (keys value))) (= resource (get value "resource")) (string? (get value "holder")) (not (str/blank? (get value "holder"))) (integer? (get value "epoch")) (<= 1 (get value "epoch") 9007199254740991))) (do
  (fail! "execution attempt lease is malformed" {:resource resource})))
  {:resource resource :holder (get value "holder") :epoch (get value "epoch")})

(defn live-attempt-lease! [port lease]
  (let [status (try
  (north.coord/check-lease! port lease)
  (catch Exception error
    (fail! "execution attempt lease could not be checked" {:resource (:resource lease) :cause (.getMessage error)})))]
  (if (not (:valid? status)) (do
  (fail! "execution attempt lease is not live" {:resource (:resource lease)})))
  lease))

(defn attempt-account! [port account authority]
  (if (not (and (string? account) (re-matches #"^[a-z0-9][a-z0-9_-]{0,63}$" account) (attempt-digest? authority))) (do
  (fail! "execution attempt account authority is malformed" {})))
  (let [facts (facts-of! port (str "@account:" account))]
  (if (not (and (= "provider_account" (north.terminal-projection/singleton-value facts "kind")) (= account (north.terminal-projection/singleton-value facts "account_id")) (= "openai" (north.terminal-projection/singleton-value facts "provider")) (= "execution" (north.terminal-projection/singleton-value facts "account_role")) (= "true" (north.terminal-projection/singleton-value facts "execution_eligible")))) (do
  (fail! "execution attempt account lacks an execution Store role" {:account account})))))

(defn attempt-subjects-for-run! [port run]
  (->> (query-rows! port run {:find "execution_attempt_subject" :rules [{:head {:rel "execution_attempt_subject" :args [{:var "a"}]} :body [{:rel "triple" :args [{:var "a"} "execution_attempt_run" run]}]}]}) (map first) distinct sort vec))

(defn singleton! [facts predicate label]
  (let [value (north.terminal-projection/singleton-value facts predicate)]
  (if (not (string? value)) (do
  (fail! (str "execution attempt has no exact " label) {:predicate predicate})))
  value))

(defn attempt-ack! [facts baseline origin]
  (let [manifest (singleton! facts "execution_attempt_manifest_sha256" "manifest")
   predecessor (north.terminal-projection/singleton-value facts "execution_attempt_predecessor_sha256")]
  (if (not (attempt-digest? manifest)) (do
  (fail! "execution attempt manifest is malformed" {})))
  (json/generate-string (sorted-map "accountAuthorityReceiptSha256" (singleton! facts "execution_attempt_account_authority_sha256" "account authority receipt") "accountId" (singleton! facts "execution_attempt_account" "account") "accountLease" (json/parse-string (singleton! facts "execution_attempt_account_lease" "account lease")) "attemptId" (execution-attempt-subject manifest) "attemptOrdinal" (Long/parseLong (singleton! facts "execution_attempt_ordinal" "ordinal")) "baselineDoneWhen" baseline "contractOrigin" origin "manifestSha256" manifest "model" (singleton! facts "execution_attempt_model" "model") "ok" true "predecessorReceiptSha256" (or predecessor nil) "provider" (singleton! facts "execution_attempt_provider" "provider") "reporter" (singleton! facts "execution_attempt_reporter" "reporter") "routeObservationReceiptSha256" (singleton! facts "execution_attempt_route_observation_sha256" "route receipt") "run" (singleton! facts "execution_attempt_run" "run") "thread" (singleton! facts "execution_attempt_thread" "thread") "threadLease" (json/parse-string (singleton! facts "execution_attempt_thread_lease" "thread lease"))))))

(defn reserve! [port request]
  (exact-request! request #{"run" "thread" "reporter" "capabilitySha256" "provider" "accountId" "model" "accountAuthorityReceiptSha256" "routeObservationReceiptSha256" "threadLease" "accountLease"})
  (let [run (run-entity! (get request "run"))
   thread (thread-entity! (get request "thread"))
   reporter (agent-entity! (get request "reporter"))
   capability (get request "capabilitySha256")
   provider (get request "provider")
   account (get request "accountId")
   model (get request "model")
   authority (get request "accountAuthorityReceiptSha256")
   route-receipt (get request "routeObservationReceiptSha256")
   thread-lease (attempt-lease! (get request "threadLease") (str "thread:" (subs thread 8) ":dispatch"))
   account-lease (attempt-lease! (get request "accountLease") (str "codex-account:" account ":slot:0"))]
  (if (not (and (attempt-digest? capability) (attempt-digest? route-receipt) (contains? #{"anthropic" "openai"} provider) (string? model) (not (str/blank? model)) (north.terminal-projection/valid-unicode-scalars? model))) (do
  (fail! "execution attempt route is malformed" {})))
  (let [published (atom nil)
   outcome (north.coord/retry-conflicts-until! (north.coord/retry-deadline-ns reservation-publication-deadline-ms) (fn [] (let [base (north.coord/cur-ver! port)
   thread-facts (facts-of! port thread)
   baseline (north.terminal-projection/canonical-done-when thread-facts)
   origin (if (seq baseline) "accepted" "worker-defined")
   run-facts (facts-of! port run)
   attempts (attempt-subjects-for-run! port run)]
  (if (not (title-bearing-thread? thread-facts)) (do
  (fail! "cannot reserve execution attempt for a non-thread subject" {:thread thread})))
  (ensure-reservable-contract! thread thread-facts baseline "thread done_when contract" {:run run})
  (attempt-account! port account authority)
  (live-attempt-lease! port thread-lease)
  (live-attempt-lease! port account-lease)
  (cond
  (> (count attempts) 1) (fail! "execution attempt run has conflicting immutable subjects" {:run run})
  (= 1 (count attempts)) (let [facts (facts-of! port (first attempts))]
  (if (not (and (north.terminal-projection/run-reservation-valid? run-facts) (= run (singleton! facts "execution_attempt_run" "run")) (= thread (singleton! facts "execution_attempt_thread" "thread")) (= reporter (singleton! facts "execution_attempt_reporter" "reporter")) (= capability (singleton! facts "execution_attempt_run_capability_sha256" "capability")) (= provider (singleton! facts "execution_attempt_provider" "provider")) (= account (singleton! facts "execution_attempt_account" "account")) (= model (singleton! facts "execution_attempt_model" "model")) (= authority (singleton! facts "execution_attempt_account_authority_sha256" "authority")) (= route-receipt (singleton! facts "execution_attempt_route_observation_sha256" "route receipt")) (= (canonical-json thread-lease) (singleton! facts "execution_attempt_thread_lease" "thread lease")) (= (canonical-json account-lease) (singleton! facts "execution_attempt_account_lease" "account lease")))) (do
  (fail! "execution attempt reservation conflicts with an immutable attempt" {:run run})))
  {:done {:facts facts :baseline baseline :origin origin}})
  (some (fn [__north_anon_1] (contains? run-facts __north_anon_1)) north.terminal-projection/run-reservation-predicates) (run-reservation-refusal! run run-facts)
  :else (let [run-projection (sorted-map "run_capability_sha256" capability "run_reservation_agent" reporter "run_reservation_contract_origin" origin "run_reservation_done_when" (json/generate-string baseline) "run_reservation_thread" thread "run_reservation_version" north.terminal-projection/run-reservation-version "run_reserved_at" (str (java.time.Instant/now)))
   run-marker (north.terminal-projection/run-reservation-manifest-sha256 run-projection)
   body (sorted-map "kind" "execution_attempt" "execution_attempt_version" execution-attempt-version "execution_attempt_run" run "execution_attempt_thread" thread "execution_attempt_reporter" reporter "execution_attempt_ordinal" "1" "execution_attempt_provider" provider "execution_attempt_account" account "execution_attempt_model" model "execution_attempt_account_authority_sha256" authority "execution_attempt_route_observation_sha256" route-receipt "execution_attempt_thread_lease" (canonical-json thread-lease) "execution_attempt_account_lease" (canonical-json account-lease) "execution_attempt_run_capability_sha256" capability "execution_attempt_run_contract_sha256" run-marker "execution_attempt_reserved_at" (str (java.time.Instant/now)))
   marker (north.terminal-projection/sha256 (canonical-json body))
   attempt (execution-attempt-subject marker)
   facts (assoc body "execution_attempt_manifest_sha256" marker)]
  (reset! published {:facts (reduce (fn [out [p r]] (conj out {:op :assert :subject run :predicate p :value r})) [] run-projection) :attempt (reduce (fn [out [p r]] (conj out {:op :assert :subject attempt :predicate p :value r})) [] facts) :baseline baseline :origin origin})
  (north.coord/transact! port (into (:facts (deref published)) (:attempt (deref published))) {:expected-version base}))))))]
  (checked! outcome [:execution-attempt-reserve run])
  (let [{:keys [facts baseline origin]} (or (:done outcome) (let [{:keys [attempt baseline origin]} (deref published)]
  {:facts (facts-of! port (:subject (first attempt))) :baseline baseline :origin origin}))]
  (println (attempt-ack! facts baseline origin))))))

(defn attempt-transition! [port operation request]
  (let [required (case operation
    "launch-intent" #{"capability" "run" "attempt" "manifestSha256"}
    "provider-start" #{"providerStartReceiptSha256" "capability" "run" "attempt" "manifestSha256" "launchIntentSha256"}
    "proved-unsent" #{"capability" "run" "unsentReceiptSha256" "attempt" "manifestSha256" "launchIntentSha256"}
    "attempt-terminal" #{"capability" "run" "providerStartManifestSha256" "terminalReceiptSha256" "attempt" "manifestSha256" "launchIntentSha256"}
    (throw (IllegalArgumentException. (str "No matching clause: " operation))))]
  (exact-request! request required)
  (let [attempt (str (get request "attempt"))
   run (run-entity! (get request "run"))
   manifest (get request "manifestSha256")
   capability (get request "capability")
   receipt-key (case operation
    "provider-start" "providerStartReceiptSha256"
    "proved-unsent" "unsentReceiptSha256"
    "attempt-terminal" "terminalReceiptSha256"
    (throw (IllegalArgumentException. (str "No matching clause: " operation))))
   receipt (if receipt-key (do
  (get request receipt-key)))]
  (if (not (and (= attempt (execution-attempt-subject manifest)) (attempt-digest? manifest) (string? capability) (not (str/blank? capability)) (or (nil? receipt) (attempt-digest? receipt)))) (do
  (fail! "execution attempt transition is malformed" {})))
  (let [outcome (north.coord/assert-batch-after-read! port attempt (fn [] (let [facts (facts-of! port attempt)
   same? (and (= run (singleton! facts "execution_attempt_run" "run")) (= manifest (singleton! facts "execution_attempt_manifest_sha256" "manifest")) (= (north.terminal-projection/sha256 capability) (singleton! facts "execution_attempt_run_capability_sha256" "capability")))]
  (if (not same?) (do
  (fail! "execution attempt transition does not own the immutable reservation" {:attempt attempt})))
  (if (and (contains? #{"provider-start" "proved-unsent" "attempt-terminal"} operation) (not= (get request "launchIntentSha256") (north.terminal-projection/singleton-value facts "execution_attempt_launch_intent_sha256"))) (do
  (fail! "execution attempt transition lacks its immutable launch intent" {:attempt attempt})))
  (if (and (= operation "attempt-terminal") (not= (get request "providerStartManifestSha256") (north.terminal-projection/singleton-value facts "execution_attempt_provider_start_manifest_sha256"))) (do
  (fail! "execution attempt terminal lacks its immutable provider start" {:attempt attempt})))
  (if (and (= operation "proved-unsent") (or (contains? facts "execution_attempt_provider_start_manifest_sha256") (contains? facts "execution_attempt_terminal_manifest_sha256"))) (do
  (fail! "execution attempt proved-unsent conflicts with a started or terminal attempt" {:attempt attempt})))
  (if (and (= operation "attempt-terminal") (contains? facts "execution_attempt_unsent_manifest_sha256")) (do
  (fail! "execution attempt terminal conflicts with proved-unsent" {:attempt attempt})))
  (let [existing (case operation
    "launch-intent" (north.terminal-projection/singleton-value facts "execution_attempt_launch_intent_sha256")
    "provider-start" (north.terminal-projection/singleton-value facts "execution_attempt_provider_start_manifest_sha256")
    "proved-unsent" (north.terminal-projection/singleton-value facts "execution_attempt_unsent_manifest_sha256")
    "attempt-terminal" (north.terminal-projection/singleton-value facts "execution_attempt_terminal_manifest_sha256")
    (throw (IllegalArgumentException. (str "No matching clause: " operation))))]
  (if existing {:done {:facts facts}} (let [now (str (java.time.Instant/now))
   additions (case operation
    "launch-intent" (let [m (north.terminal-projection/sha256 (canonical-json {"version" execution-attempt-launch-version "attempt" attempt "at" now}))]
  [["execution_attempt_launch_intent_version" execution-attempt-launch-version] ["execution_attempt_launch_intent_at" now] ["execution_attempt_launch_intent_sha256" m]])
    "provider-start" (let [m (north.terminal-projection/sha256 (canonical-json {"version" execution-attempt-provider-start-version "launch" (get request "launchIntentSha256") "receipt" receipt "at" now}))]
  [["execution_attempt_provider_start_receipt_sha256" receipt] ["execution_attempt_provider_started_at" now] ["execution_attempt_provider_start_manifest_sha256" m]])
    "proved-unsent" (let [m (north.terminal-projection/sha256 (canonical-json {"version" execution-attempt-unsent-version "launch" (get request "launchIntentSha256") "receipt" receipt "at" now}))]
  [["execution_attempt_unsent_receipt_sha256" receipt] ["execution_attempt_unsent_at" now] ["execution_attempt_unsent_manifest_sha256" m]])
    "attempt-terminal" (let [m (north.terminal-projection/sha256 (canonical-json {"launch" (get request "launchIntentSha256") "start" (get request "providerStartManifestSha256") "receipt" receipt "at" now}))]
  [["execution_attempt_terminal_receipt_sha256" receipt] ["execution_attempt_terminal_at" now] ["execution_attempt_terminal_manifest_sha256" m]])
    (throw (IllegalArgumentException. (str "No matching clause: " operation))))]
  {:facts (mapv (fn [[p r]] {:p p :r r}) additions)}))))))
   _ (checked! outcome [:execution-attempt-transition operation attempt])
   facts (or (get-in outcome [:done :facts]) (facts-of! port attempt))]
  (let [ack (case operation
    "launch-intent" {"ok" true "attempt" attempt "launchIntentSha256" (singleton! facts "execution_attempt_launch_intent_sha256" "launch intent") "launchedAt" (singleton! facts "execution_attempt_launch_intent_at" "launch time")}
    "provider-start" {"ok" true "attempt" attempt "providerStartReceiptSha256" (singleton! facts "execution_attempt_provider_start_receipt_sha256" "provider start receipt") "providerStartManifestSha256" (singleton! facts "execution_attempt_provider_start_manifest_sha256" "provider start manifest") "providerStartedAt" (singleton! facts "execution_attempt_provider_started_at" "provider start time")}
    "proved-unsent" {"ok" true "attempt" attempt "unsentReceiptSha256" (singleton! facts "execution_attempt_unsent_receipt_sha256" "unsent receipt") "unsentManifestSha256" (singleton! facts "execution_attempt_unsent_manifest_sha256" "unsent manifest") "unsentAt" (singleton! facts "execution_attempt_unsent_at" "unsent time")}
    "attempt-terminal" {"ok" true "attempt" attempt "terminalReceiptSha256" (singleton! facts "execution_attempt_terminal_receipt_sha256" "terminal receipt") "terminalManifestSha256" (singleton! facts "execution_attempt_terminal_manifest_sha256" "terminal manifest") "terminalAt" (singleton! facts "execution_attempt_terminal_at" "terminal time")}
    (throw (IllegalArgumentException. (str "No matching clause: " operation))))]
  (if (and receipt (not= receipt (get ack receipt-key))) (do
  (fail! "execution attempt transition conflicts with its immutable receipt" {:attempt attempt})))
  (if (contains? #{"proved-unsent" "attempt-terminal"} operation) (do
  (doseq [predicate ["execution_attempt_thread_lease" "execution_attempt_account_lease"]]
  (try
  (north.coord/release-lease! port (json/parse-string (singleton! facts predicate "lease")))
  (catch Exception _
    nil)))))
  (println (canonical-json ack)))))))

(defn -main []
  (let [[port-s operation raw] *command-line-args*
   port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
   request (parse-request! (if (some? raw) raw (slurp *in*)))]
  (case operation
    "reserve" (reserve! port request)
    "launch-intent" (attempt-transition! port "launch-intent" request)
    "provider-start" (attempt-transition! port "provider-start" request)
    "proved-unsent" (attempt-transition! port "proved-unsent" request)
    "attempt-terminal" (attempt-transition! port "attempt-terminal" request)
    "record" (record! port request)
    "record-unreserved" (record-unreserved! port request)
    (fail! "unsupported delivery evidence operation" {:operation operation}))))

(if (= *file* (System/getProperty "babashka.file")) (do
  (-main)))
