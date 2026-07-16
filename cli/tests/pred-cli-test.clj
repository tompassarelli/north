#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def vocab (slurp (io/file root "cli/pred-cli.clj")))
(def launcher (slurp (io/file root "bin/north")))
(def predicates ["provider_target" "requested_target" "fallback_target_path"])
(def checks (atom []))
(defn check [label value] (swap! checks conj [label (boolean value)]))

(doseq [predicate predicates]
  (check (str predicate " is single-valued in the registry")
         (str/includes? vocab (str "[\"" predicate "\" \"single\" \"literal\"")))
  (check (str predicate " is single-valued in the legacy fallback")
         (re-find (re-pattern (str "FRAM_SINGLE_VALUED=.*\\b" predicate "\\b")) launcher)))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\ntarget predicate cardinality: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
