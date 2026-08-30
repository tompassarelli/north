(ns north.work-occurrences
  (:require [clojure.string :as str]
            [north.projections :as proj]
            [north.referents :as referents]
            [store.types :as t]))

(def ^String work-ownership-version "work-ownership-v1")

(def ^String semantic-receipt-protocol "north.semantic-receipt")

(def semantic-receipt-version 1)

(def ^String semantic-view-protocol "north.semantic-view")

(def ^String semantic-history-protocol "north.semantic-history")

(def ^String semantic-inbox-protocol "north.semantic-inbox")

(def ^String occurrence-entity-kind "occurrence")

(def ^String request-kind "request")

(def ^String ack-kind "ack")

(def ^String result-kind "result")

(def ^String ownership-transition-kind "ownership-transition")

(def ^String settlement-kind "settlement")

(def actor-kinds #{"human" "listener-agent" "agent-run"})

(defrecord Actor [kind id])

(defn actor-kind [r] (:kind r))

(defn actor-id [r] (:id r))

(defrecord PendingOffer [id from to])

(defn pendingoffer-id [r] (:id r))

(defn pendingoffer-from [r] (:from r))

(defn pendingoffer-to [r] (:to r))

(defrecord OwnershipState [goal owner accountable-parent pending-offer])

(defn ownershipstate-goal [r] (:goal r))

(defn ownershipstate-owner [r] (:owner r))

(defn ownershipstate-accountable-parent [r] (:accountable-parent r))

(defn ownershipstate-pending-offer [r] (:pending-offer r))

;; OwnershipEvent = Offer | Accept | Transfer | Refuse | Escalate
(defrecord Offer [actor offer-id to])

(defn offer-actor [r] (:actor r))

(defn offer-offer-id [r] (:offer-id r))

(defn offer-to [r] (:to r))
(defrecord Accept [actor offer-id])

(defn accept-actor [r] (:actor r))

(defn accept-offer-id [r] (:offer-id r))
(defrecord Transfer [actor to acknowledged-by])

(defn transfer-actor [r] (:actor r))

(defn transfer-to [r] (:to r))

(defn transfer-acknowledged-by [r] (:acknowledged-by r))
(defrecord Refuse [actor offer-id reason])

(defn refuse-actor [r] (:actor r))

(defn refuse-offer-id [r] (:offer-id r))

(defn refuse-reason [r] (:reason r))
(defrecord Escalate [actor to reason])

(defn escalate-actor [r] (:actor r))

(defn escalate-to [r] (:to r))

(defn escalate-reason [r] (:reason r))

(defrecord OwnershipTransition [version before event after])

(defn ownershiptransition-version [r] (:version r))

(defn ownershiptransition-before [r] (:before r))

(defn ownershiptransition-event [r] (:event r))

(defn ownershiptransition-after [r] (:after r))

(defrecord StoreAction [op subject predicate value])

(defn storeaction-op [r] (:op r))

(defn storeaction-subject [r] (:subject r))

(defn storeaction-predicate [r] (:predicate r))

(defn storeaction-value [r] (:value r))

;; ReceiptIntent = TrackIntent | PlanIntent | StartIntent | AssignIntent | RequestIntent | AckIntent | ResultIntent | OwnershipIntent | SettleIntent
(defrecord TrackIntent [referent])

(defn trackintent-referent [r] (:referent r))
(defrecord PlanIntent [referent revision])

(defn planintent-referent [r] (:referent r))

(defn planintent-revision [r] (:revision r))
(defrecord StartIntent [referent occurrence])

(defn startintent-referent [r] (:referent r))

(defn startintent-occurrence [r] (:occurrence r))
(defrecord AssignIntent [referent assignment])

(defn assignintent-referent [r] (:referent r))

(defn assignintent-assignment [r] (:assignment r))
(defrecord RequestIntent [request referent])

(defn requestintent-request [r] (:request r))

(defn requestintent-referent [r] (:referent r))
(defrecord AckIntent [request ack])

(defn ackintent-request [r] (:request r))

(defn ackintent-ack [r] (:ack r))
(defrecord ResultIntent [request result outcome referent])

(defn resultintent-request [r] (:request r))

(defn resultintent-result [r] (:result r))

(defn resultintent-outcome [r] (:outcome r))

(defn resultintent-referent [r] (:referent r))
(defrecord OwnershipIntent [transition owner])

(defn ownershipintent-transition [r] (:transition r))

(defn ownershipintent-owner [r] (:owner r))
(defrecord SettleIntent [assignment accepted-transition settlement outcome])

(defn settleintent-assignment [r] (:assignment r))

(defn settleintent-accepted-transition [r] (:accepted-transition r))

(defn settleintent-settlement [r] (:settlement r))

(defn settleintent-outcome [r] (:outcome r))

(defrecord PersistencePlan [action expected-store-space expected-store-version actions receipt-intent])

(defn persistenceplan-action [r] (:action r))

(defn persistenceplan-expected-store-space [r] (:expected-store-space r))

(defn persistenceplan-expected-store-version [r] (:expected-store-version r))

(defn persistenceplan-actions [r] (:actions r))

(defn persistenceplan-receipt-intent [r] (:receipt-intent r))

(defrecord CanonicalSnapshot [store-space store-version facts])

(defn canonicalsnapshot-store-space [r] (:store-space r))

(defn canonicalsnapshot-store-version [r] (:store-version r))

(defn canonicalsnapshot-facts [r] (:facts r))

(defrecord RequestSnapshot [store-space store-version request referent from to body at])

(defn requestsnapshot-store-space [r] (:store-space r))

(defn requestsnapshot-store-version [r] (:store-version r))

(defn requestsnapshot-request [r] (:request r))

(defn requestsnapshot-referent [r] (:referent r))

(defn requestsnapshot-from [r] (:from r))

(defn requestsnapshot-to [r] (:to r))

(defn requestsnapshot-body [r] (:body r))

(defn requestsnapshot-at [r] (:at r))

(defrecord PlanSnapshot [store-space store-version referent revision intended-path endorsed-by endorsed-at])

(defn plansnapshot-store-space [r] (:store-space r))

(defn plansnapshot-store-version [r] (:store-version r))

(defn plansnapshot-referent [r] (:referent r))

(defn plansnapshot-revision [r] (:revision r))

(defn plansnapshot-intended-path [r] (:intended-path r))

(defn plansnapshot-endorsed-by [r] (:endorsed-by r))

(defn plansnapshot-endorsed-at [r] (:endorsed-at r))

(defrecord AssignmentSnapshot [store-space store-version assignment referent assigned-by assignee at])

(defn assignmentsnapshot-store-space [r] (:store-space r))

(defn assignmentsnapshot-store-version [r] (:store-version r))

(defn assignmentsnapshot-assignment [r] (:assignment r))

(defn assignmentsnapshot-referent [r] (:referent r))

(defn assignmentsnapshot-assigned-by [r] (:assigned-by r))

(defn assignmentsnapshot-assignee [r] (:assignee r))

(defn assignmentsnapshot-at [r] (:at r))

(defrecord AckSnapshot [store-space store-version ack request referent actor at])

(defn acksnapshot-store-space [r] (:store-space r))

(defn acksnapshot-store-version [r] (:store-version r))

(defn acksnapshot-ack [r] (:ack r))

(defn acksnapshot-request [r] (:request r))

(defn acksnapshot-referent [r] (:referent r))

(defn acksnapshot-actor [r] (:actor r))

(defn acksnapshot-at [r] (:at r))

(defrecord ResultSnapshot [store-space store-version result request referent reporting-actor outcome summary at])

(defn resultsnapshot-store-space [r] (:store-space r))

(defn resultsnapshot-store-version [r] (:store-version r))

(defn resultsnapshot-result [r] (:result r))

(defn resultsnapshot-request [r] (:request r))

(defn resultsnapshot-referent [r] (:referent r))

(defn resultsnapshot-reporting-actor [r] (:reporting-actor r))

(defn resultsnapshot-outcome [r] (:outcome r))

(defn resultsnapshot-summary [r] (:summary r))

(defn resultsnapshot-at [r] (:at r))

(defrecord OwnershipOccurrence [store-space store-version occurrence at transition])

(defn ownershipoccurrence-store-space [r] (:store-space r))

(defn ownershipoccurrence-store-version [r] (:store-version r))

(defn ownershipoccurrence-occurrence [r] (:occurrence r))

(defn ownershipoccurrence-at [r] (:at r))

(defn ownershipoccurrence-transition [r] (:transition r))

(defrecord SettlementSnapshot [store-space store-version settlement referent assignment accepted-transition reporting-actor accountable-parent outcome summary at])

(defn settlementsnapshot-store-space [r] (:store-space r))

(defn settlementsnapshot-store-version [r] (:store-version r))

(defn settlementsnapshot-settlement [r] (:settlement r))

(defn settlementsnapshot-referent [r] (:referent r))

(defn settlementsnapshot-assignment [r] (:assignment r))

(defn settlementsnapshot-accepted-transition [r] (:accepted-transition r))

(defn settlementsnapshot-reporting-actor [r] (:reporting-actor r))

(defn settlementsnapshot-accountable-parent [r] (:accountable-parent r))

(defn settlementsnapshot-outcome [r] (:outcome r))

(defn settlementsnapshot-summary [r] (:summary r))

(defn settlementsnapshot-at [r] (:at r))

(defrecord ReadPlan [action mode identity subjects predicates follow-predicates followed-predicates limit])

(defn readplan-action [r] (:action r))

(defn readplan-mode [r] (:mode r))

(defn readplan-identity [r] (:identity r))

(defn readplan-subjects [r] (:subjects r))

(defn readplan-predicates [r] (:predicates r))

(defn readplan-follow-predicates [r] (:follow-predicates r))

(defn readplan-followed-predicates [r] (:followed-predicates r))

(defn readplan-limit [r] (:limit r))

(defrecord CanonicalActionReceipt [input-index changed occurrence])

(defn canonicalactionreceipt-input-index [r] (:input-index r))

(defn canonicalactionreceipt-changed [r] (:changed r))

(defn canonicalactionreceipt-occurrence [r] (:occurrence r))

(defrecord CanonicalCommit [store-space store-version actions])

(defn canonicalcommit-store-space [r] (:store-space r))

(defn canonicalcommit-store-version [r] (:store-version r))

(defn canonicalcommit-actions [r] (:actions r))

;; SemanticReceipt = TrackReceipt | PlanReceipt | StartReceipt | AssignReceipt | RequestReceipt | RequestAboutReceipt | AckReceipt | ResultReceipt | ResultAboutReceipt | OwnershipReceipt | SettlementReceipt
(defrecord TrackReceipt [protocol version action storeVersion referent])

(defn trackreceipt-protocol [r] (:protocol r))

(defn trackreceipt-version [r] (:version r))

(defn trackreceipt-action [r] (:action r))

(defn trackreceipt-storeVersion [r] (:storeVersion r))

(defn trackreceipt-referent [r] (:referent r))
(defrecord PlanReceipt [protocol version action storeVersion referent revision])

(defn planreceipt-protocol [r] (:protocol r))

(defn planreceipt-version [r] (:version r))

(defn planreceipt-action [r] (:action r))

(defn planreceipt-storeVersion [r] (:storeVersion r))

(defn planreceipt-referent [r] (:referent r))

(defn planreceipt-revision [r] (:revision r))
(defrecord StartReceipt [protocol version action storeVersion referent occurrence])

(defn startreceipt-protocol [r] (:protocol r))

(defn startreceipt-version [r] (:version r))

(defn startreceipt-action [r] (:action r))

(defn startreceipt-storeVersion [r] (:storeVersion r))

(defn startreceipt-referent [r] (:referent r))

(defn startreceipt-occurrence [r] (:occurrence r))
(defrecord AssignReceipt [protocol version action storeVersion referent assignment])

(defn assignreceipt-protocol [r] (:protocol r))

(defn assignreceipt-version [r] (:version r))

(defn assignreceipt-action [r] (:action r))

(defn assignreceipt-storeVersion [r] (:storeVersion r))

(defn assignreceipt-referent [r] (:referent r))

(defn assignreceipt-assignment [r] (:assignment r))
(defrecord RequestReceipt [protocol version action storeVersion request])

(defn requestreceipt-protocol [r] (:protocol r))

(defn requestreceipt-version [r] (:version r))

(defn requestreceipt-action [r] (:action r))

(defn requestreceipt-storeVersion [r] (:storeVersion r))

(defn requestreceipt-request [r] (:request r))
(defrecord RequestAboutReceipt [protocol version action storeVersion request referent])

(defn requestaboutreceipt-protocol [r] (:protocol r))

(defn requestaboutreceipt-version [r] (:version r))

(defn requestaboutreceipt-action [r] (:action r))

(defn requestaboutreceipt-storeVersion [r] (:storeVersion r))

(defn requestaboutreceipt-request [r] (:request r))

(defn requestaboutreceipt-referent [r] (:referent r))
(defrecord AckReceipt [protocol version action storeVersion request ack])

(defn ackreceipt-protocol [r] (:protocol r))

(defn ackreceipt-version [r] (:version r))

(defn ackreceipt-action [r] (:action r))

(defn ackreceipt-storeVersion [r] (:storeVersion r))

(defn ackreceipt-request [r] (:request r))

(defn ackreceipt-ack [r] (:ack r))
(defrecord ResultReceipt [protocol version action storeVersion request result outcome])

(defn resultreceipt-protocol [r] (:protocol r))

(defn resultreceipt-version [r] (:version r))

(defn resultreceipt-action [r] (:action r))

(defn resultreceipt-storeVersion [r] (:storeVersion r))

(defn resultreceipt-request [r] (:request r))

(defn resultreceipt-result [r] (:result r))

(defn resultreceipt-outcome [r] (:outcome r))
(defrecord ResultAboutReceipt [protocol version action storeVersion request result outcome referent])

(defn resultaboutreceipt-protocol [r] (:protocol r))

(defn resultaboutreceipt-version [r] (:version r))

(defn resultaboutreceipt-action [r] (:action r))

(defn resultaboutreceipt-storeVersion [r] (:storeVersion r))

(defn resultaboutreceipt-request [r] (:request r))

(defn resultaboutreceipt-result [r] (:result r))

(defn resultaboutreceipt-outcome [r] (:outcome r))

(defn resultaboutreceipt-referent [r] (:referent r))
(defrecord OwnershipReceipt [protocol version action storeVersion transition owner])

(defn ownershipreceipt-protocol [r] (:protocol r))

(defn ownershipreceipt-version [r] (:version r))

(defn ownershipreceipt-action [r] (:action r))

(defn ownershipreceipt-storeVersion [r] (:storeVersion r))

(defn ownershipreceipt-transition [r] (:transition r))

(defn ownershipreceipt-owner [r] (:owner r))
(defrecord SettlementReceipt [protocol version action storeVersion assignment acceptedTransition settlement outcome])

(defn settlementreceipt-protocol [r] (:protocol r))

(defn settlementreceipt-version [r] (:version r))

(defn settlementreceipt-action [r] (:action r))

(defn settlementreceipt-storeVersion [r] (:storeVersion r))

(defn settlementreceipt-assignment [r] (:assignment r))

(defn settlementreceipt-acceptedTransition [r] (:acceptedTransition r))

(defn settlementreceipt-settlement [r] (:settlement r))

(defn settlementreceipt-outcome [r] (:outcome r))

(defrecord ViewFact [subject predicate value])

(defn viewfact-subject [r] (:subject r))

(defn viewfact-predicate [r] (:predicate r))

(defn viewfact-value [r] (:value r))

(defrecord SemanticView [protocol version referent facts derived])

(defn semanticview-protocol [r] (:protocol r))

(defn semanticview-version [r] (:version r))

(defn semanticview-referent [r] (:referent r))

(defn semanticview-facts [r] (:facts r))

(defn semanticview-derived [r] (:derived r))

(defrecord SemanticHistory [protocol version referent occurrences])

(defn semantichistory-protocol [r] (:protocol r))

(defn semantichistory-version [r] (:version r))

(defn semantichistory-referent [r] (:referent r))

(defn semantichistory-occurrences [r] (:occurrences r))

(defrecord SemanticInbox [protocol version actor requests])

(defn semanticinbox-protocol [r] (:protocol r))

(defn semanticinbox-version [r] (:version r))

(defn semanticinbox-actor [r] (:actor r))

(defn semanticinbox-requests [r] (:requests r))

(defn- fail [^String message code data]
  (throw (ex-info message (assoc data :type code))))

(defn- ^String require-text! [^String label value]
  (if (and (string? value) (not (str/blank? value)) (not (str/includes? value "\u0000"))) value (fail (str label " must be a nonblank, non-NUL string") :north.work-occurrences/invalid-text {:field label :value value})))

(defn- require-version! [^String label value]
  (if (and (integer? value) (not (neg? value))) value (fail (str label " must be a non-negative integer") :north.work-occurrences/invalid-store-version {:field label :value value})))

(defn ^Actor actor! [kind id]
  (let [^String checked-kind (require-text! "actor kind" kind)
   ^String checked-id (require-text! "actor id" id)]
  (if (contains? actor-kinds checked-kind) (->Actor checked-kind checked-id) (fail "actor kind is outside work-ownership-v1" :north.work-occurrences/invalid-actor-kind {:kind checked-kind}))))

(declare pending-offer! ownership-state! validate-ownership-transition!)

(def actor-json-fields #{"kind" "id"})

(def pending-offer-json-fields #{"id" "from" "to"})

(def ownership-state-json-fields #{"goal" "owner" "accountableParent" "pendingOffer"})

(def ownership-transition-json-fields #{"version" "before" "event" "after"})

(defn- ^Boolean exact-string-fields? [value expected]
  (and (map? value) (= expected (set (keys value)))))

(defn- malformed-ownership-json! [^String message value]
  (fail message :north.work-occurrences/invalid-ownership-json {:value value}))

(defn ^Actor decode-actor! [value]
  (if (exact-string-fields? value actor-json-fields) (actor! (get value "kind") (get value "id")) (malformed-ownership-json! "work ownership actor must contain exactly kind and id" value)))

(defn- decode-nullable-actor! [^String label value]
  (if (nil? value) nil (try
  (decode-actor! value)
  (catch Throwable error
    (malformed-ownership-json! (str label " is not a work ownership actor") value)))))

(defn- ^PendingOffer decode-pending-offer-json! [value]
  (if (exact-string-fields? value pending-offer-json-fields) (pending-offer! (get value "id") (decode-actor! (get value "from")) (decode-actor! (get value "to"))) (malformed-ownership-json! "pending offer has a malformed field set" value)))

(defn- decode-nullable-pending-offer! [value]
  (if (nil? value) nil (decode-pending-offer-json! value)))

(defn- ^OwnershipState decode-ownership-state-json! [value]
  (if (exact-string-fields? value ownership-state-json-fields) (ownership-state! (get value "goal") (decode-actor! (get value "owner")) (decode-nullable-actor! "accountableParent" (get value "accountableParent")) (decode-nullable-pending-offer! (get value "pendingOffer"))) (malformed-ownership-json! "ownership state has a malformed field set" value)))

(defn- decode-ownership-event-json! [value]
  (if (map? value) (let [kind (get value "kind")]
  (cond
  (= kind "offer") (if (exact-string-fields? value #{"kind" "actor" "offerId" "to"}) (->Offer (decode-actor! (get value "actor")) (require-text! "offerId" (get value "offerId")) (decode-actor! (get value "to"))) (malformed-ownership-json! "offer event has a malformed field set" value))
  (= kind "accept") (if (exact-string-fields? value #{"kind" "actor" "offerId"}) (->Accept (decode-actor! (get value "actor")) (require-text! "offerId" (get value "offerId"))) (malformed-ownership-json! "accept event has a malformed field set" value))
  (= kind "transfer") (let [base #{"kind" "actor" "to"}
   expected (if (contains? value "acknowledgedBy") (conj base "acknowledgedBy") base)]
  (if (exact-string-fields? value expected) (->Transfer (decode-actor! (get value "actor")) (decode-actor! (get value "to")) (if (contains? value "acknowledgedBy") (decode-actor! (get value "acknowledgedBy")) nil)) (malformed-ownership-json! "transfer event has a malformed field set" value)))
  (= kind "refuse") (if (exact-string-fields? value #{"kind" "actor" "offerId" "reason"}) (->Refuse (decode-actor! (get value "actor")) (require-text! "offerId" (get value "offerId")) (require-text! "refusal reason" (get value "reason"))) (malformed-ownership-json! "refuse event has a malformed field set" value))
  (= kind "escalate") (if (exact-string-fields? value #{"kind" "actor" "to" "reason"}) (->Escalate (decode-actor! (get value "actor")) (decode-actor! (get value "to")) (require-text! "escalation reason" (get value "reason"))) (malformed-ownership-json! "escalate event has a malformed field set" value))
  :else (malformed-ownership-json! "ownership event kind is unsupported" value))) (malformed-ownership-json! "ownership event must be an object" value)))

(defn ^OwnershipTransition decode-ownership-transition! [value]
  (if (exact-string-fields? value ownership-transition-json-fields) (validate-ownership-transition! (->OwnershipTransition (require-text! "work ownership version" (get value "version")) (decode-ownership-state-json! (get value "before")) (decode-ownership-event-json! (get value "event")) (decode-ownership-state-json! (get value "after")))) (malformed-ownership-json! "work ownership transition has a malformed field set" value)))

(defn- ^Actor validate-actor! [^Actor value]
  (let [^Actor checked (actor! (:kind value) (:id value))]
  (if (= checked value) value (fail "actor is malformed" :north.work-occurrences/invalid-actor {:actor value}))))

(defn- ^Boolean actor-equal? [left right]
  (cond
  (and (nil? left) (nil? right)) true
  (or (nil? left) (nil? right)) false
  :else (and (= (:kind left) (:kind right)) (= (:id left) (:id right)))))

(defn ^PendingOffer pending-offer! [id ^Actor from ^Actor to]
  (let [^Actor checked-from (validate-actor! from)
   ^Actor checked-to (validate-actor! to)]
  (if (actor-equal? checked-from checked-to) (fail "offer source and recipient must differ" :north.work-occurrences/invalid-offer {:from checked-from :to checked-to}) (->PendingOffer (require-text! "offer id" id) checked-from checked-to))))

(defn- ^PendingOffer validate-pending-offer! [^PendingOffer value]
  (let [^PendingOffer checked (pending-offer! (:id value) (:from value) (:to value))]
  (if (= checked value) value (fail "pending offer is malformed" :north.work-occurrences/invalid-offer {:offer value}))))

(defn ^OwnershipState ownership-state! [goal ^Actor owner accountable-parent pending-offer]
  (let [^Actor checked-owner (validate-actor! owner)
   checked-parent (let [bind__0 accountable-parent]
  (if bind__0 (let [^Actor parent bind__0]
  (validate-actor! parent)) nil))
   checked-offer (let [bind__1 pending-offer]
  (if bind__1 (let [^PendingOffer offer bind__1]
  (validate-pending-offer! offer)) nil))]
  (->OwnershipState (require-text! "ownership goal" goal) checked-owner checked-parent checked-offer)))

(defn- ^OwnershipState validate-state! [^OwnershipState value]
  (let [^OwnershipState checked (ownership-state! (:goal value) (:owner value) (:accountable-parent value) (:pending-offer value))]
  (if (= checked value) value (fail "ownership state is malformed" :north.work-occurrences/invalid-ownership-state {:state value}))))

(defn- ^Boolean pending-offer-equal? [left right]
  (cond
  (and (nil? left) (nil? right)) true
  (or (nil? left) (nil? right)) false
  :else (and (= (:id left) (:id right)) (and (actor-equal? (:from left) (:from right)) (actor-equal? (:to left) (:to right))))))

(defn- ^Boolean state-equal? [^OwnershipState left ^OwnershipState right]
  (and (= (:goal left) (:goal right)) (and (actor-equal? (:owner left) (:owner right)) (and (actor-equal? (:accountable-parent left) (:accountable-parent right)) (pending-offer-equal? (:pending-offer left) (:pending-offer right))))))

(defn- reject-transition! [^String message ^OwnershipTransition transition]
  (fail message :north.work-occurrences/invalid-ownership-transition {:transition transition}))

(defn ^OwnershipTransition validate-ownership-transition! [^OwnershipTransition transition]
  (let [^OwnershipState before (validate-state! (:before transition))
   ^OwnershipState after (validate-state! (:after transition))
   event (:event transition)]
  (if (not (= work-ownership-version (:version transition))) (do
  (reject-transition! "work ownership transition version must be work-ownership-v1" transition)))
  (if (not (= (:goal before) (:goal after))) (do
  (reject-transition! "work ownership events must not change the goal" transition)))
  (let [match__0 event]
  (cond
    (instance? Offer match__0) (let [actor (:actor match__0) offer-id (:offer-id match__0) to (:to match__0)] (let [^Actor checked-actor (validate-actor! actor)
   ^Actor checked-to (validate-actor! to)
   ^String checked-offer-id (require-text! "offer id" offer-id)
   pending (:pending-offer after)]
  (cond
  (some? (:pending-offer before)) (reject-transition! "offer requires no existing pending offer" transition)
  (not (actor-equal? checked-actor (:owner before))) (reject-transition! "only the current owner may offer work" transition)
  (actor-equal? checked-actor checked-to) (reject-transition! "offer recipient must differ from the current owner" transition)
  (not (actor-equal? (:owner after) (:owner before))) (reject-transition! "an offer must not move ownership" transition)
  (not (actor-equal? (:accountable-parent after) (:accountable-parent before))) (reject-transition! "an offer must not change accountable parent" transition)
  (nil? pending) (reject-transition! "an offer must create a pending offer" transition)
  (not (= (:id pending) checked-offer-id)) (reject-transition! "pending offer ID must match the offer event" transition)
  (not (actor-equal? (:from pending) checked-actor)) (reject-transition! "pending offer source must be the current owner" transition)
  (not (actor-equal? (:to pending) checked-to)) (reject-transition! "pending offer recipient must match the offer event" transition))))
    (instance? Accept match__0) (let [actor (:actor match__0) offer-id (:offer-id match__0)] (let [^Actor checked-actor (validate-actor! actor)
   ^String checked-offer-id (require-text! "offer id" offer-id)
   pending (:pending-offer before)]
  (cond
  (nil? pending) (reject-transition! "acceptance requires a pending offer" transition)
  (not (= (:id pending) checked-offer-id)) (reject-transition! "acceptance must name the pending offer" transition)
  (not (actor-equal? (:from pending) (:owner before))) (reject-transition! "pending offer source must still own the work" transition)
  (not (actor-equal? checked-actor (:to pending))) (reject-transition! "only the offered recipient may accept work" transition)
  (not (actor-equal? (:owner after) checked-actor)) (reject-transition! "acceptance must move ownership to the accepting run" transition)
  (not (actor-equal? (:accountable-parent after) (:owner before))) (reject-transition! "acceptance must retain the previous owner as accountable parent" transition)
  (some? (:pending-offer after)) (reject-transition! "acceptance must clear the pending offer" transition))))
    (instance? Transfer match__0) (let [actor (:actor match__0) to (:to match__0) acknowledged-by (:acknowledged-by match__0)] (let [^Actor checked-actor (validate-actor! actor)
   ^Actor checked-to (validate-actor! to)
   checked-ack (let [bind__2 acknowledged-by]
  (if bind__2 (let [^Actor ack bind__2]
  (validate-actor! ack)) nil))]
  (cond
  (some? (:pending-offer before)) (reject-transition! "direct transfer requires no pending offer" transition)
  (not (actor-equal? checked-actor (:owner before))) (reject-transition! "only the current owner may transfer work" transition)
  (actor-equal? checked-actor checked-to) (reject-transition! "transfer recipient must differ from the current owner" transition)
  (nil? checked-ack) (if (not (state-equal? before after)) (do
  (reject-transition! "unacknowledged transfer must not move ownership or accountability" transition)))
  (not (actor-equal? checked-ack checked-to)) (reject-transition! "transfer acknowledgement must come from the recipient" transition)
  (not (actor-equal? (:owner after) checked-to)) (reject-transition! "acknowledged transfer must move ownership to the recipient" transition)
  (not (actor-equal? (:accountable-parent after) (:accountable-parent before))) (reject-transition! "acknowledged transfer must preserve the existing accountable parent" transition)
  (some? (:pending-offer after)) (reject-transition! "acknowledged transfer must not create a pending offer" transition))))
    (instance? Refuse match__0) (let [actor (:actor match__0) offer-id (:offer-id match__0) reason (:reason match__0)] (let [^Actor checked-actor (validate-actor! actor)
   ^String checked-offer-id (require-text! "offer id" offer-id)
   ^String _reason (require-text! "refusal reason" reason)
   pending (:pending-offer before)]
  (cond
  (nil? pending) (reject-transition! "refusal requires a pending offer" transition)
  (not (= (:id pending) checked-offer-id)) (reject-transition! "refusal must name the pending offer" transition)
  (not (actor-equal? checked-actor (:to pending))) (reject-transition! "only the offered recipient may refuse work" transition)
  (not (actor-equal? (:owner after) (:owner before))) (reject-transition! "refusal must leave owner and accountable parent unchanged" transition)
  (not (actor-equal? (:accountable-parent after) (:accountable-parent before))) (reject-transition! "refusal must leave owner and accountable parent unchanged" transition)
  (some? (:pending-offer after)) (reject-transition! "refusal must clear only the pending offer" transition))))
    (instance? Escalate match__0) (let [actor (:actor match__0) to (:to match__0) reason (:reason match__0)] (let [^Actor checked-actor (validate-actor! actor)
   ^Actor checked-to (validate-actor! to)
   ^String _reason (require-text! "escalation reason" reason)]
  (cond
  (not (actor-equal? checked-actor (:owner before))) (reject-transition! "only the current owner may escalate work" transition)
  (nil? (:accountable-parent before)) (reject-transition! "escalation requires an accountable parent" transition)
  (not (actor-equal? checked-to (:accountable-parent before))) (reject-transition! "escalation must return to the accountable parent" transition)
  (not (state-equal? before after)) (reject-transition! "escalation must not change owner, goal, accountability, or pending offer" transition))))))
  transition))

(defn- ^StoreAction store-action [^String subject ^String predicate ^String value]
  (->StoreAction :assert subject predicate value))

(defn- actor-actions [^String subject ^String prefix ^Actor actor]
  [(store-action subject (str prefix "_kind") (:kind actor)) (store-action subject prefix (:id actor))])

(defn- pending-offer-actions [^String subject ^String prefix ^PendingOffer offer]
  (vec (concat [(store-action subject (str prefix "_id") (:id offer))] (vec (concat (actor-actions subject (str prefix "_from") (:from offer)) (actor-actions subject (str prefix "_to") (:to offer)))))))

(defn- state-actions [^String subject ^String prefix ^OwnershipState state]
  (let [base (vec (concat [(store-action subject (str prefix "_goal") (:goal state))] (actor-actions subject (str prefix "_owner") (:owner state))))
   with-parent (let [bind__3 (:accountable-parent state)]
  (if bind__3 (let [^Actor parent bind__3]
  (vec (concat base (actor-actions subject (str prefix "_accountable_parent") parent)))) base))]
  (let [bind__4 (:pending-offer state)]
  (if bind__4 (let [^PendingOffer offer bind__4]
  (vec (concat with-parent (pending-offer-actions subject (str prefix "_pending_offer") offer)))) with-parent))))

(defn- ^String event-kind [event]
  (let [match__1 event]
  (cond
    (instance? Offer match__1) (let [_ (:actor match__1) _ (:offer-id match__1) _ (:to match__1)] "offer")
    (instance? Accept match__1) (let [_ (:actor match__1) _ (:offer-id match__1)] "accept")
    (instance? Transfer match__1) (let [_ (:actor match__1) _ (:to match__1) _ (:acknowledged-by match__1)] "transfer")
    (instance? Refuse match__1) (let [_ (:actor match__1) _ (:offer-id match__1) _ (:reason match__1)] "refuse")
    (instance? Escalate match__1) (let [_ (:actor match__1) _ (:to match__1) _ (:reason match__1)] "escalate"))))

(defn- event-actions [^String subject event]
  (let [base [(store-action subject "ownership_event_kind" (event-kind event))]]
  (let [match__2 event]
  (cond
    (instance? Offer match__2) (let [actor (:actor match__2) offer-id (:offer-id match__2) to (:to match__2)] (vec (concat base (vec (concat (actor-actions subject "ownership_event_actor" actor) (vec (concat [(store-action subject "ownership_event_offer_id" offer-id)] (actor-actions subject "ownership_event_to" to))))))))
    (instance? Accept match__2) (let [actor (:actor match__2) offer-id (:offer-id match__2)] (vec (concat base (vec (concat (actor-actions subject "ownership_event_actor" actor) [(store-action subject "ownership_event_offer_id" offer-id)])))))
    (instance? Transfer match__2) (let [actor (:actor match__2) to (:to match__2) acknowledged-by (:acknowledged-by match__2)] (let [required (vec (concat (actor-actions subject "ownership_event_actor" actor) (actor-actions subject "ownership_event_to" to)))]
  (vec (concat base (let [bind__5 acknowledged-by]
  (if bind__5 (let [^Actor ack bind__5]
  (vec (concat required (actor-actions subject "ownership_event_acknowledged_by" ack)))) required))))))
    (instance? Refuse match__2) (let [actor (:actor match__2) offer-id (:offer-id match__2) reason (:reason match__2)] (vec (concat base (vec (concat (actor-actions subject "ownership_event_actor" actor) [(store-action subject "ownership_event_offer_id" offer-id) (store-action subject "ownership_event_reason" reason)])))))
    (instance? Escalate match__2) (let [actor (:actor match__2) to (:to match__2) reason (:reason match__2)] (vec (concat base (vec (concat (actor-actions subject "ownership_event_actor" actor) (vec (concat (actor-actions subject "ownership_event_to" to) [(store-action subject "ownership_event_reason" reason)])))))))))))

(defn- occurrence-actions [^String occurrence ^String kind referent ^String actor ^String at]
  (let [base [(store-action occurrence "entity_kind" occurrence-entity-kind) (store-action occurrence "occurrence_kind" kind) (store-action occurrence "actor" actor) (store-action occurrence "at" at)]]
  (let [bind__6 referent]
  (if bind__6 (let [^String about bind__6]
  (do
  (if (= occurrence about) (do
  (fail "an occurrence cannot reuse its tracked thing identity" :north.work-occurrences/invalid-occurrence-identity {:occurrence occurrence :referent about})))
  (conj base (store-action occurrence "about" about)))) base))))

(defn- ^PersistencePlan plan! [action ^CanonicalSnapshot snapshot actions receipt-intent]
  (let [^String checked-action (require-text! "mutation action" action)
   ^String checked-space (require-text! "expected Store space" (:store-space snapshot))
   checked-version (require-version! "expected Store version" (:store-version snapshot))
   identities (set (mapv (fn [^StoreAction store-action] (do
  (if (not (= :assert (:op store-action))) (do
  (fail "semantic publication actions must be assert-only" :north.work-occurrences/invalid-store-action {:action store-action})))
  [(:subject store-action) (:predicate store-action) (:value store-action)])) actions))]
  (if (empty? actions) (do
  (fail "semantic mutation plan must contain at least one assertion" :north.work-occurrences/invalid-store-action {:action checked-action})))
  (if (not (= (count identities) (count actions))) (do
  (fail "semantic mutation plan contains duplicate Store assertions" :north.work-occurrences/duplicate-store-action {:action checked-action})))
  (->PersistencePlan checked-action checked-space checked-version actions receipt-intent)))

(defn- require-same-snapshot! [^String label ^String expected-space expected-version ^CanonicalSnapshot snapshot]
  (if (not (and (= expected-space (:store-space snapshot)) (= expected-version (:store-version snapshot)))) (do
  (fail (str label " must come from the exact canonical Store snapshot") :north.work-occurrences/read-commit-race {:expected-space (:store-space snapshot) :actual-space expected-space :expected-version (:store-version snapshot) :actual-version expected-version}))))

(defn- ^String require-unused-occurrence-id! [^CanonicalSnapshot snapshot identity]
  (let [^String checked (require-text! "occurrence identity" identity)
   existing (filterv (fn [fact] (= checked (t/triple-t1 fact))) (:facts snapshot))]
  (if (not (empty? existing)) (do
  (fail "occurrence identity already exists in the canonical Store snapshot" :north.work-occurrences/occurrence-identity-conflict {:identity checked})))
  checked))

(defn- facts-actions! [facts]
  (mapv (fn [fact] (let [subject (t/triple-t1 fact)
   predicate (t/triple-t2 fact)
   value (t/triple-t3 fact)]
  (store-action (require-text! "Store fact subject" subject) (require-text! "Store fact predicate" predicate) (require-text! "Store fact value" value)))) facts))

(defn ^PersistencePlan track-plan! [referent title tracked-by tracked-at ^CanonicalSnapshot snapshot]
  (let [^String checked-referent (require-text! "tracked thing" referent)
   ^String checked-title (require-text! "tracked thing title" title)
   ^String checked-actor (require-text! "tracking actor" tracked-by)
   ^String checked-at (require-text! "tracking instant" tracked-at)]
  (plan! "track" snapshot (facts-actions! (referents/tracked-thing-facts! checked-referent checked-title checked-actor checked-at)) (->TrackIntent checked-referent))))

(defn ^PersistencePlan plan-revision-plan! [referent revision intended-path endorsed-by endorsed-at ^CanonicalSnapshot snapshot]
  (let [^String checked-referent (require-text! "tracked thing" referent)
   ^String checked-revision (require-text! "Plan revision" revision)
   ^String checked-path (require-text! "intended path or change" intended-path)
   ^String checked-actor (require-text! "endorsing actor" endorsed-by)
   ^String checked-at (require-text! "endorsement instant" endorsed-at)]
  (plan! "plan" snapshot (facts-actions! (referents/plan-revision-facts! checked-referent checked-revision checked-path checked-actor checked-at)) (->PlanIntent checked-referent checked-revision))))

(defn ^PersistencePlan start-plan! [start ^PlanSnapshot plan exact-revision started-by signature started-at ^CanonicalSnapshot snapshot]
  (let [^String checked-start (require-text! "start occurrence" start)
   ^String checked-revision (require-text! "exact Plan revision" exact-revision)
   ^String checked-actor (require-text! "starting actor" started-by)
   ^String checked-signature (require-text! "start signature" signature)
   ^String checked-at (require-text! "start instant" started-at)]
  (require-same-snapshot! "Plan revision" (:store-space plan) (:store-version plan) snapshot)
  (if (not (= checked-revision (:revision plan))) (do
  (fail "start must name the exact Plan revision from its Store snapshot" :north.work-occurrences/plan-snapshot-mismatch {:referent (:referent plan) :expected (:revision plan) :actual checked-revision})))
  (plan! "start" snapshot (facts-actions! (referents/start-facts! checked-start (:referent plan) checked-revision checked-actor checked-signature checked-at)) (->StartIntent (:referent plan) checked-start))))

(defn ^PersistencePlan assignment-plan! [assignment referent assigned-by assignee assigned-at ^CanonicalSnapshot snapshot]
  (let [^String checked-assignment (require-text! "Assignment" assignment)
   ^String checked-referent (require-text! "tracked thing" referent)
   ^String checked-actor (require-text! "assigning actor" assigned-by)
   ^String checked-assignee (require-text! "assignee" assignee)
   ^String checked-at (require-text! "assignment instant" assigned-at)]
  (plan! "assign" snapshot (facts-actions! (referents/assignment-facts! checked-assignment checked-referent checked-actor checked-assignee checked-at)) (->AssignIntent checked-referent checked-assignment))))

(defn ^PersistencePlan request-plan! [request referent from to body occurred-at ^CanonicalSnapshot snapshot]
  (let [^String checked-request (require-unused-occurrence-id! snapshot request)
   checked-referent (if (nil? referent) nil (require-text! "Request tracked thing" referent))
   ^String checked-from (require-text! "Request source" from)
   ^String checked-to (require-text! "Request recipient" to)
   ^String checked-body (require-text! "Request body" body)
   ^String checked-at (require-text! "Request instant" occurred-at)]
  (if (= checked-from checked-to) (do
  (fail "Request source and recipient must differ" :north.work-occurrences/invalid-request {:from checked-from :to checked-to})))
  (plan! "request" snapshot (vec (concat (occurrence-actions checked-request request-kind checked-referent checked-from checked-at) [(store-action checked-request "to" checked-to) (store-action checked-request "body" checked-body)])) (->RequestIntent checked-request checked-referent))))

(defn- ^Actor ownership-event-actor [event]
  (let [match__3 event]
  (cond
    (instance? Offer match__3) (let [actor (:actor match__3) _ (:offer-id match__3) _ (:to match__3)] actor)
    (instance? Accept match__3) (let [actor (:actor match__3) _ (:offer-id match__3)] actor)
    (instance? Transfer match__3) (let [actor (:actor match__3) _ (:to match__3) _ (:acknowledged-by match__3)] actor)
    (instance? Refuse match__3) (let [actor (:actor match__3) _ (:offer-id match__3) _ (:reason match__3)] actor)
    (instance? Escalate match__3) (let [actor (:actor match__3) _ (:to match__3) _ (:reason match__3)] actor))))

(defn ^PersistencePlan ownership-transition-plan! [occurrence ^OwnershipTransition transition occurred-at ^CanonicalSnapshot snapshot]
  (let [^String checked-occurrence (require-text! "ownership transition occurrence" occurrence)
   ^OwnershipTransition checked (validate-ownership-transition! transition)
   ^String goal (:goal (:before checked))
   ^Actor actor (ownership-event-actor (:event checked))
   ^String checked-at (require-text! "ownership transition instant" occurred-at)]
  (plan! "ownership" snapshot (vec (concat (occurrence-actions checked-occurrence ownership-transition-kind goal (:id actor) checked-at) (vec (concat [(store-action checked-occurrence "ownership_contract" work-ownership-version)] (vec (concat (state-actions checked-occurrence "ownership_before" (:before checked)) (vec (concat (event-actions checked-occurrence (:event checked)) (state-actions checked-occurrence "ownership_after" (:after checked)))))))))) (->OwnershipIntent checked-occurrence (:id (:owner (:after checked)))))))

(defn ^CanonicalSnapshot canonical-snapshot! [store-space store-version values]
  (let [^String checked-space (require-text! "canonical snapshot Store space" store-space)
   checked-version (require-version! "canonical snapshot Store version" store-version)]
  (if (not (vector? values)) (do
  (fail "canonical snapshot facts must be a vector" :north.work-occurrences/invalid-snapshot {:facts values})))
  (let [facts (loop [remaining values
   accepted []]
  (if (empty? remaining) accepted (let [value (first remaining)]
  (if (not (t/triple? value)) (do
  (fail "canonical snapshot contains a malformed Store fact" :north.work-occurrences/invalid-snapshot {:fact value})))
  (let [fact value
   ^String subject (require-text! "snapshot fact subject" (t/triple-t1 fact))
   ^String predicate (require-text! "snapshot fact predicate" (t/triple-t2 fact))
   ^String fact-value (require-text! "snapshot fact value" (t/triple-t3 fact))]
  (recur (vec (rest remaining)) (conj accepted (t/triple subject predicate fact-value)))))))
   distinct-facts (set facts)]
  (if (not (= (count facts) (count distinct-facts))) (do
  (fail "canonical snapshot contains duplicate Store facts" :north.work-occurrences/invalid-snapshot {:store-version checked-version})))
  (->CanonicalSnapshot checked-space checked-version facts))))

(defn- facts-at [^CanonicalSnapshot snapshot ^String subject]
  (filterv (fn [fact] (= subject (t/triple-t1 fact))) (:facts snapshot)))

(defn- values-at [^CanonicalSnapshot snapshot ^String subject ^String predicate]
  (mapv (fn [fact] (t/triple-t3 fact)) (filterv (fn [fact] (and (= subject (t/triple-t1 fact)) (= predicate (t/triple-t2 fact)))) (:facts snapshot))))

(defn- ^String exact-value-at! [^CanonicalSnapshot snapshot ^String subject ^String predicate]
  (let [values (values-at snapshot subject predicate)]
  (if (= 1 (count values)) (first values) (fail "canonical snapshot predicate must have exactly one value" :north.work-occurrences/invalid-snapshot {:subject subject :predicate predicate :values values}))))

(defn- optional-value-at! [^CanonicalSnapshot snapshot ^String subject ^String predicate]
  (let [values (values-at snapshot subject predicate)]
  (cond
  (empty? values) nil
  (= 1 (count values)) (first values)
  :else (fail "canonical snapshot optional predicate has multiple values" :north.work-occurrences/invalid-snapshot {:subject subject :predicate predicate :values values}))))

(defn- action-key [^StoreAction action]
  [(:subject action) (:predicate action) (:value action)])

(defn- fact-key [fact]
  [(t/triple-t1 fact) (t/triple-t2 fact) (t/triple-t3 fact)])

(defn- exact-subject-actions! [^CanonicalSnapshot snapshot ^String subject expected code]
  (let [actual (facts-at snapshot subject)
   actual-keys (set (mapv fact-key actual))
   expected-keys (set (mapv action-key expected))]
  (if (not (and (= (count actual) (count expected)) (= actual-keys expected-keys))) (do
  (fail "canonical snapshot subject does not match its exact occurrence shape" code {:subject subject :actual actual-keys :expected expected-keys})))))

(defn ^PlanSnapshot decode-plan-snapshot! [^CanonicalSnapshot snapshot referent exact-revision]
  (let [^String checked-referent (require-text! "tracked thing" referent)
   ^String checked-revision (require-text! "exact Plan revision" exact-revision)
   ^String current-revision (exact-value-at! snapshot checked-referent "current_plan_revision")]
  (if (not (= checked-revision current-revision)) (do
  (fail "caller-supplied Plan revision is not current in the exact snapshot" :north.work-occurrences/plan-snapshot-mismatch {:referent checked-referent :expected checked-revision :current current-revision})))
  (let [^String intended-path (require-text! "intended path or change" (exact-value-at! snapshot checked-revision "body"))
   ^String endorsed-by (require-text! "endorsing actor" (exact-value-at! snapshot checked-revision "actor"))
   ^String endorsed-at (require-text! "endorsement instant" (exact-value-at! snapshot checked-revision "at"))
   idx (proj/index-triples (:facts snapshot))]
  (if (not (referents/plan? idx checked-referent)) (do
  (fail "canonical snapshot does not contain the exact complete Plan" :north.work-occurrences/invalid-plan-snapshot {:referent checked-referent :revision checked-revision})))
  (let [^PersistencePlan expected (plan-revision-plan! checked-referent checked-revision intended-path endorsed-by endorsed-at snapshot)
   revision-actions (filterv (fn [^StoreAction action] (= checked-revision (:subject action))) (:actions expected))]
  (exact-subject-actions! snapshot checked-revision revision-actions :north.work-occurrences/invalid-plan-snapshot))
  (->PlanSnapshot (:store-space snapshot) (:store-version snapshot) checked-referent checked-revision intended-path endorsed-by endorsed-at))))

(defn ^RequestSnapshot decode-request-snapshot! [^CanonicalSnapshot snapshot request]
  (let [^String checked-request (require-text! "Request" request)
   ^String kind (exact-value-at! snapshot checked-request "entity_kind")
   ^String occurrence-kind (exact-value-at! snapshot checked-request "occurrence_kind")
   ^String from (require-text! "Request source" (exact-value-at! snapshot checked-request "actor"))
   ^String at (require-text! "Request instant" (exact-value-at! snapshot checked-request "at"))
   referent (optional-value-at! snapshot checked-request "about")
   ^String to (require-text! "Request recipient" (exact-value-at! snapshot checked-request "to"))
   ^String body (require-text! "Request body" (exact-value-at! snapshot checked-request "body"))
   idx (proj/index-triples (:facts snapshot))]
  (if (not (and (= occurrence-entity-kind kind) (= request-kind occurrence-kind) (referents/complete-base-occurrence? idx checked-request request-kind ["to" "body"]))) (do
  (fail "canonical Request snapshot is not a complete Request occurrence" :north.work-occurrences/invalid-request-snapshot {:request checked-request})))
  (if (= from to) (do
  (fail "canonical Request source and recipient must differ" :north.work-occurrences/invalid-request-snapshot {:request checked-request :from from :to to})))
  (if (not (and (referents/agent? idx from) (referents/agent? idx to))) (do
  (fail "canonical Request actors must be tracked Agents" :north.work-occurrences/invalid-request-snapshot {:request checked-request :from from :to to})))
  (if (some? referent) (do
  (if (not (referents/tracked-thing? idx referent)) (do
  (fail "canonical Request about link must name a tracked thing" :north.work-occurrences/invalid-request-snapshot {:request checked-request :referent referent})))))
  (let [expected (vec (concat (occurrence-actions checked-request request-kind referent from at) [(store-action checked-request "to" to) (store-action checked-request "body" body)]))]
  (exact-subject-actions! snapshot checked-request expected :north.work-occurrences/invalid-request-snapshot))
  (->RequestSnapshot (:store-space snapshot) (:store-version snapshot) checked-request referent from to body at)))

(defn- ^PersistencePlan ack-plan-from-snapshot! [ack ^RequestSnapshot request actor occurred-at ^CanonicalSnapshot snapshot ^Boolean require-unused?]
  (let [^String checked-ack (require-text! "ACK" ack)
   ^String checked-actor (require-text! "ACK actor" actor)
   ^String checked-at (require-text! "ACK instant" occurred-at)]
  (if (= checked-ack (:request request)) (do
  (fail "ACK identity must differ from its Request" :north.work-occurrences/invalid-ack {:ack checked-ack :request (:request request)})))
  (require-same-snapshot! "Request" (:store-space request) (:store-version request) snapshot)
  (if require-unused? (do
  (require-unused-occurrence-id! snapshot checked-ack)))
  (if (not (= checked-actor (:to request))) (do
  (fail "only the exact Request recipient may ACK it" :north.work-occurrences/invalid-ack {:request (:request request) :expected (:to request) :actual checked-actor})))
  (plan! "ack" snapshot (conj (occurrence-actions checked-ack ack-kind (:referent request) checked-actor checked-at) (store-action checked-ack "acknowledges" (:request request))) (->AckIntent (:request request) checked-ack))))

(defn ^PersistencePlan ack-plan! [ack ^RequestSnapshot request actor occurred-at ^CanonicalSnapshot snapshot]
  (ack-plan-from-snapshot! ack request actor occurred-at snapshot true))

(defn ^AckSnapshot decode-ack-snapshot! [^CanonicalSnapshot snapshot ack]
  (let [^String checked-ack (require-text! "ACK" ack)
   ^String request-id (require-text! "acknowledged Request" (exact-value-at! snapshot checked-ack "acknowledges"))
   ^String actor (require-text! "ACK actor" (exact-value-at! snapshot checked-ack "actor"))
   ^String at (require-text! "ACK instant" (exact-value-at! snapshot checked-ack "at"))
   referent (optional-value-at! snapshot checked-ack "about")
   ^RequestSnapshot request (decode-request-snapshot! snapshot request-id)
   ^PersistencePlan expected (ack-plan-from-snapshot! checked-ack request actor at snapshot false)]
  (if (not (= referent (:referent request))) (do
  (fail "ACK about link must equal its exact Request about link" :north.work-occurrences/invalid-ack-snapshot {:ack checked-ack :request request-id :expected (:referent request) :actual referent})))
  (exact-subject-actions! snapshot checked-ack (:actions expected) :north.work-occurrences/invalid-ack-snapshot)
  (->AckSnapshot (:store-space snapshot) (:store-version snapshot) checked-ack request-id referent actor at)))

(defn- ^PersistencePlan result-plan-from-snapshot! [result ^RequestSnapshot request reporting-actor outcome summary occurred-at ^CanonicalSnapshot snapshot ^Boolean require-unused?]
  (let [^String checked-result (require-text! "Result" result)
   ^String checked-actor (require-text! "Result reporting actor" reporting-actor)
   ^String checked-outcome (require-text! "Result outcome" outcome)
   ^String checked-summary (require-text! "Result summary" summary)
   ^String checked-at (require-text! "Result instant" occurred-at)]
  (if (= checked-result (:request request)) (do
  (fail "Result identity must differ from its Request" :north.work-occurrences/invalid-result {:result checked-result :request (:request request)})))
  (require-same-snapshot! "Request" (:store-space request) (:store-version request) snapshot)
  (if require-unused? (do
  (require-unused-occurrence-id! snapshot checked-result)))
  (if (not (= checked-actor (:to request))) (do
  (fail "only the exact Request recipient may report its Result" :north.work-occurrences/invalid-result {:request (:request request) :expected (:to request) :actual checked-actor})))
  (plan! "result" snapshot (vec (concat (occurrence-actions checked-result result-kind (:referent request) checked-actor checked-at) [(store-action checked-result "request" (:request request)) (store-action checked-result "outcome" checked-outcome) (store-action checked-result "summary" checked-summary)])) (->ResultIntent (:request request) checked-result checked-outcome (:referent request)))))

(defn ^PersistencePlan result-plan! [result ^RequestSnapshot request reporting-actor outcome summary occurred-at ^CanonicalSnapshot snapshot]
  (result-plan-from-snapshot! result request reporting-actor outcome summary occurred-at snapshot true))

(defn ^ResultSnapshot decode-result-snapshot! [^CanonicalSnapshot snapshot result]
  (let [^String checked-result (require-text! "Result" result)
   ^String request-id (require-text! "Result Request" (exact-value-at! snapshot checked-result "request"))
   ^String actor (require-text! "Result reporting actor" (exact-value-at! snapshot checked-result "actor"))
   ^String outcome (require-text! "Result outcome" (exact-value-at! snapshot checked-result "outcome"))
   ^String summary (require-text! "Result summary" (exact-value-at! snapshot checked-result "summary"))
   ^String at (require-text! "Result instant" (exact-value-at! snapshot checked-result "at"))
   referent (optional-value-at! snapshot checked-result "about")
   ^RequestSnapshot request (decode-request-snapshot! snapshot request-id)
   ^PersistencePlan expected (result-plan-from-snapshot! checked-result request actor outcome summary at snapshot false)]
  (if (not (= referent (:referent request))) (do
  (fail "Result about link must equal its exact Request about link" :north.work-occurrences/invalid-result-snapshot {:result checked-result :request request-id :expected (:referent request) :actual referent})))
  (exact-subject-actions! snapshot checked-result (:actions expected) :north.work-occurrences/invalid-result-snapshot)
  (->ResultSnapshot (:store-space snapshot) (:store-version snapshot) checked-result request-id referent actor outcome summary at)))

(defn ^AssignmentSnapshot decode-assignment-snapshot! [^CanonicalSnapshot snapshot assignment]
  (let [^String checked-assignment (require-text! "Assignment" assignment)
   ^String referent (require-text! "Assignment tracked thing" (exact-value-at! snapshot checked-assignment "about"))
   ^String assigned-by (require-text! "assigning actor" (exact-value-at! snapshot checked-assignment "actor"))
   ^String assignee (require-text! "assignee" (exact-value-at! snapshot checked-assignment "assignee"))
   ^String at (require-text! "assignment instant" (exact-value-at! snapshot checked-assignment "at"))
   idx (proj/index-triples (:facts snapshot))]
  (if (not (and (referents/tracked-thing? idx referent) (referents/assignment-occurrence? idx checked-assignment referent))) (do
  (fail "canonical Assignment snapshot is not a complete Assignment" :north.work-occurrences/invalid-assignment-snapshot {:assignment checked-assignment :referent referent})))
  (exact-subject-actions! snapshot checked-assignment (facts-actions! (referents/assignment-facts! checked-assignment referent assigned-by assignee at)) :north.work-occurrences/invalid-assignment-snapshot)
  (->AssignmentSnapshot (:store-space snapshot) (:store-version snapshot) checked-assignment referent assigned-by assignee at)))

(defn- ^Actor decode-actor-facts! [^CanonicalSnapshot snapshot ^String subject ^String prefix]
  (actor! (exact-value-at! snapshot subject (str prefix "_kind")) (exact-value-at! snapshot subject prefix)))

(defn- decode-optional-actor-facts! [^CanonicalSnapshot snapshot ^String subject ^String prefix]
  (let [kind (optional-value-at! snapshot subject (str prefix "_kind"))
   id (optional-value-at! snapshot subject prefix)]
  (cond
  (and (nil? kind) (nil? id)) nil
  (or (nil? kind) (nil? id)) (fail "canonical ownership actor facts are incomplete" :north.work-occurrences/invalid-ownership-snapshot {:subject subject :prefix prefix})
  :else (actor! kind id))))

(defn- decode-pending-offer-facts! [^CanonicalSnapshot snapshot ^String subject ^String prefix]
  (let [id (optional-value-at! snapshot subject (str prefix "_id"))]
  (if (nil? id) (let [residue (vec (concat (values-at snapshot subject (str prefix "_from_kind")) (vec (concat (values-at snapshot subject (str prefix "_from")) (vec (concat (values-at snapshot subject (str prefix "_to_kind")) (values-at snapshot subject (str prefix "_to"))))))))]
  (if (empty? residue) nil (fail "canonical pending-offer facts are incomplete" :north.work-occurrences/invalid-ownership-snapshot {:subject subject :prefix prefix}))) (pending-offer! id (decode-actor-facts! snapshot subject (str prefix "_from")) (decode-actor-facts! snapshot subject (str prefix "_to"))))))

(defn- ^OwnershipState decode-state-facts! [^CanonicalSnapshot snapshot ^String subject ^String prefix]
  (ownership-state! (exact-value-at! snapshot subject (str prefix "_goal")) (decode-actor-facts! snapshot subject (str prefix "_owner")) (decode-optional-actor-facts! snapshot subject (str prefix "_accountable_parent")) (decode-pending-offer-facts! snapshot subject (str prefix "_pending_offer"))))

(defn- decode-event-facts! [^CanonicalSnapshot snapshot ^String subject]
  (let [^String kind (exact-value-at! snapshot subject "ownership_event_kind")
   ^Actor actor (decode-actor-facts! snapshot subject "ownership_event_actor")]
  (cond
  (= kind "offer") (->Offer actor (exact-value-at! snapshot subject "ownership_event_offer_id") (decode-actor-facts! snapshot subject "ownership_event_to"))
  (= kind "accept") (->Accept actor (exact-value-at! snapshot subject "ownership_event_offer_id"))
  (= kind "transfer") (->Transfer actor (decode-actor-facts! snapshot subject "ownership_event_to") (decode-optional-actor-facts! snapshot subject "ownership_event_acknowledged_by"))
  (= kind "refuse") (->Refuse actor (exact-value-at! snapshot subject "ownership_event_offer_id") (exact-value-at! snapshot subject "ownership_event_reason"))
  (= kind "escalate") (->Escalate actor (decode-actor-facts! snapshot subject "ownership_event_to") (exact-value-at! snapshot subject "ownership_event_reason"))
  :else (fail "canonical ownership event kind is unsupported" :north.work-occurrences/invalid-ownership-snapshot {:subject subject :kind kind}))))

(defn ^OwnershipOccurrence decode-ownership-occurrence! [^CanonicalSnapshot snapshot occurrence]
  (let [^String checked-occurrence (require-text! "ownership transition occurrence" occurrence)
   ^String entity-kind (exact-value-at! snapshot checked-occurrence "entity_kind")
   ^String kind (exact-value-at! snapshot checked-occurrence "occurrence_kind")
   ^String referent (exact-value-at! snapshot checked-occurrence "about")
   ^String actor-id (exact-value-at! snapshot checked-occurrence "actor")
   ^String at (exact-value-at! snapshot checked-occurrence "at")
   ^String contract (exact-value-at! snapshot checked-occurrence "ownership_contract")
   ^OwnershipTransition transition (validate-ownership-transition! (->OwnershipTransition contract (decode-state-facts! snapshot checked-occurrence "ownership_before") (decode-event-facts! snapshot checked-occurrence) (decode-state-facts! snapshot checked-occurrence "ownership_after")))]
  (if (not (and (= occurrence-entity-kind entity-kind) (= ownership-transition-kind kind) (= work-ownership-version contract) (= referent (:goal (:before transition))) (= actor-id (:id (ownership-event-actor (:event transition)))))) (do
  (fail "canonical ownership occurrence does not bind its transition" :north.work-occurrences/invalid-ownership-snapshot {:occurrence checked-occurrence})))
  (let [^PersistencePlan expected (ownership-transition-plan! checked-occurrence transition at snapshot)]
  (exact-subject-actions! snapshot checked-occurrence (:actions expected) :north.work-occurrences/invalid-ownership-snapshot))
  (->OwnershipOccurrence (:store-space snapshot) (:store-version snapshot) checked-occurrence at transition)))

(defn- ^Boolean accepted-transition? [^OwnershipTransition transition]
  (let [match__4 (:event transition)]
  (cond
    (instance? Accept match__4) (let [_ (:actor match__4) _ (:offer-id match__4)] true)
    (instance? Transfer match__4) (let [_ (:actor match__4) _ (:to match__4) acknowledged-by (:acknowledged-by match__4)] (some? acknowledged-by))
    (instance? Offer match__4) (let [_ (:actor match__4) _ (:offer-id match__4) _ (:to match__4)] false)
    (instance? Refuse match__4) (let [_ (:actor match__4) _ (:offer-id match__4) _ (:reason match__4)] false)
    (instance? Escalate match__4) (let [_ (:actor match__4) _ (:to match__4) _ (:reason match__4)] false))))

(defn- ^PersistencePlan settlement-plan-from-snapshot! [settlement ^AssignmentSnapshot assignment ^OwnershipOccurrence accepted reporting-actor outcome summary occurred-at ^CanonicalSnapshot snapshot ^Boolean require-unused?]
  (let [^String checked-settlement (require-text! "Settlement" settlement)
   ^String checked-actor (require-text! "Settlement reporting actor" reporting-actor)
   ^String checked-outcome (require-text! "Settlement outcome" outcome)
   ^String checked-summary (require-text! "Settlement summary" summary)
   ^String checked-at (require-text! "Settlement instant" occurred-at)
   ^OwnershipTransition transition (:transition accepted)
   ^Actor owner (:owner (:after transition))
   parent (:accountable-parent (:after transition))]
  (require-same-snapshot! "Assignment" (:store-space assignment) (:store-version assignment) snapshot)
  (require-same-snapshot! "ownership transition" (:store-space accepted) (:store-version accepted) snapshot)
  (if (not (accepted-transition? transition)) (do
  (fail "Settlement requires an accepted ownership transition" :north.work-occurrences/invalid-settlement {:transition (:occurrence accepted)})))
  (if (not (= (:referent assignment) (:goal (:after transition)))) (do
  (fail "Settlement Assignment and ownership transition must name one tracked thing" :north.work-occurrences/invalid-settlement {:assignment (:assignment assignment) :transition (:occurrence accepted)})))
  (if (not (and (= checked-actor (:assignee assignment)) (= checked-actor (:id owner)))) (do
  (fail "Settlement reporting actor must be the assignee and current owner" :north.work-occurrences/invalid-settlement {:actor checked-actor :assignee (:assignee assignment) :owner (:id owner)})))
  (if (or (= checked-settlement (:assignment assignment)) (= checked-settlement (:occurrence accepted))) (do
  (fail "Settlement must have an identity distinct from its inputs" :north.work-occurrences/invalid-settlement {:settlement checked-settlement :assignment (:assignment assignment) :transition (:occurrence accepted)})))
  (if require-unused? (do
  (require-unused-occurrence-id! snapshot checked-settlement)))
  (let [bind__7 parent]
  (if bind__7 (let [^Actor accountable-parent bind__7]
  (plan! "settle" snapshot (vec (concat (occurrence-actions checked-settlement settlement-kind (:referent assignment) checked-actor checked-at) [(store-action checked-settlement "assignment" (:assignment assignment)) (store-action checked-settlement "accepted_transition" (:occurrence accepted)) (store-action checked-settlement "reporting_actor_kind" (:kind owner)) (store-action checked-settlement "accountable_parent" (:id accountable-parent)) (store-action checked-settlement "accountable_parent_kind" (:kind accountable-parent)) (store-action checked-settlement "outcome" checked-outcome) (store-action checked-settlement "summary" checked-summary)])) (->SettleIntent (:assignment assignment) (:occurrence accepted) checked-settlement checked-outcome))) (fail "Settlement requires the immediate accountable parent" :north.work-occurrences/invalid-settlement {:transition (:occurrence accepted)})))))

(defn ^PersistencePlan settlement-plan! [settlement ^AssignmentSnapshot assignment ^OwnershipOccurrence accepted reporting-actor outcome summary occurred-at ^CanonicalSnapshot snapshot]
  (settlement-plan-from-snapshot! settlement assignment accepted reporting-actor outcome summary occurred-at snapshot true))

(defn ^SettlementSnapshot decode-settlement-snapshot! [^CanonicalSnapshot snapshot settlement]
  (let [^String checked-settlement (require-text! "Settlement" settlement)
   ^String referent (require-text! "Settlement tracked thing" (exact-value-at! snapshot checked-settlement "about"))
   ^String assignment-id (require-text! "Settlement Assignment" (exact-value-at! snapshot checked-settlement "assignment"))
   ^String transition-id (require-text! "Settlement accepted transition" (exact-value-at! snapshot checked-settlement "accepted_transition"))
   ^Actor reporting-actor (actor! (exact-value-at! snapshot checked-settlement "reporting_actor_kind") (exact-value-at! snapshot checked-settlement "actor"))
   ^Actor accountable-parent (actor! (exact-value-at! snapshot checked-settlement "accountable_parent_kind") (exact-value-at! snapshot checked-settlement "accountable_parent"))
   ^String outcome (require-text! "Settlement outcome" (exact-value-at! snapshot checked-settlement "outcome"))
   ^String summary (require-text! "Settlement summary" (exact-value-at! snapshot checked-settlement "summary"))
   ^String at (require-text! "Settlement instant" (exact-value-at! snapshot checked-settlement "at"))
   ^AssignmentSnapshot assignment (decode-assignment-snapshot! snapshot assignment-id)
   ^OwnershipOccurrence accepted (decode-ownership-occurrence! snapshot transition-id)
   ^PersistencePlan expected (settlement-plan-from-snapshot! checked-settlement assignment accepted (:id reporting-actor) outcome summary at snapshot false)]
  (if (not (and (= referent (:referent assignment)) (actor-equal? accountable-parent (:accountable-parent (:after (:transition accepted)))))) (do
  (fail "Settlement snapshot does not bind its tracked thing and accountable parent" :north.work-occurrences/invalid-settlement-snapshot {:settlement checked-settlement :referent referent :accountable-parent accountable-parent})))
  (exact-subject-actions! snapshot checked-settlement (:actions expected) :north.work-occurrences/invalid-settlement-snapshot)
  (->SettlementSnapshot (:store-space snapshot) (:store-version snapshot) checked-settlement referent assignment-id transition-id reporting-actor accountable-parent outcome summary at)))

(defn- ^ReadPlan read-plan! [^String action mode identity subjects predicates follow-predicates followed-predicates limit]
  (let [^String checked-identity (require-text! "read identity" identity)]
  (if (not (contains? #{:subjects :about :inbox} mode)) (do
  (fail "semantic read plan mode is unsupported" :north.work-occurrences/invalid-read-plan {:mode mode})))
  (if (not (pos? limit)) (do
  (fail "semantic read plan limit must be positive" :north.work-occurrences/invalid-read-plan {:limit limit})))
  (->ReadPlan action mode checked-identity subjects predicates follow-predicates followed-predicates limit)))

(def ownership-predicates ["entity_kind" "occurrence_kind" "about" "actor" "at" "ownership_contract" "ownership_before_goal" "ownership_before_owner_kind" "ownership_before_owner" "ownership_before_accountable_parent_kind" "ownership_before_accountable_parent" "ownership_before_pending_offer_id" "ownership_before_pending_offer_from_kind" "ownership_before_pending_offer_from" "ownership_before_pending_offer_to_kind" "ownership_before_pending_offer_to" "ownership_event_kind" "ownership_event_actor_kind" "ownership_event_actor" "ownership_event_offer_id" "ownership_event_to_kind" "ownership_event_to" "ownership_event_acknowledged_by_kind" "ownership_event_acknowledged_by" "ownership_event_reason" "ownership_after_goal" "ownership_after_owner_kind" "ownership_after_owner" "ownership_after_accountable_parent_kind" "ownership_after_accountable_parent" "ownership_after_pending_offer_id" "ownership_after_pending_offer_from_kind" "ownership_after_pending_offer_from" "ownership_after_pending_offer_to_kind" "ownership_after_pending_offer_to"])

(def request-predicates ["entity_kind" "occurrence_kind" "about" "actor" "at" "to" "body"])

(defn ^ReadPlan request-read-plan! [request]
  (let [^String checked (require-text! "Request" request)]
  (read-plan! "request" :subjects checked [checked] ["entity_kind"] [] [] 2)))

(defn ^ReadPlan ack-read-plan! [request ack]
  (let [^String checked-request (require-text! "Request" request)
   ^String checked-ack (require-text! "ACK" ack)]
  (read-plan! "ack" :subjects checked-request [checked-request checked-ack] request-predicates ["actor" "to" "about"] ["entity_kind" "referent_role"] 32)))

(defn ^ReadPlan result-read-plan! [request result]
  (let [^String checked-request (require-text! "Request" request)
   ^String checked-result (require-text! "Result" result)]
  (read-plan! "result" :subjects checked-request [checked-request checked-result] request-predicates ["actor" "to" "about"] ["entity_kind" "referent_role"] 32)))

(defn ^ReadPlan settle-read-plan! [assignment accepted-transition settlement]
  (let [^String checked-assignment (require-text! "Assignment" assignment)
   ^String checked-transition (require-text! "accepted ownership transition" accepted-transition)
   ^String checked-settlement (require-text! "Settlement" settlement)]
  (read-plan! "settle" :subjects checked-assignment [checked-assignment checked-transition checked-settlement] (vec (concat ["entity_kind" "occurrence_kind" "about" "actor" "at" "assignee"] ownership-predicates)) ["actor" "assignee" "about"] ["entity_kind" "referent_role"] 80)))

(defn ^ReadPlan show-read-plan! [referent]
  (let [^String checked (require-text! "tracked thing" referent)]
  (read-plan! "show" :subjects checked [checked] ["entity_kind" "title" "tracked_by" "tracked_at" "referent_role" "desired_outcome" "current_plan_revision"] ["current_plan_revision"] ["entity_kind" "occurrence_kind" "about" "actor" "at" "body"] 64)))

(defn ^ReadPlan start-read-plan! [referent revision]
  (let [^String checked-referent (require-text! "tracked thing" referent)
   ^String checked-revision (require-text! "exact Plan revision" revision)]
  (read-plan! "start" :subjects checked-referent [checked-referent checked-revision] ["entity_kind" "referent_role" "current_plan_revision" "occurrence_kind" "about" "actor" "at" "body"] ["tracked_by" "actor"] ["entity_kind" "referent_role"] 32)))

(defn ^ReadPlan history-read-plan! [referent]
  (read-plan! "history" :about referent [] (vec (concat ["entity_kind" "occurrence_kind" "about" "actor" "at" "to" "body" "acknowledges" "request" "assignee" "plan_revision" "signature" "assignment" "accepted_transition" "reporting_actor_kind" "accountable_parent" "accountable_parent_kind" "outcome" "summary"] ownership-predicates)) [] [] 256))

(defn ^ReadPlan inbox-read-plan! [actor]
  (read-plan! "inbox" :inbox actor [] request-predicates ["actor" "to" "about"] ["entity_kind" "referent_role"] 256))

(defn ^String plan-action [^PersistencePlan plan]
  (:action plan))

(defn publication-actions [^PersistencePlan plan]
  (:actions plan))

(defn publication-options [^PersistencePlan plan]
  {:expected-version (:expected-store-version plan)})

(def canonical-commit-fields #{:ok :changed? :results})

(def canonical-action-fields #{:input-index :changed? :occurrence})

(defn- ^Boolean exact-fields? [value expected]
  (and (map? value) (= expected (set (keys value)))))

(defn- coordinate-version! [coordinate]
  (let [transaction (t/triple-t1 coordinate)]
  (if (t/transaction-coordinate? transaction) (let [value (t/triple-t3 transaction)]
  (if (integer? value) value (fail "Store occurrence transaction version is malformed" :north.work-occurrences/invalid-store-commit {:occurrence coordinate}))) (fail "Store action result has no transaction coordinate" :north.work-occurrences/invalid-store-commit {:occurrence coordinate}))))

(defn- ^String coordinate-space! [coordinate]
  (let [transaction (t/triple-t1 coordinate)]
  (if (t/transaction-coordinate? transaction) (let [value (t/triple-t1 transaction)]
  (if (string? value) value (fail "Store occurrence space is malformed" :north.work-occurrences/invalid-store-commit {:occurrence coordinate}))) (fail "Store action result has no transaction coordinate" :north.work-occurrences/invalid-store-commit {:occurrence coordinate}))))

(defn- ^CanonicalActionReceipt decode-action-receipt! [value expected-index expected-version expected-space]
  (if (not (exact-fields? value canonical-action-fields)) (do
  (fail "Store action result has a malformed field set" :north.work-occurrences/invalid-store-commit {:result value})))
  (let [input-index (:input-index value)
   changed (:changed? value)
   occurrence (:occurrence value)]
  (if (not (and (integer? input-index) (= expected-index input-index))) (do
  (fail "Store action results do not match plan order" :north.work-occurrences/store-commit-mismatch {:expected expected-index :actual input-index})))
  (if (not (true? changed)) (do
  (fail "immutable occurrence assertion did not change canonical Store history" :north.work-occurrences/store-commit-mismatch {:input-index expected-index :changed changed})))
  (if (not (t/occurrence-coordinate? occurrence)) (do
  (fail "Store action result has no canonical occurrence coordinate" :north.work-occurrences/invalid-store-commit {:input-index expected-index :occurrence occurrence})))
  (let [coordinate occurrence
   ordinal (t/triple-t3 coordinate)
   version (coordinate-version! coordinate)
   ^String space (coordinate-space! coordinate)]
  (if (not (= ordinal expected-index)) (do
  (fail "Store action occurrence ordinal does not match plan order" :north.work-occurrences/store-commit-mismatch {:expected expected-index :actual ordinal})))
  (if (not (= version expected-version)) (do
  (fail "Store action occurrence version does not match the commit" :north.work-occurrences/store-commit-mismatch {:expected expected-version :actual version})))
  (if (and (some? expected-space) (not= expected-space space)) (do
  (fail "Store action occurrences span multiple spaces" :north.work-occurrences/store-commit-mismatch {:expected expected-space :actual space})))
  (->CanonicalActionReceipt input-index true coordinate))))

(defn- ^CanonicalCommit decode-canonical-commit! [^PersistencePlan plan value]
  (if (not (exact-fields? value canonical-commit-fields)) (do
  (fail "canonical Store commit result has a malformed field set" :north.work-occurrences/invalid-store-commit {:result value})))
  (let [store-version (:ok value)
   changed (:changed? value)
   results (:results value)
   expected-version (inc (:expected-store-version plan))]
  (if (not (and (integer? store-version) (= expected-version store-version))) (do
  (fail "Store commit version does not immediately follow the planned base" :north.work-occurrences/store-commit-mismatch {:expected expected-version :actual store-version})))
  (if (not (true? changed)) (do
  (fail "canonical Store commit result does not prove a mutation" :north.work-occurrences/store-commit-mismatch {:changed changed})))
  (if (not (and (vector? results) (= (count results) (count (:actions plan))))) (do
  (fail "Store commit action count does not match the occurrence plan" :north.work-occurrences/store-commit-mismatch {:expected (count (:actions plan)) :actual (if (vector? results) (count results) nil)})))
  (let [decoded (loop [remaining results
   index 0
   known-space (:expected-store-space plan)
   accepted []]
  (if (empty? remaining) accepted (let [^CanonicalActionReceipt receipt (decode-action-receipt! (first remaining) index store-version known-space)
   ^String space (coordinate-space! (:occurrence receipt))]
  (recur (vec (rest remaining)) (inc index) space (conj accepted receipt)))))
   ^String space (if (empty? decoded) (fail "canonical Store commit has no action receipts" :north.work-occurrences/invalid-store-commit {:result value}) (coordinate-space! (:occurrence (first decoded))))]
  (->CanonicalCommit space store-version decoded))))

(def publication-option-fields #{:expected-version})

(defn- validate-publication-options! [^PersistencePlan plan options]
  (if (not (and (exact-fields? options publication-option-fields) (= (:expected-store-version plan) (:expected-version options)))) (do
  (fail "publication options do not match the planned Store snapshot" :north.work-occurrences/read-commit-race {:expected (publication-options plan) :actual options}))))

(defn- ^String intent-action [intent]
  (let [match__5 intent]
  (cond
    (instance? TrackIntent match__5) (let [_ (:referent match__5)] "track")
    (instance? PlanIntent match__5) (let [_ (:referent match__5) _ (:revision match__5)] "plan")
    (instance? StartIntent match__5) (let [_ (:referent match__5) _ (:occurrence match__5)] "start")
    (instance? AssignIntent match__5) (let [_ (:referent match__5) _ (:assignment match__5)] "assign")
    (instance? RequestIntent match__5) (let [_ (:request match__5) _ (:referent match__5)] "request")
    (instance? AckIntent match__5) (let [_ (:request match__5) _ (:ack match__5)] "ack")
    (instance? ResultIntent match__5) (let [_ (:request match__5) _ (:result match__5) _ (:outcome match__5) _ (:referent match__5)] "result")
    (instance? OwnershipIntent match__5) (let [_ (:transition match__5) _ (:owner match__5)] "ownership")
    (instance? SettleIntent match__5) (let [_ (:assignment match__5) _ (:accepted-transition match__5) _ (:settlement match__5) _ (:outcome match__5)] "settle"))))

(defn- receipt-from-intent [intent store-version]
  (let [match__6 intent]
  (cond
    (instance? TrackIntent match__6) (let [referent (:referent match__6)] (->TrackReceipt semantic-receipt-protocol semantic-receipt-version "track" store-version referent))
    (instance? PlanIntent match__6) (let [referent (:referent match__6) revision (:revision match__6)] (->PlanReceipt semantic-receipt-protocol semantic-receipt-version "plan" store-version referent revision))
    (instance? StartIntent match__6) (let [referent (:referent match__6) occurrence (:occurrence match__6)] (->StartReceipt semantic-receipt-protocol semantic-receipt-version "start" store-version referent occurrence))
    (instance? AssignIntent match__6) (let [referent (:referent match__6) assignment (:assignment match__6)] (->AssignReceipt semantic-receipt-protocol semantic-receipt-version "assign" store-version referent assignment))
    (instance? RequestIntent match__6) (let [request (:request match__6) referent (:referent match__6)] (let [bind__8 referent]
  (if bind__8 (let [^String about bind__8]
  (->RequestAboutReceipt semantic-receipt-protocol semantic-receipt-version "request" store-version request about)) (->RequestReceipt semantic-receipt-protocol semantic-receipt-version "request" store-version request))))
    (instance? AckIntent match__6) (let [request (:request match__6) ack (:ack match__6)] (->AckReceipt semantic-receipt-protocol semantic-receipt-version "ack" store-version request ack))
    (instance? ResultIntent match__6) (let [request (:request match__6) result (:result match__6) outcome (:outcome match__6) referent (:referent match__6)] (let [bind__9 referent]
  (if bind__9 (let [^String about bind__9]
  (->ResultAboutReceipt semantic-receipt-protocol semantic-receipt-version "result" store-version request result outcome about)) (->ResultReceipt semantic-receipt-protocol semantic-receipt-version "result" store-version request result outcome))))
    (instance? OwnershipIntent match__6) (let [transition (:transition match__6) owner (:owner match__6)] (->OwnershipReceipt semantic-receipt-protocol semantic-receipt-version "ownership" store-version transition owner))
    (instance? SettleIntent match__6) (let [assignment (:assignment match__6) accepted-transition (:accepted-transition match__6) settlement (:settlement match__6) outcome (:outcome match__6)] (->SettlementReceipt semantic-receipt-protocol semantic-receipt-version "settle" store-version assignment accepted-transition settlement outcome)))))

(defn semantic-receipt! [^PersistencePlan plan options canonical-store-result]
  (do
  (validate-publication-options! plan options)
  (if (not (= (:action plan) (intent-action (:receipt-intent plan)))) (do
  (fail "mutation plan action does not match its receipt identity" :north.work-occurrences/invalid-mutation-plan {:action (:action plan)})))
  (let [^CanonicalCommit commit (decode-canonical-commit! plan canonical-store-result)]
  (receipt-from-intent (:receipt-intent plan) (:store-version commit)))))

(defn- read-plan-snapshot! [^ReadPlan plan ^CanonicalSnapshot snapshot]
  (if (> (count (:facts snapshot)) (:limit plan)) (do
  (fail "canonical read snapshot exceeds its bounded plan" :north.work-occurrences/invalid-read-snapshot {:action (:action plan) :limit (:limit plan) :actual (count (:facts snapshot))}))))

(defn- sorted-view-facts [facts]
  (mapv (fn [fact] (->ViewFact (t/triple-t1 fact) (t/triple-t2 fact) (t/triple-t3 fact))) (vec (sort-by (fn [fact] (fact-key fact)) facts))))

(defn ^SemanticView semantic-view! [^ReadPlan plan ^CanonicalSnapshot snapshot]
  (do
  (if (not (and (= "show" (:action plan)) (= :subjects (:mode plan)))) (do
  (fail "semantic view requires a show read plan" :north.work-occurrences/invalid-read-plan {:action (:action plan) :mode (:mode plan)})))
  (read-plan-snapshot! plan snapshot)
  (let [^String referent (:identity plan)
   idx (proj/index-triples (:facts snapshot))]
  (if (not (referents/tracked-thing? idx referent)) (do
  (fail "semantic view subject is not a tracked thing" :north.work-occurrences/invalid-read-snapshot {:referent referent})))
  (let [derived (vec (concat (if (referents/agent? idx referent) ["Agent"] []) (vec (concat (if (referents/work? idx referent) ["Work"] []) (vec (concat (if (referents/goal? idx referent) ["Goal"] []) (vec (concat (if (referents/plan? idx referent) ["Plan"] []) (vec (concat (if (referents/project? idx referent) ["Project"] []) (if (referents/task? idx referent) ["Task"] [])))))))))))]
  (->SemanticView semantic-view-protocol semantic-receipt-version referent (sorted-view-facts (:facts snapshot)) derived)))))

(defn ^SemanticHistory semantic-history! [^ReadPlan plan ^CanonicalSnapshot snapshot]
  (do
  (if (not (and (= "history" (:action plan)) (= :about (:mode plan)))) (do
  (fail "semantic history requires a history read plan" :north.work-occurrences/invalid-read-plan {:action (:action plan) :mode (:mode plan)})))
  (read-plan-snapshot! plan snapshot)
  (let [^String referent (:identity plan)
   occurrences (vec (sort (distinct (mapv (fn [fact] (t/triple-t1 fact)) (filterv (fn [fact] (and (= "about" (t/triple-t2 fact)) (= referent (t/triple-t3 fact)))) (:facts snapshot))))))]
  (->SemanticHistory semantic-history-protocol semantic-receipt-version referent occurrences))))

(defn ^SemanticInbox semantic-inbox! [^ReadPlan plan ^CanonicalSnapshot snapshot]
  (do
  (if (not (and (= "inbox" (:action plan)) (= :inbox (:mode plan)))) (do
  (fail "semantic inbox requires an inbox read plan" :north.work-occurrences/invalid-read-plan {:action (:action plan) :mode (:mode plan)})))
  (read-plan-snapshot! plan snapshot)
  (let [^String actor (:identity plan)
   candidates (vec (sort (distinct (mapv (fn [fact] (t/triple-t1 fact)) (filterv (fn [fact] (and (= "to" (t/triple-t2 fact)) (= actor (t/triple-t3 fact)))) (:facts snapshot))))))
   requests (mapv (fn [^String request] (:request (decode-request-snapshot! snapshot request))) candidates)]
  (->SemanticInbox semantic-inbox-protocol semantic-receipt-version actor requests))))
