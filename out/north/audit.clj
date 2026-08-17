(ns north.audit
  (:require [north.projections :as proj]
            [clojure.string :as str]))

(defrecord DriftGroup [norm forms])

(defn driftgroup-norm [r] (:norm r))

(defn driftgroup-forms [r] (:forms r))

(defn tally [idx ^String pred]
  (reduce (fn [m ^String te] (reduce (fn [mm ^String v] (assoc mm v (+ 1 (int (get mm v 0))))) m (proj/string-values-at idx te pred))) {} (proj/thread-subjects idx)))

(defn- ^String norm-repo [^String v]
  (let [low (str/lower-case v)]
  (if (str/starts-with? low "~/code/") (subs low 7) low)))

(defn- collisions [forms grouped]
  (filterv (fn [^DriftGroup g] (> (count (:forms g)) 1)) (mapv (fn [^String kk] (->DriftGroup kk (get grouped kk []))) (vec (sort (set (keys grouped)))))))

(defn repo-drift [idx]
  (let [forms (vec (sort (set (keys (tally idx "repo")))))
   grouped (reduce (fn [m ^String t] (let [kk (norm-repo t)]
  (assoc m kk (conj (get m kk []) t)))) {} forms)]
  (collisions forms grouped)))
