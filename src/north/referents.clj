(ns north.referents
  (:require [store.types :as t]
            [north.projections :as proj]
            [clojure.string :as str]))

(def ^String referent-kind "referent")

(def ^String occurrence-kind "occurrence")

(def ^String agent-role "agent")

(def ^String work-role "work")

(def ^String plan-role "plan")

(def ^String plan-revision-kind "plan_revision")

(def ^String started-kind "started")

(def ^String assignment-kind "assignment")

(defn- ^String nonblank! [^String field ^String value]
  (if (str/blank? value) (throw (ex-info (str field " must be nonblank") {:type :north/invalid-semantic-value :field field})) value))

(defn- distinct-identity! [^String occurrence ^String tracked-thing]
  (if (= occurrence tracked-thing) (do
  (throw (ex-info "an occurrence cannot reuse its tracked thing identity" {:type :north/occurrence-identity-collision :tracked-thing tracked-thing :occurrence occurrence})))))

(defn occurrence-facts! [^String occurrence ^String kind ^String actor ^String at]
  (do
  (nonblank! "occurrence" occurrence)
  (nonblank! "occurrence kind" kind)
  (nonblank! "actor" actor)
  (nonblank! "occurrence instant" at)
  [(t/triple occurrence "entity_kind" occurrence-kind) (t/triple occurrence "occurrence_kind" kind) (t/triple occurrence "actor" actor) (t/triple occurrence "at" at)]))

(defn occurrence-about-facts! [^String occurrence ^String tracked-thing]
  (do
  (nonblank! "occurrence" occurrence)
  (nonblank! "tracked thing" tracked-thing)
  (distinct-identity! occurrence tracked-thing)
  [(t/triple occurrence "about" tracked-thing)]))

(defn- tracked-occurrence-facts! [^String occurrence ^String kind ^String tracked-thing ^String actor ^String at]
  (vec (concat (occurrence-facts! occurrence kind actor at) (occurrence-about-facts! occurrence tracked-thing))))

(defn tracked-thing-facts! [^String tracked-thing ^String title ^String tracked-by ^String tracked-at]
  (do
  (nonblank! "tracked thing" tracked-thing)
  (nonblank! "title" title)
  (nonblank! "tracking actor" tracked-by)
  (nonblank! "tracking instant" tracked-at)
  [(t/triple tracked-thing "entity_kind" referent-kind) (t/triple tracked-thing "title" title) (t/triple tracked-thing "tracked_by" tracked-by) (t/triple tracked-thing "tracked_at" tracked-at)]))

(defn agent-role-facts! [^String tracked-thing]
  (do
  (nonblank! "tracked thing" tracked-thing)
  [(t/triple tracked-thing "referent_role" agent-role)]))

(defn work-role-facts! [^String tracked-thing]
  (do
  (nonblank! "tracked thing" tracked-thing)
  [(t/triple tracked-thing "referent_role" work-role)]))

(defn desired-outcome-facts! [^String tracked-thing ^String outcome]
  (do
  (nonblank! "tracked thing" tracked-thing)
  (nonblank! "desired outcome" outcome)
  [(t/triple tracked-thing "desired_outcome" outcome)]))

(defn plan-revision-facts! [^String tracked-thing ^String revision ^String intended-path ^String endorsed-by ^String endorsed-at]
  (do
  (nonblank! "intended path or change" intended-path)
  (vec (concat [(t/triple tracked-thing "referent_role" work-role) (t/triple tracked-thing "referent_role" plan-role) (t/triple tracked-thing "current_plan_revision" revision)] (conj (tracked-occurrence-facts! revision plan-revision-kind tracked-thing endorsed-by endorsed-at) (t/triple revision "body" intended-path))))))

(defn start-facts! [^String start ^String tracked-thing ^String revision ^String started-by ^String signature ^String started-at]
  (do
  (nonblank! "authorized Plan revision" revision)
  (nonblank! "start signature" signature)
  (if (= start revision) (do
  (throw (ex-info "a start must have its own occurrence identity" {:type :north/occurrence-identity-collision :start start :revision revision}))))
  (vec (concat (tracked-occurrence-facts! start started-kind tracked-thing started-by started-at) [(t/triple start "plan_revision" revision) (t/triple start "signature" signature)]))))

(defn assignment-facts! [^String assignment ^String tracked-thing ^String assigned-by ^String assignee ^String assigned-at]
  (do
  (nonblank! "assignee" assignee)
  (conj (tracked-occurrence-facts! assignment assignment-kind tracked-thing assigned-by assigned-at) (t/triple assignment "assignee" assignee))))

