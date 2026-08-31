(ns north.work-occurrences-test
  (:require [north.referents :as referents]
            [north.work-occurrences :as work]
            [store.types :as t]))

(defrecord Check [label passed])

(defn check-label [r] (:label r))

(defn check-passed [r] (:passed r))

(def checks (atom []))

(defn check! [^String label value]
  (do
  (swap! checks conj (->Check label (boolean value)))
  nil))

(defn rejected-type [operation]
  (try
  (do
  (operation)
  nil)
  (catch Throwable error
    (:type (ex-data error)))))

(defn key-set [value]
  (set (keys value)))

(defn action-values [plan ^String predicate]
  (mapv (fn [action] (:value action)) (filterv (fn [action] (= predicate (:predicate action))) (work/publication-actions plan))))

(defn ^Boolean has-action? [plan ^String predicate ^String value]
  (= [value] (action-values plan predicate)))

(defn plan-facts [plan]
  (mapv (fn [action] (t/triple (:subject action) (:predicate action) (:value action))) (work/publication-actions plan)))

(defn canonical-result [plan ^String space]
  (let [version (inc (:expected-store-version plan))
   transaction (t/transaction-coordinate space version)]
  {:ok version :changed? true :results (mapv (fn [index] {:input-index index :changed? true :occurrence (t/occurrence-coordinate transaction index)}) (range (count (work/publication-actions plan))))}))

(def human-map {"kind" "human" "id" "@actor:tom"})

(def listener-map {"kind" "listener-agent" "id" "@actor:listener"})

(def worker-map {"kind" "agent-run" "id" "@actor:worker"})

(def ^String tracked-thing "@tracked:ship-cutover")

(def ^String request-id "@occ:request-1")

(def ^String ack-id "@occ:ack-1")

(def ^String result-id "@occ:result-1")

(def ^String assignment-id "@occ:assignment-1")

(def ^String accepted-id "@occ:ownership-accept-1")

(def ^String settlement-id "@occ:settlement-1")

(def ^String occurred-at "2026-08-30T18:00:00Z")

(def ^String store-space "north-coordination")

(defn snapshot-at [store-version facts]
  (work/canonical-snapshot! store-space store-version facts))

(def pending-map {"id" "offer-1" "from" listener-map "to" worker-map})

(def offered-map {"goal" tracked-thing "owner" listener-map "accountableParent" human-map "pendingOffer" pending-map})

(def accepted-map {"goal" tracked-thing "owner" worker-map "accountableParent" listener-map "pendingOffer" nil})

(def accepted-json {"version" work/work-ownership-version "before" offered-map "event" {"kind" "accept" "actor" worker-map "offerId" "offer-1"} "after" accepted-map})

(def accepted-transition (work/decode-ownership-transition! accepted-json))

(check! "strict JSON decoder returns the typed accepted transition" (and (= work/work-ownership-version (:version accepted-transition)) (= "@actor:worker" (:id (:owner (:after accepted-transition)))) (= "listener-agent" (:kind (:accountable-parent (:after accepted-transition))))))

(check! "ownership actor kind comes only from the exact closed JSON field" (= :north.work-occurrences/invalid-actor-kind (rejected-type (fn [] (work/decode-ownership-transition! (assoc-in accepted-json ["event" "actor" "kind"] "agent"))))))

(check! "ownership JSON rejects extra fields instead of widening the schema" (= :north.work-occurrences/invalid-ownership-json (rejected-type (fn [] (work/decode-ownership-transition! (assoc accepted-json "provider" "codex"))))))

(def tracker-facts (vec (concat (referents/tracked-thing-facts! "@actor:listener" "Listener" "@actor:tom" occurred-at) (referents/agent-role-facts! "@actor:listener"))))

(def worker-facts (vec (concat (referents/tracked-thing-facts! "@actor:worker" "Worker" "@actor:tom" occurred-at) (referents/agent-role-facts! "@actor:worker"))))

(def referent-facts (referents/tracked-thing-facts! tracked-thing "Ship cutover" "@actor:listener" occurred-at))

(def support-facts (vec (concat referent-facts (vec (concat tracker-facts worker-facts)))))

(def tracked (work/track-plan! tracked-thing "Ship cutover" "@actor:listener" occurred-at (snapshot-at 10 [])))

(def revision (work/plan-revision-plan! tracked-thing "@occ:revision-1" "Ship the exact cutover" "@actor:listener" occurred-at (snapshot-at 11 [])))

(def plan-snapshot (snapshot-at 12 (vec (concat support-facts (plan-facts revision)))))

