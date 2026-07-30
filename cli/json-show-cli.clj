#!/usr/bin/env bb
;; Exact indexed JSON subject projection.
;;
;; `north json show` is a pre-side-effect admission read. Keep it on the
;; coordinator's :show index instead of loading the whole corpus through
;; north.main merely to discard every other subject.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io])

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def show-envelope (ns-resolve 'north.coord 'show-envelope))

(defn- coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

(defn exact-facts
  [port id]
  (let [rows (:rows (show-envelope port (str "@" id)))]
    (mapv (fn [[predicate value]]
            {:predicate predicate :value value})
          rows)))

(defn- refuse! [id error]
  (binding [*out* *err*]
    (println
     (str "north: json show REFUSED — coordinator unavailable for @"
          id
          (when-let [detail (some-> error .getMessage not-empty)]
            (str ": " detail)))))
  (System/exit 4))

(defn -main [& args]
  (if (= 1 (count args))
    (try
      (println (json/generate-string
                (exact-facts (coordination-port) (first args))))
      (catch Exception error
        (refuse! (first args) error)))
    (do
      (binding [*out* *err*]
        (println "usage: json show <id>"))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
