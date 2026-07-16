#!/usr/bin/env bb
;; Compatibility bridge. Dollar-centric dial scoring was retired in favor of
;; evidence-aware routing feedback.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io])
(def here (.getParent (io/file (System/getProperty "babashka.file"))))
(binding [*out* *err*]
  (println "north dials is superseded by: north routing report performance"))
(let [{:keys [exit]} (proc/shell {:inherit true} "bb" (str here "/routing-report.clj") "report" "performance")]
  (System/exit exit))
