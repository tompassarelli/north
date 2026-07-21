(ns north.projections
  (:require [fram.kernel :as k]
            [clojure.string :as str]))

(def terminal-preds (let [env (System/getenv "FRAM_TERMINAL_PREDS")]
  (if (and (some? env) (not (= env ""))) (vec (str/split env #"\s+")) ["outcome" "abandoned" "superseded_by"])))

(def withdrawn-preds (let [env (System/getenv "FRAM_WITHDRAWN_PREDS")]
  (if (and (some? env) (not (= env ""))) (vec (str/split env #"\s+")) ["abandoned"])))

(defn- ^Boolean any-of-i? [idx ^String te preds]
  (loop [ps preds]
  (if (empty? ps) false (if (some? (k/one-i idx te (first ps))) true (recur (rest ps))))))

(defn ^Boolean terminal-i? [idx ^String te]
  (any-of-i? idx te terminal-preds))

(defn ^Boolean withdrawn-i? [idx ^String te]
  (any-of-i? idx te withdrawn-preds))

(defn ^Boolean anchor-i? [idx ^String te]
  (and (some? (k/one-i idx te "title")) (and (some? (k/one-i idx te "committed")) (and (not (terminal-i? idx te)) (and (nil? (k/one-i idx te "driver")) (and (empty? (k/many-i idx te "depends_on")) (and (nil? (k/one-i idx te "part_of")) (and (nil? (k/one-i idx te "do_on")) (and (nil? (k/one-i idx te "valid_until")) (and (nil? (k/one-i idx te "estimate_hours")) (and (nil? (k/one-i idx te "lead")) (and (empty? (k/many-i idx te "proposed_by")) (and (nil? (k/one-i idx te "created_at")) (and (nil? (k/one-i idx te "updated_at")) (nil? (k/one-i idx te "repo"))))))))))))))))

(defn work-thread-ids-i [idx]
  (filterv (fn [s] (not (anchor-i? idx s))) (k/thread-ids-i idx)))

(defn incomplete-deps [idx ^String te]
  (filterv (fn [d] (and (some? (k/one-i idx d "title")) (not (terminal-i? idx d)))) (k/many-i idx te "depends_on")))

(defn ^Boolean blocked? [idx ^String te]
  (not (empty? (incomplete-deps idx te))))

(defn ^Boolean dormant? [idx ^String te ^String today before?]
  (let [d (k/one-i idx te "do_on")]
  (and (some? d) (before? today d))))

(defn ^Boolean assigned? [idx ^String te]
  (some? (k/one-i idx te "driver")))

(defn ^String classify [idx ^String te ^String today before? live?]
  (cond
  (terminal-i? idx te) "terminal"
  (blocked? idx te) "blocked"
  (live? idx te) "active"
  (dormant? idx te today before?) "dormant"
  (some? (k/one-i idx te "committed")) "ready"
  :else "draft"))

(defn ^Boolean eligible? [idx ^String te ^String today before? live?]
  (= (classify idx te today before? live?) "ready"))

(defn ready [idx ^String today before? live?]
  (filterv (fn [te] (eligible? idx te today before? live?)) (work-thread-ids-i idx)))

(defrecord Eligibility [state eligible reason])

(defn eligibility-state [r] (:state r))

(defn eligibility-eligible [r] (:eligible r))

(defn eligibility-reason [r] (:reason r))

(defn ^Eligibility explain [idx ^String te ^String today before? live?]
  (let [st (classify idx te today before? live?)]
  (->Eligibility st (= st "ready") (cond
  (= st "terminal") "resolved (outcome/abandoned/superseded_by) — not workable"
  (= st "blocked") (str "waiting on " (count (incomplete-deps idx te)) " incomplete dependency(ies)")
  (= st "active") "a live driver is on it now — being worked, not pull-able"
  (= st "dormant") (str "scheduled for a future do_on (" (let [d (k/one-i idx te "do_on")]
  (if (some? d) d "?")) ") — dormant until then")
  (= st "ready") "committed, unblocked, no live driver, not scheduled-later — pull anytime"
  :else "uncommitted draft — decide + commit before it is work"))))

(defn blocked [idx]
  (filterv (fn [te] (and (not (terminal-i? idx te)) (blocked? idx te))) (work-thread-ids-i idx)))

(defn ^String condition-i [idx ^String te ^String today before? live?]
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

(defn ^String condition-emoji [idx ^String c]
  (let [o (k/one-i idx "@ui" (str "emoji_" c))]
  (if (some? o) o (default-emoji c))))

(defn transitive-dependents [idx ^String te]
  (loop [frontier (k/dependents-i idx te)
   seen []]
  (if (empty? frontier) seen (let [x (first frontier)
   rest-f (vec (rest frontier))]
  (if (k/vec-contains? seen x) (recur rest-f seen) (recur (vec (concat rest-f (k/dependents-i idx x))) (conj seen x)))))))

(defn leverage-score [idx ^String te]
  (count (filterv (fn [d] (and (not (= d te)) (not (terminal-i? idx d)))) (transitive-dependents idx te))))