(def decoded-plan (work/decode-plan-snapshot! plan-snapshot tracked-thing "@occ:revision-1"))

(def started (work/start-plan! "@occ:start-auth-1" decoded-plan "@occ:revision-1" "@actor:listener" "sig:exact-revision-1" occurred-at plan-snapshot))

(def assigned (work/assignment-plan! assignment-id tracked-thing "@actor:listener" "@actor:worker" occurred-at (snapshot-at 21 [])))

(check! "B1-backed mutation plans remain assert-only and exactly fenced" (and (= "track" (work/plan-action tracked)) (= store-space (:expected-store-space tracked)) (= {:expected-version 10} (work/publication-options tracked)) (every? (fn [action] (= :assert (:op action))) (work/publication-actions tracked)) (has-action? revision "body" "Ship the exact cutover") (has-action? started "plan_revision" "@occ:revision-1") (has-action? started "signature" "sig:exact-revision-1") (= {:expected-version 12} (work/publication-options started)) (has-action? assigned "assignee" "@actor:worker")))

(check! "start consumes the exact Plan snapshot and never selects a revision" (and (= :north.work-occurrences/plan-snapshot-mismatch (rejected-type (fn [] (work/decode-plan-snapshot! plan-snapshot tracked-thing "@occ:other-revision")))) (= :north.work-occurrences/plan-snapshot-mismatch (rejected-type (fn [] (work/start-plan! "@occ:wrong-start" decoded-plan "@occ:other-revision" "@actor:listener" "sig:caller-supplied" occurred-at plan-snapshot))))))

(def request-base-snapshot (snapshot-at 20 []))

(def request (work/request-plan! request-id tracked-thing "@actor:listener" "@actor:worker" "Please ship B2." occurred-at request-base-snapshot))

(def unscoped-request (work/request-plan! "@occ:request-unscoped" nil "@actor:listener" "@actor:worker" "Please inspect this." occurred-at request-base-snapshot))

(check! "Request is an immutable Message envelope with optional about" (and (has-action? request "entity_kind" "occurrence") (has-action? request "occurrence_kind" "request") (has-action? request "about" tracked-thing) (has-action? request "actor" "@actor:listener") (has-action? request "to" "@actor:worker") (has-action? request "body" "Please ship B2.") (empty? (action-values unscoped-request "about"))))

(def request-snapshot (snapshot-at 21 (vec (concat support-facts (plan-facts request)))))

(def decoded-request (work/decode-request-snapshot! request-snapshot request-id))

(def unscoped-request-snapshot (snapshot-at 21 (vec (concat support-facts (plan-facts unscoped-request)))))

(def decoded-unscoped-request (work/decode-request-snapshot! unscoped-request-snapshot "@occ:request-unscoped"))

(def ack (work/ack-plan! ack-id decoded-request "@actor:worker" "2026-08-30T18:01:00Z" request-snapshot))

(check! "ACK derives its about link and fence from the exact Request snapshot" (and (= 21 (:expected-store-version ack)) (has-action? ack "acknowledges" request-id) (has-action? ack "about" tracked-thing) (has-action? ack "actor" "@actor:worker") (empty? (action-values ack "ownership_contract"))))

(check! "only the exact Request recipient may ACK" (= :north.work-occurrences/invalid-ack (rejected-type (fn [] (work/ack-plan! "@occ:bad-ack" decoded-request "@actor:listener" occurred-at request-snapshot)))))

(def result-request-snapshot (snapshot-at 22 (vec (concat support-facts (vec (concat (plan-facts request) (plan-facts ack)))))))

(def result-request (work/decode-request-snapshot! result-request-snapshot request-id))

(def result (work/result-plan! result-id result-request "@actor:worker" "done" "The requested B2 work is complete." "2026-08-30T18:03:00Z" result-request-snapshot))

(def unscoped-result-plan (work/result-plan! "@occ:result-unscoped" decoded-unscoped-request "@actor:worker" "inspected" "The unscoped Request was inspected." "2026-08-30T18:04:00Z" unscoped-request-snapshot))

