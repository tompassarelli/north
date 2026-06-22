(ns lodestar.projections
  (:require [fram.kernel :as k]))

(defn incomplete-deps [idx ^String te]
  (filterv (fn [d] (and (some? (k/one-i idx d "title")) (not (k/terminal-i? idx d)))) (k/many-i idx te "depends_on")))

(defn ^Boolean blocked? [idx ^String te]
  (not (empty? (incomplete-deps idx te))))

(defn ^Boolean dormant? [idx ^String te ^String today before?]
  (let [d (k/one-i idx te "do_on")]
  (and (some? d) (before? today d))))

(defn ^String classify [idx ^String te ^String today before?]
  (cond
  (k/terminal-i? idx te) "terminal"
  (blocked? idx te) "blocked"
  (some? (k/one-i idx te "driver")) "active"
  (some? (k/one-i idx te "committed")) "ready"
  (dormant? idx te today before?) "dormant"
  :else "draft"))

(defn ready [idx ^String today before?]
  (filterv (fn [te] (and (not (k/terminal-i? idx te)) (and (not (blocked? idx te)) (not (dormant? idx te today before?))))) (k/work-thread-ids-i idx)))

(defn blocked [idx]
  (filterv (fn [te] (and (not (k/terminal-i? idx te)) (blocked? idx te))) (k/work-thread-ids-i idx)))

(defn ^String condition-i [idx ^String te ^String today before?]
  (classify idx te today before?))

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
  (count (filterv (fn [d] (and (not (= d te)) (not (k/terminal-i? idx d)))) (transitive-dependents idx te))))
