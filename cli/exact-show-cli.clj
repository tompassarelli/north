#!/usr/bin/env bb
;; Human exact-subject renderer.  Keep ordinary `north show <id>` on the
;; coordinator's :show index rather than constructing north.main's full corpus.
(require '[clojure.java.io :as io])
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(defn port [] (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))
(defn -main [id]
  (try
    (let [subject (if (.startsWith id "@") id (str "@" id))]
      (if-let [rows (:rows (north.coord/show-envelope (port) subject))]
        (if (seq rows)
          (doseq [[predicate value] rows] (println (str "  " predicate "  " value)))
          (println (str "no facts for " subject)))))
    (catch Exception error
      (binding [*out* *err*]
        (println (str "north: show REFUSED — coordinator unavailable for @" id
                      (when-let [detail (some-> error .getMessage not-empty)] (str ": " detail)))))
      (System/exit 4))))
(when (= *file* (System/getProperty "babashka.file")) (apply -main *command-line-args*))