(check! "Result derives its recipient, about link, and fence from one Request snapshot" (and (= 22 (:expected-store-version result)) (has-action? result "occurrence_kind" "result") (has-action? result "request" request-id) (has-action? result "actor" "@actor:worker") (has-action? result "about" tracked-thing) (has-action? result "outcome" "done") (has-action? result "summary" "The requested B2 work is complete.") (empty? (action-values unscoped-result-plan "about")) (= 4 (count #{request-id ack-id result-id settlement-id}))))

(check! "Result rejects a non-recipient, a reused Request identity, and NUL text" (and (= :north.work-occurrences/invalid-result (rejected-type (fn [] (work/result-plan! "@occ:bad-result-actor" result-request "@actor:listener" "done" "wrong actor" occurred-at result-request-snapshot)))) (= :north.work-occurrences/invalid-result (rejected-type (fn [] (work/result-plan! request-id result-request "@actor:worker" "done" "reused Request" occurred-at result-request-snapshot)))) (= :north.work-occurrences/invalid-text (rejected-type (fn [] (work/result-plan! "@occ:bad-result-text" result-request "@actor:worker" "done" "bad\u0000summary" occurred-at result-request-snapshot))))))

(def ownership-plan (work/ownership-transition-plan! accepted-id accepted-transition "2026-08-30T18:02:00Z" (snapshot-at 21 [])))

(def settlement-snapshot (snapshot-at 22 (vec (concat support-facts (vec (concat (plan-facts assigned) (plan-facts ownership-plan)))))))

(def decoded-assignment (work/decode-assignment-snapshot! settlement-snapshot assignment-id))

(def decoded-accepted (work/decode-ownership-occurrence! settlement-snapshot accepted-id))

(def settlement (work/settlement-plan! settlement-id decoded-assignment decoded-accepted "@actor:worker" "done" "Candidate and focused check delivered." "2026-08-30T18:30:00Z" settlement-snapshot))

(check! "Settlement binds Assignment and accepted ownership transition" (and (= 22 (:expected-store-version settlement)) (has-action? settlement "assignment" assignment-id) (has-action? settlement "accepted_transition" accepted-id) (has-action? settlement "actor" "@actor:worker") (has-action? settlement "accountable_parent" "@actor:listener") (has-action? settlement "outcome" "done") (has-action? settlement "summary" "Candidate and focused check delivered.") (empty? (action-values settlement "ownership_after_owner"))))

(check! "Settlement rejects a reporting actor other than assignee and owner" (= :north.work-occurrences/invalid-settlement (rejected-type (fn [] (work/settlement-plan! "@occ:bad-settlement" decoded-assignment decoded-accepted "@actor:listener" "done" "wrong actor" occurred-at settlement-snapshot)))))

(check! "Settlement outcome is open text but rejects NUL" (= :north.work-occurrences/invalid-text (rejected-type (fn [] (work/settlement-plan! "@occ:bad-outcome" decoded-assignment decoded-accepted "@actor:worker" "done\u0000later" "invalid" occurred-at settlement-snapshot)))))

(def restart-snapshot (snapshot-at 24 (vec (concat support-facts (vec (concat (plan-facts request) (vec (concat (plan-facts ack) (vec (concat (plan-facts result) (vec (concat (plan-facts assigned) (vec (concat (plan-facts ownership-plan) (plan-facts settlement)))))))))))))))

(def restart-ack (work/decode-ack-snapshot! restart-snapshot ack-id))

(def restart-result (work/decode-result-snapshot! restart-snapshot result-id))

(def restart-settlement (work/decode-settlement-snapshot! restart-snapshot settlement-id))

(def restart-request (work/decode-request-snapshot! restart-snapshot request-id))

(def restart-assignment (work/decode-assignment-snapshot! restart-snapshot assignment-id))

(def restart-accepted (work/decode-ownership-occurrence! restart-snapshot accepted-id))

(def restart-history (work/semantic-history! (work/history-read-plan! tracked-thing) restart-snapshot))

(check! "committed facts reconstruct the bounded Request through Result and Settlement path" (and (= request-id (:request restart-ack)) (= request-id (:request restart-result)) (= "@actor:worker" (:reporting-actor restart-result)) (= "done" (:outcome restart-result)) (= assignment-id (:assignment restart-settlement)) (= accepted-id (:accepted-transition restart-settlement)) (= "done" (:outcome restart-settlement)) (= #{request-id ack-id result-id assignment-id accepted-id settlement-id} (set (:occurrences restart-history)))))

(def chained-occurrence-ids [request-id ack-id result-id settlement-id])

(check! "Request ACK Result and Settlement reject every occupied chain identity" (and (every? (fn [^String candidate] (some? (rejected-type (fn [] (work/request-plan! candidate tracked-thing "@actor:listener" "@actor:worker" "collision" occurred-at restart-snapshot))))) chained-occurrence-ids) (every? (fn [^String candidate] (some? (rejected-type (fn [] (work/ack-plan! candidate restart-request "@actor:worker" occurred-at restart-snapshot))))) chained-occurrence-ids) (every? (fn [^String candidate] (some? (rejected-type (fn [] (work/result-plan! candidate restart-request "@actor:worker" "done" "collision" occurred-at restart-snapshot))))) chained-occurrence-ids) (every? (fn [^String candidate] (some? (rejected-type (fn [] (work/settlement-plan! candidate restart-assignment restart-accepted "@actor:worker" "done" "collision" occurred-at restart-snapshot))))) chained-occurrence-ids)))

(def request-result (canonical-result request "north-coordination"))

(def request-receipt (work/semantic-receipt! request (work/publication-options request) request-result))

(def unscoped-request-result (canonical-result unscoped-request "north-coordination"))

(def unscoped-receipt (work/semantic-receipt! unscoped-request (work/publication-options unscoped-request) unscoped-request-result))

(def ack-receipt (work/semantic-receipt! ack (work/publication-options ack) (canonical-result ack "north-coordination")))

(def result-receipt (work/semantic-receipt! result (work/publication-options result) (canonical-result result "north-coordination")))

(def unscoped-result-receipt (work/semantic-receipt! unscoped-result-plan (work/publication-options unscoped-result-plan) (canonical-result unscoped-result-plan "north-coordination")))

(def settlement-receipt (work/semantic-receipt! settlement (work/publication-options settlement) (canonical-result settlement "north-coordination")))

(check! "Request receipts have the exact optional-about key set" (and (= #{:protocol :version :action :storeVersion :request :referent} (key-set request-receipt)) (= #{:protocol :version :action :storeVersion :request} (key-set unscoped-receipt)) (= "north.semantic-receipt" (:protocol request-receipt)) (= 1 (:version request-receipt)) (= "request" (:action request-receipt)) (= request-id (:request request-receipt)) (= tracked-thing (:referent request-receipt))))

(check! "Result receipts have only the exact snapshot-derived optional referent" (and (= #{:protocol :version :action :storeVersion :request :result :outcome :referent} (key-set result-receipt)) (= #{:protocol :version :action :storeVersion :request :result :outcome} (key-set unscoped-result-receipt)) (= "result" (:action result-receipt)) (= request-id (:request result-receipt)) (= result-id (:result result-receipt)) (= "done" (:outcome result-receipt)) (= tracked-thing (:referent result-receipt))))

(check! "ACK and Settlement receipts use their closed Bridge contracts" (and (= #{:protocol :version :action :storeVersion :request :ack} (key-set ack-receipt)) (= #{:protocol :version :action :storeVersion :assignment :acceptedTransition :settlement :outcome} (key-set settlement-receipt)) (= assignment-id (:assignment settlement-receipt)) (= accepted-id (:acceptedTransition settlement-receipt)) (= settlement-id (:settlement settlement-receipt)) (= "done" (:outcome settlement-receipt))))

(check! "receipt construction rejects publication options from another read" (= :north.work-occurrences/read-commit-race (rejected-type (fn [] (work/semantic-receipt! request {:expected-version 19} request-result)))))

(check! "receipt construction rejects incomplete canonical commit evidence" (= :north.work-occurrences/store-commit-mismatch (rejected-type (fn [] (work/semantic-receipt! request (work/publication-options request) (assoc request-result :results []))))))

(check! "receipt construction rejects coordinates from another Store space" (= :north.work-occurrences/store-commit-mismatch (rejected-type (fn [] (work/semantic-receipt! request (work/publication-options request) (canonical-result request "other-space"))))))

(check! "state-dependent read requirements are bounded and explicit" (let [request-read (work/request-read-plan! request-id)
   ack-read (work/ack-read-plan! request-id ack-id)
   result-read (work/result-read-plan! request-id result-id)
   settle-read (work/settle-read-plan! assignment-id accepted-id settlement-id)
   start-read (work/start-read-plan! tracked-thing "@occ:revision-1")]
  (and (= "request" (:action request-read)) (= [request-id] (:subjects request-read)) (= "ack" (:action ack-read)) (= [request-id ack-id] (:subjects ack-read)) (= 32 (:limit ack-read)) (= "result" (:action result-read)) (= [request-id result-id] (:subjects result-read)) (= 32 (:limit result-read)) (= [assignment-id accepted-id settlement-id] (:subjects settle-read)) (= "start" (:action start-read)) (= [tracked-thing "@occ:revision-1"] (:subjects start-read)))))

(let [results (deref checks)
   passed (count (filterv (fn [^Check result] (:passed result)) results))]
  (doseq [result results]
  (println (format "  [%s] %s" (if (:passed result) "PASS" "FAIL") (:label result))))
  (println (format "\nWork occurrences: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
