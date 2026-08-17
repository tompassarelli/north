(ns north.projections
  (:require [store.types :as t]
            [store.kernel-classify :as kc]
            [clojure.string :as str]))

(defrecord ProjectionIndex [single many subjects subject-set reverse-dependencies reference-predicates acyclic-predicates])

(defn projectionindex-single [r] (:single r))

(defn projectionindex-many [r] (:many r))

(defn projectionindex-subjects [r] (:subjects r))

(defn projectionindex-subject-set [r] (:subject-set r))

(defn projectionindex-reverse-dependencies [r] (:reverse-dependencies r))

(defn projectionindex-reference-predicates [r] (:reference-predicates r))

(defn projectionindex-acyclic-predicates [r] (:acyclic-predicates r))

(def no-terms [])

(def fallback-reference-predicates ["depends_on" "part_of" "relates_to" "clarifies" "amends"])

(def fallback-acyclic-predicates ["depends_on" "part_of"])

(defn- ^String schema-predicate-name [^String subject]
  (if (str/starts-with? subject "@") (subs subject 1) subject))

(defn- set-string-membership [members ^String value ^Boolean present?]
  (if present? (if (kc/vec-member? members value) members (conj members value)) (filterv (fn [^String member] (not (= member value))) members)))

(defn ^ProjectionIndex index-triples [triples]
  (let [base (reduce (fn [^ProjectionIndex index triple] (let [subject (t/triple-t1 triple)
   predicate (t/triple-t2 triple)
   value (t/triple-t3 triple)
   key [subject predicate]
   values (conj (get (projectionindex-many index) key no-terms) value)
   reverse (if (= predicate "depends_on") (assoc (projectionindex-reverse-dependencies index) value (conj (get (projectionindex-reverse-dependencies index) value no-terms) subject)) (projectionindex-reverse-dependencies index))
   known-subject (contains? (projectionindex-subject-set index) subject)]
  (->ProjectionIndex (assoc (projectionindex-single index) key value) (assoc (projectionindex-many index) key values) (if known-subject (projectionindex-subjects index) (conj (projectionindex-subjects index) subject)) (if known-subject (projectionindex-subject-set index) (assoc (projectionindex-subject-set index) subject true)) reverse (projectionindex-reference-predicates index) (projectionindex-acyclic-predicates index)))) (->ProjectionIndex {} {} [] {} {} [] []) triples)
   schema (reduce (fn [rows subject] (if (string? subject) (let [value-kind (get (projectionindex-single base) [subject "value_kind"])
   acyclic (get (projectionindex-single base) [subject "acyclic"])]
  (if (or (string? value-kind) (string? acyclic)) (conj rows [(schema-predicate-name subject) (if (string? value-kind) value-kind "") (if (string? acyclic) acyclic "")]) rows)) rows)) [] (projectionindex-subjects base))
   reference-predicates (reduce (fn [predicates row] (let [kind (nth row 1)]
  (if (= kind "") predicates (set-string-membership predicates (nth row 0) (= kind "ref"))))) fallback-reference-predicates schema)
   acyclic-predicates (reduce (fn [predicates row] (let [declared (nth row 2)]
  (if (= declared "") predicates (set-string-membership predicates (nth row 0) (= declared "true"))))) fallback-acyclic-predicates schema)]
  (->ProjectionIndex (projectionindex-single base) (projectionindex-many base) (projectionindex-subjects base) (projectionindex-subject-set base) (projectionindex-reverse-dependencies base) reference-predicates acyclic-predicates)))

(defn value-at [^ProjectionIndex index subject predicate]
  (get (projectionindex-single index) [subject predicate]))

(defn values-at [^ProjectionIndex index subject predicate]
  (get (projectionindex-many index) [subject predicate] no-terms))

(defn string-value-at [^ProjectionIndex index subject predicate]
  (let [value (value-at index subject predicate)]
  (if (string? value) value nil)))

(defn string-values-at [^ProjectionIndex index subject predicate]
  (reduce (fn [values value] (if (string? value) (conj values value) values)) [] (values-at index subject predicate)))

(defn all-subjects [^ProjectionIndex index]
  (reduce (fn [subjects subject] (if (string? subject) (conj subjects subject) subjects)) [] (projectionindex-subjects index)))

(defn ^Boolean subject? [^ProjectionIndex index ^String subject]
  (contains? (projectionindex-subject-set index) subject))

