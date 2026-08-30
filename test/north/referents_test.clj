(ns north.referents-test
  (:require [store.types :as t]
            [north.projections :as proj]
            [north.referents :as referents]))

(defrecord Check [label passed])

(defn check-label [r] (:label r))

(defn check-passed [r] (:passed r))

(def checks (atom []))

(defn check! [^String label ^Boolean passed]
  (do
  (swap! checks conj (->Check label passed))
  nil))

(defn denied-type [operation]
  (try
  (do
  (operation)
  nil)
  (catch Throwable error
    (:type (ex-data error)))))

(def ^String tracker "@agent/tracker")

(def ^String worker "@agent/worker")

(def ^String other-worker "@agent/other")

(def ^String goal "@goal/release")

(def ^String plan "@plan/release")

(def ^String plain "@note/plain")

(def ^String revision "@plan/release/revision/1")

(def ^String authorization "@plan/release/start-authorization/1")

(def ^String assignment "@plan/release/assignment/1")

(def catalog-facts (vec (concat (referents/tracked-thing-facts! tracker "Tracker" tracker "2026-08-30T10:00:00Z") (referents/agent-role-facts! tracker) (referents/tracked-thing-facts! worker "Worker" tracker "2026-08-30T10:00:01Z") (referents/agent-role-facts! worker) (referents/tracked-thing-facts! other-worker "Other worker" tracker "2026-08-30T10:00:02Z") (referents/agent-role-facts! other-worker) (referents/tracked-thing-facts! goal "Release succeeds" tracker "2026-08-30T10:00:03Z") (referents/desired-outcome-facts! goal "The release is available to its intended users") (referents/tracked-thing-facts! plan "Release path" tracker "2026-08-30T10:00:04Z") (referents/plan-revision-facts! plan revision "Publish the accepted build" tracker "2026-08-30T10:01:00Z") (referents/tracked-thing-facts! plain "Plain tracked note" tracker "2026-08-30T10:00:05Z") [(t/triple "@legacy/actor" "entity_kind" "actor") (t/triple "@role-only" "referent_role" referents/agent-role)])))

(def catalog-idx (proj/index-triples catalog-facts))

(check! "All catalogs tracked things and excludes occurrences and legacy actors" (= [tracker worker other-worker goal plan plain] (referents/all-tracked-thing-ids catalog-idx)))

(check! "Agents are tracked identities playing the agent role" (= [tracker worker other-worker] (referents/agent-ids catalog-idx)))

(check! "an agent role without tracked identity is not an Agent" (not (referents/agent? catalog-idx "@role-only")))

(check! "legacy actor shape is not an Agent compatibility path" (not (referents/agent? catalog-idx "@legacy/actor")))

(check! "Goals derives only tracked things with a desired outcome" (= [goal] (referents/goal-ids catalog-idx)))

(check! "Plan is not a universal Goal supertype" (and (referents/plan? catalog-idx plan) (not (referents/goal? catalog-idx plan))))

(check! "Goal is not a magic Plan container" (and (referents/goal? catalog-idx goal) (not (referents/plan? catalog-idx goal))))

(check! "blank desired outcomes fail at typed fact construction" (= :north/invalid-semantic-value (denied-type (fn [] (referents/desired-outcome-facts! goal "")))))

(def ^String base-only-occurrence "@message/envelope/without-about")

(def base-only-idx (proj/index-triples (referents/occurrence-facts! base-only-occurrence "message_envelope" tracker "2026-08-30T10:01:30Z")))

(check! "generic occurrence base permits about to be absent" (and (referents/complete-base-occurrence? base-only-idx base-only-occurrence "message_envelope" []) (empty? (proj/string-values-at base-only-idx base-only-occurrence "about"))))

(check! "optional about cannot encode blank as absent" (= :north/invalid-semantic-value (denied-type (fn [] (referents/occurrence-about-facts! base-only-occurrence "")))))

(def linked-base-idx (proj/index-triples (vec (concat (referents/occurrence-facts! base-only-occurrence "message_envelope" tracker "2026-08-30T10:01:30Z") (referents/occurrence-about-facts! base-only-occurrence plan)))))

