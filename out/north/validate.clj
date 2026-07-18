(ns north.validate
  (:require [fram.kernel :as k]
            [north.projections :as proj]))

(def thread-ref-preds ["part_of" "depends_on" "relates_to"])

(defn work-violations-i [idx ^String te]
  (let [term? (proj/terminal-i? idx te)
   v-thread (reduce (fn [acc p] (reduce (fn [a rt] (cond
  (and (k/entity-i? idx rt) (nil? (k/one-i idx rt "title"))) (conj a (str p " references non-thread entity " rt))
  (and (not (k/entity-i? idx rt)) (not (k/vec-contains? (:ref-preds idx) p))) (conj a (str p " references missing thread " rt))
  :else a)) acc (k/many-i idx te p))) [] thread-ref-preds)
   v-ab (reduce (fn [acc d] (if (and (not term?) (proj/withdrawn-i? idx d)) (conj acc (str "depends_on points at abandoned " d)) acc)) v-thread (k/many-i idx te "depends_on"))
   v-p (reduce (fn [acc p] (let [ref (k/one-i idx te p)]
  (if (and (some? ref) (nil? (k/one-i idx ref "display_name"))) (conj acc (str p " references unknown person " ref)) acc))) v-ab ["lead" "driver"])
   v-pb (reduce (fn [acc rt] (if (nil? (k/one-i idx rt "display_name")) (conj acc (str "proposed_by references unknown person " rt)) acc)) v-p (k/many-i idx te "proposed_by"))]
  v-pb))

(defn violations-i [idx ^String te]
  (vec (concat (k/violations-i idx te) (work-violations-i idx te))))
