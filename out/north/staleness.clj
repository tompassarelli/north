(ns north.staleness
  (:require [clojure.string :as str]
            [fram.kernel-classify :as kc]
            [north.projections :as proj]))

(defrecord Review [te pred detail])

(defn review-te [r] (:te r))

(defn review-pred [r] (:pred r))

(defn review-detail [r] (:detail r))

(defrecord Latest [tx l p r frame])

(defn latest-tx [r] (:tx r))

(defn latest-l [r] (:l r))

(defn latest-p [r] (:p r))

(defn latest-r [r] (:r r))

(defn latest-frame [r] (:frame r))

(def scope-preds ["depends_on" "part_of" "body"])

(def edge-preds ["relates_to" "clarifies" "amends"])

(defn- ^Boolean real-edit-frame? [^String fr]
  (or (= fr "coord") (or (= fr "agent") (= fr "cli"))))

(defn time-stale [idx ^String today before?]
  (reduce (fn [acc ^String te] (let [vu (proj/string-value-at idx te "valid_until")]
  (if (and (not (proj/terminal-i? idx te)) (and (some? vu) (before? vu today))) (conj acc (->Review te "valid_until" (str vu " is past " today " — re-validate or drop"))) acc))) [] (proj/thread-subjects idx)))

(defn edge-stale [idx]
  (reduce (fn [acc ^String te] (if (proj/terminal-i? idx te) acc (reduce (fn [a ^String p] (reduce (fn [b ^String tgt] (if (some? (proj/string-value-at idx tgt "abandoned")) (conj b (->Review te p (str "→ " tgt " was abandoned — relationship may be stale"))) b)) a (proj/string-values-at idx te p))) acc edge-preds))) [] (proj/thread-subjects idx)))

(defn- later-edit-scope-tx [latest ^String l]
  (reduce (fn [m ^Latest v] (if (and (= (:l v) l) (and (kc/vec-member? scope-preds (:p v)) (and (real-edit-frame? (:frame v)) (> (:tx v) m)))) (:tx v) m)) 0 latest))

(defn estimate-stale [idx latest]
  (reduce (fn [acc ^Latest v] (if (and (= (:p v) "estimate_hours") (and (not (proj/terminal-i? idx (:l v))) (> (later-edit-scope-tx latest (:l v)) (:tx v)))) (conj acc (->Review (:l v) "estimate_hours" (str (:r v) "h estimated before a later scope edit — re-estimate"))) acc)) [] latest))

(defn ^String bar-mark [evs ^String bar]
  (if (some? (first (filterv (fn [^String e] (str/includes? e bar)) evs))) "✓" "○"))

(defn bars-missing [idx]
  (reduce (fn [acc ^String te] (if (and (some? (proj/string-value-at idx te "committed")) (and (some? (proj/string-value-at idx te "driver")) (and (not (proj/terminal-i? idx te)) (empty? (proj/string-values-at idx te "done_when"))))) (conj acc (->Review te "done_when" "committed + driven with no done bar — tell it done_when \"<probe + expected result>\"")) acc)) [] (proj/thread-subjects idx)))

(defn bars-unevidenced [idx]
  (reduce (fn [acc ^String te] (let [bars (proj/string-values-at idx te "done_when")]
  (if (and (some? (proj/string-value-at idx te "outcome")) (not (empty? bars))) (let [evs (proj/string-values-at idx te "bar_evidence")
   marks (mapv (fn [^String b] (str (bar-mark evs b) " " b)) bars)
   open (filterv (fn [^String m] (str/starts-with? m "○")) marks)]
  (if (empty? open) acc (conj acc (->Review te "outcome" (str (- (count bars) (count open)) "/" (count bars) " bar(s) evidenced — " (reduce (fn [^String s ^String m] (if (str/blank? s) m (str s " · " m))) "" marks) "  (bar_evidence \"<bar> → <observed result>\")"))))) acc))) [] (proj/thread-subjects idx)))

(defn needs-review [idx latest ^String today before?]
  (vec (concat (time-stale idx today before?) (vec (concat (edge-stale idx) (vec (concat (estimate-stale idx latest) (vec (concat (bars-missing idx) (bars-unevidenced idx))))))))))

(defn- ^Boolean has-structure? [idx ^String te]
  (or (some? (proj/string-value-at idx te "driver")) (or (some? (proj/string-value-at idx te "estimate_hours")) (or (some? (proj/string-value-at idx te "part_of")) (or (not (empty? (proj/string-values-at idx te "depends_on"))) (not (empty? (proj/string-values-at idx te "relates_to"))))))))

(defn promotable [idx]
  (filterv (fn [^String te] (and (nil? (proj/string-value-at idx te "committed")) (and (not (proj/terminal-i? idx te)) (has-structure? idx te)))) (proj/thread-subjects idx)))