(check! "separate optional-about facts produce a complete linked occurrence" (referents/complete-occurrence? linked-base-idx base-only-occurrence plan "message_envelope" []))

(check! "an occurrence cannot reuse tracked identity" (= :north/occurrence-identity-collision (denied-type (fn [] (referents/assignment-facts! plan plan tracker worker "2026-08-30T10:02:00Z")))))

(check! "endorsement establishes Plan but does not authorize Project" (and (referents/plan? catalog-idx plan) (not (referents/project? catalog-idx plan))))

(def wrong-authorization-facts (referents/project-start-authorization-facts! "@plan/release/start-authorization/wrong" plan "@plan/release/revision/unknown" tracker "sig:wrong" "2026-08-30T10:02:00Z"))

(def wrong-authorization-idx (proj/index-triples (vec (concat catalog-facts wrong-authorization-facts))))

(check! "signed authorization fails closed when its exact Plan revision is absent" (not (referents/project? wrong-authorization-idx plan)))

(check! "authorization signature must be nonblank" (= :north/invalid-semantic-value (denied-type (fn [] (referents/project-start-authorization-facts! authorization plan revision tracker "" "2026-08-30T10:02:00Z")))))

(check! "authorization has identity distinct from the exact Plan revision" (= :north/occurrence-identity-collision (denied-type (fn [] (referents/project-start-authorization-facts! revision plan revision tracker "sig:collision" "2026-08-30T10:02:00Z")))))

(def authorization-facts (referents/project-start-authorization-facts! authorization plan revision tracker "sig:accepted" "2026-08-30T10:02:00Z"))

(def project-idx (proj/index-triples (vec (concat catalog-facts authorization-facts))))

(check! "valid signed start authorization derives Project on the Plan identity" (referents/project? project-idx plan))

(check! "Project authorization does not mint a second tracked identity" (= (referents/all-tracked-thing-ids catalog-idx) (referents/all-tracked-thing-ids project-idx)))

(def assignment-facts (referents/assignment-facts! assignment plan tracker worker "2026-08-30T10:03:00Z"))

(def task-idx (proj/index-triples (vec (concat catalog-facts assignment-facts))))

(def project-task-idx (proj/index-triples (vec (concat catalog-facts authorization-facts assignment-facts))))

(check! "complete Assignment derives Task from Plan without requiring Project" (and (referents/assignment-occurrence? task-idx assignment plan) (referents/task? task-idx plan) (not (referents/project? task-idx plan))))

(check! "Task may also be the same identity as Project" (and (referents/task? project-task-idx plan) (referents/project? project-task-idx plan)))

(check! "Assignment is an immutable occurrence and has no driver fact" (and (= [referents/occurrence-kind] (proj/string-values-at task-idx assignment "entity_kind")) (= [plan] (proj/string-values-at task-idx assignment "about")) (empty? (proj/string-values-at task-idx assignment "driver"))))

(def conflicting-assignment-idx (proj/index-triples (conj (vec (concat catalog-facts assignment-facts)) (t/triple assignment "assignee" other-worker))))

(check! "conflicting mutation invalidates the immutable Assignment occurrence" (and (not (referents/assignment-occurrence? conflicting-assignment-idx assignment plan)) (not (referents/task? conflicting-assignment-idx plan))))

(def ^String plain-assignment "@note/plain/assignment/1")

(def plain-assignment-idx (proj/index-triples (vec (concat catalog-facts (referents/assignment-facts! plain-assignment plain tracker worker "2026-08-30T10:04:00Z")))))

(check! "complete Assignment alone cannot turn a non-Plan into Task" (and (referents/assignment-occurrence? plain-assignment-idx plain-assignment plain) (not (referents/task? plain-assignment-idx plain))))

(let [results (deref checks)
   passed (count (filter (fn [^Check result] (check-passed result)) results))]
  (doseq [result results]
  (println (format "  [%s] %s" (if (check-passed result) "PASS" "FAIL") (check-label result))))
  (println (format "\nReferent core B1: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
