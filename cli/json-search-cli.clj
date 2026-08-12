#!/usr/bin/env bb
;; Literal search over the canonical live Fram projection.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file
 (str (.getParent (io/file *file*)) "/coord.clj"))

(defn- coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

(defn- short-id [subject]
  (if (str/starts-with? subject "@") (subs subject 1) subject))

(defn- contains-literal? [haystack needle]
  (str/includes? (str/lower-case haystack) needle))

(defn- matching-row? [[_subject predicate value] query]
  (or (contains-literal? predicate query)
      (contains-literal? value query)))

(defn- subject-title [rows]
  (or (some (fn [[_subject predicate value]]
              (when (= predicate "title") value))
            rows)
      ""))

(defn- round-robin [groups]
  (loop [remaining (vec groups)
         results []]
    (if (empty? remaining)
      results
      (let [heads (mapv first remaining)
            tails (filterv seq (mapv #(vec (rest %)) remaining))]
        (recur tails (into results heads))))))

(defn search-rows [facts query]
  (let [needle (str/lower-case query)
        by-subject (group-by first facts)
        groups
        (keep
         (fn [subject]
           (let [rows (get by-subject subject)
                 title (subject-title rows)
                 matches (sort-by (fn [[_ predicate value]] [predicate value])
                                  (filter #(matching-row? % needle) rows))]
             (when (seq matches)
               (mapv (fn [[_ predicate value]]
                       {:subject (short-id subject)
                        :title title
                        :predicate predicate
                        :value value})
                     matches))))
         (sort (keys by-subject)))]
    (round-robin groups)))

(defn search-results [port query]
  (let [view (north.coord/live-facts-view port)]
    (when-not (:complete view)
      (throw (ex-info "FRAMRPC live projection is incomplete"
                      {:type :incomplete-live-projection
                       :unavailable (:unavailable view)})))
    (search-rows (:facts view) query)))

(defn- refuse! [query error]
  (binding [*out* *err*]
    (println
     (str "north: json search REFUSED — coordinator unavailable for "
          (pr-str query)
          (when-let [detail (some-> error .getMessage not-empty)]
            (str ": " detail)))))
  (System/exit 4))

(defn -main [& args]
  (if (and (= 1 (count args)) (not (str/blank? (first args))))
    (try
      (println (json/generate-string
                (search-results (coordination-port) (first args))))
      (catch Exception error
        (refuse! (first args) error)))
    (do
      (binding [*out* *err*]
        (println "usage: json search <literal>"))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
