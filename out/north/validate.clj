(ns north.validate
  (:require [store.kernel-classify :as kc]
            [north.projections :as proj]))

(def thread-ref-preds ["part_of" "depends_on" "relates_to"])

(defn- ^Boolean reachable? [idx ^String predicate ^String target frontier]
  (loop [remaining frontier
   seen {}]
  (cond
  (empty? remaining) false
  (= target (first remaining)) true
  (contains? seen (first remaining)) (recur (vec (rest remaining)) seen)
  :else (recur (vec (concat (rest remaining) (proj/string-values-at idx (first remaining) predicate))) (assoc seen (first remaining) true)))))

(defn- ^Boolean cycle? [idx ^String predicate ^String subject]
  (reachable? idx predicate subject (proj/string-values-at idx subject predicate)))

(defn structural-violations-i [idx ^String subject]
  (let [reference-violations (reduce (fn [violations ^String predicate] (reduce (fn [found ^String target] (if (proj/subject? idx target) found (conj found (str predicate " references missing entity " target)))) violations (proj/string-values-at idx subject predicate))) [] (proj/projectionindex-reference-predicates idx))]
  (reduce (fn [violations ^String predicate] (if (cycle? idx predicate subject) (conj violations (str predicate " cycle")) violations)) reference-violations (proj/projectionindex-acyclic-predicates idx))))

(defn work-violations-i [idx ^String te]
  (let [term? (proj/terminal-i? idx te)
   v-thread (reduce (fn [acc ^String p] (reduce (fn [a ^String rt] (cond
  (and (proj/subject? idx rt) (nil? (proj/string-value-at idx rt "title"))) (conj a (str p " references non-thread entity " rt))
  (not (proj/subject? idx rt)) (conj a (str p " references missing thread " rt))
  :else a)) acc (proj/string-values-at idx te p))) [] thread-ref-preds)]
  (do
  (let [v-ab (reduce (fn [acc ^String d] (if (and (not term?) (proj/withdrawn-i? idx d)) (conj acc (str "depends_on points at abandoned " d)) acc)) v-thread (proj/string-values-at idx te "depends_on"))]
  v-ab))))

(defn violations-i [idx ^String te]
  (vec (concat (structural-violations-i idx te) (work-violations-i idx te))))