(def configured-single-predicates (let [configured (System/getenv "BEAGLE_STORE_SINGLE_VALUED")]
  (if (and (some? configured) (not (str/blank? configured))) (vec (str/split configured #"\s+")) [])))

(defn ^Boolean single-valued? [^ProjectionIndex index ^String predicate]
  (let [declared (string-value-at index (str "@" predicate) "cardinality")]
  (kc/single-eff? (some? declared) (= declared "single") (kc/configured-single? configured-single-predicates predicate) predicate)))

(defn thread-subjects [^ProjectionIndex index]
  (reduce (fn [subjects subject] (if (and (string? subject) (some? (string-value-at index subject "title"))) (conj subjects subject) subjects)) [] (projectionindex-subjects index)))

(defn dependents-of [^ProjectionIndex index ^String subject]
  (reduce (fn [subjects candidate] (if (string? candidate) (conj subjects candidate) subjects)) [] (get (projectionindex-reverse-dependencies index) subject no-terms)))

(def terminal-preds (let [env (System/getenv "BEAGLE_STORE_TERMINAL_PREDS")]
  (if (and (some? env) (not (= env ""))) (vec (str/split env #"\s+")) ["outcome" "abandoned" "superseded_by"])))

(def withdrawn-preds (let [env (System/getenv "BEAGLE_STORE_WITHDRAWN_PREDS")]
  (if (and (some? env) (not (= env ""))) (vec (str/split env #"\s+")) ["abandoned"])))

(defn- ^Boolean any-of-i? [^ProjectionIndex idx ^String te preds]
  (loop [ps preds]
  (if (empty? ps) false (if (some? (string-value-at idx te (first ps))) true (recur (rest ps))))))

(defn ^Boolean terminal-i? [^ProjectionIndex idx ^String te]
  (any-of-i? idx te terminal-preds))

(defn ^Boolean withdrawn-i? [^ProjectionIndex idx ^String te]
  (any-of-i? idx te withdrawn-preds))

(defn ^Boolean anchor-i? [^ProjectionIndex idx ^String te]
  (and (some? (string-value-at idx te "title")) (and (some? (string-value-at idx te "committed")) (and (not (terminal-i? idx te)) (and (nil? (string-value-at idx te "driver")) (and (empty? (string-values-at idx te "depends_on")) (and (nil? (string-value-at idx te "part_of")) (and (nil? (string-value-at idx te "do_on")) (and (nil? (string-value-at idx te "valid_until")) (and (nil? (string-value-at idx te "estimate_hours")) (and (nil? (string-value-at idx te "lead")) (and (empty? (string-values-at idx te "proposed_by")) (and (nil? (string-value-at idx te "created_at")) (and (nil? (string-value-at idx te "updated_at")) (nil? (string-value-at idx te "repo"))))))))))))))))

(defn work-thread-ids-i [^ProjectionIndex idx]
  (filterv (fn [^String s] (not (anchor-i? idx s))) (thread-subjects idx)))

(defn incomplete-deps [^ProjectionIndex idx ^String te]
  (filterv (fn [^String d] (and (some? (string-value-at idx d "title")) (not (terminal-i? idx d)))) (string-values-at idx te "depends_on")))

(defn ^Boolean blocked? [^ProjectionIndex idx ^String te]
  (not (empty? (incomplete-deps idx te))))

(defn ^Boolean dormant? [^ProjectionIndex idx ^String te ^String today before?]
  (let [d (string-value-at idx te "do_on")]
  (and (some? d) (before? today d))))

(defn ^Boolean assigned? [^ProjectionIndex idx ^String te]
  (some? (string-value-at idx te "driver")))

(defn ^String classify [^ProjectionIndex idx ^String te ^String today before? live?]
  (cond
  (terminal-i? idx te) "terminal"
  (blocked? idx te) "blocked"
  (live? idx te) "active"
  (dormant? idx te today before?) "dormant"
  (some? (string-value-at idx te "committed")) "ready"
  :else "draft"))

(defn ^Boolean eligible? [^ProjectionIndex idx ^String te ^String today before? live?]
  (= (classify idx te today before? live?) "ready"))

(defn ready [^ProjectionIndex idx ^String today before? live?]
  (filterv (fn [^String te] (eligible? idx te today before? live?)) (work-thread-ids-i idx)))

(defrecord Eligibility [state eligible reason])

(defn eligibility-state [r] (:state r))

(defn eligibility-eligible [r] (:eligible r))

(defn eligibility-reason [r] (:reason r))

(defn ^Eligibility explain [^ProjectionIndex idx ^String te ^String today before? live?]
  (let [st (classify idx te today before? live?)]
  (->Eligibility st (= st "ready") (cond
  (= st "terminal") "resolved (outcome/abandoned/superseded_by) — not workable"
  (= st "blocked") (str "waiting on " (count (incomplete-deps idx te)) " incomplete dependency(ies)")
  (= st "active") "a live driver is on it now — being worked, not pull-able"
  (= st "dormant") (str "scheduled for a future do_on (" (let [d (string-value-at idx te "do_on")]
  (if (some? d) d "?")) ") — dormant until then")
  (= st "ready") "committed, unblocked, no live driver, not scheduled-later — pull anytime"
  :else "uncommitted draft — decide + commit before it is work"))))

(defn blocked [^ProjectionIndex idx]
  (filterv (fn [^String te] (and (not (terminal-i? idx te)) (blocked? idx te))) (work-thread-ids-i idx)))

(defn ^String condition-i [^ProjectionIndex idx ^String te ^String today before? live?]
  (classify idx te today before? live?))

(defn- ^String default-emoji [^String c]
  (cond
  (= c "active") "🔵"
  (= c "ready") "🟢"
  (= c "blocked") "🔴"
  (= c "dormant") "🟡"
  (= c "terminal") "⚫"
  (= c "draft") "⚪"
  :else "•"))

(defn ^String condition-emoji [^ProjectionIndex idx ^String c]
  (let [o (string-value-at idx "@ui" (str "emoji_" c))]
  (if (some? o) o (default-emoji c))))

(defn transitive-dependents [^ProjectionIndex idx ^String te]
  (loop [frontier (dependents-of idx te)
   seen {}
   ordered []]
  (if (empty? frontier) ordered (let [x (first frontier)
   rest-f (vec (rest frontier))]
  (if (contains? seen x) (recur rest-f seen ordered) (recur (vec (concat rest-f (dependents-of idx x))) (assoc seen x true) (conj ordered x)))))))

(defn leverage-score [^ProjectionIndex idx ^String te]
  (count (filterv (fn [^String d] (and (not (= d te)) (not (terminal-i? idx d)))) (transitive-dependents idx te))))