(defn- ^Boolean exact-value? [idx ^String subject ^String predicate ^String expected]
  (= [expected] (proj/string-values-at idx subject predicate)))

(defn- ^Boolean exact-nonblank-value? [idx ^String subject ^String predicate]
  (let [values (proj/string-values-at idx subject predicate)]
  (and (= 1 (count values)) (not (str/blank? (first values))))))

(defn- ^Boolean has-role? [idx ^String tracked-thing ^String role]
  (boolean (some (fn [^String candidate] (= candidate role)) (proj/string-values-at idx tracked-thing "referent_role"))))

(defn ^Boolean tracked-thing? [idx ^String subject]
  (exact-value? idx subject "entity_kind" referent-kind))

(defn ^Boolean agent? [idx ^String subject]
  (and (tracked-thing? idx subject) (has-role? idx subject agent-role)))

(defn ^Boolean work? [idx ^String subject]
  (and (tracked-thing? idx subject) (has-role? idx subject work-role)))

(defn ^Boolean goal? [idx ^String subject]
  (and (tracked-thing? idx subject) (exact-nonblank-value? idx subject "desired_outcome")))

(defn- tracked-things-matching [idx included?]
  (filterv (fn [^String subject] (included? idx subject)) (proj/all-subjects idx)))

(defn all-tracked-thing-ids [idx]
  (tracked-things-matching idx tracked-thing?))

(defn agent-ids [idx]
  (tracked-things-matching idx agent?))

(defn goal-ids [idx]
  (tracked-things-matching idx goal?))

(defn ^Boolean complete-base-occurrence? [idx ^String occurrence ^String kind required-predicates]
  (and (exact-value? idx occurrence "entity_kind" occurrence-kind) (exact-value? idx occurrence "occurrence_kind" kind) (exact-nonblank-value? idx occurrence "actor") (exact-nonblank-value? idx occurrence "at") (every? (fn [^String predicate] (exact-nonblank-value? idx occurrence predicate)) required-predicates)))

(defn ^Boolean complete-occurrence? [idx ^String occurrence ^String tracked-thing ^String kind required-predicates]
  (and (not= occurrence tracked-thing) (complete-base-occurrence? idx occurrence kind required-predicates) (exact-value? idx occurrence "about" tracked-thing)))

(defn occurrence-ids [idx ^String tracked-thing ^String kind]
  (filterv (fn [^String subject] (and (exact-value? idx subject "about" tracked-thing) (exact-value? idx subject "occurrence_kind" kind))) (proj/all-subjects idx)))

(defn ^Boolean plan-revision-occurrence? [idx ^String revision ^String tracked-thing]
  (and (complete-occurrence? idx revision tracked-thing plan-revision-kind ["body"]) (agent? idx (or (proj/string-value-at idx revision "actor") ""))))

(defn ^Boolean plan? [idx ^String tracked-thing]
  (let [revision (proj/string-value-at idx tracked-thing "current_plan_revision")]
  (and (tracked-thing? idx tracked-thing) (has-role? idx tracked-thing plan-role) (some? revision) (exact-value? idx tracked-thing "current_plan_revision" revision) (plan-revision-occurrence? idx revision tracked-thing))))

(defn ^Boolean start-occurrence? [idx ^String start ^String tracked-thing]
  (let [revision (proj/string-value-at idx start "plan_revision")
   actor (proj/string-value-at idx start "actor")]
  (and (complete-occurrence? idx start tracked-thing started-kind ["plan_revision" "signature"]) (some? revision) (some? actor) (not= start revision) (agent? idx actor) (plan-revision-occurrence? idx revision tracked-thing))))

(defn ^Boolean project? [idx ^String tracked-thing]
  (and (plan? idx tracked-thing) (boolean (some (fn [^String start] (start-occurrence? idx start tracked-thing)) (occurrence-ids idx tracked-thing started-kind)))))

(defn ^Boolean assignment-occurrence? [idx ^String assignment ^String tracked-thing]
  (let [assigned-by (proj/string-value-at idx assignment "actor")
   assignee (proj/string-value-at idx assignment "assignee")]
  (and (complete-occurrence? idx assignment tracked-thing assignment-kind ["assignee"]) (some? assigned-by) (some? assignee) (agent? idx assigned-by) (agent? idx assignee))))

(defn ^Boolean task? [idx ^String tracked-thing]
  (and (plan? idx tracked-thing) (boolean (some (fn [^String assignment] (assignment-occurrence? idx assignment tracked-thing)) (occurrence-ids idx tracked-thing assignment-kind)))))
