#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/north-map.clj"))
(def gaffer (str (.getParent (io/file root)) "/gaffer"))
(def checks (atom []))
(defn check [label value] (swap! checks conj [label (boolean value)]))
(defn run [role]
  (proc/shell {:out :string :err :string :continue true
               :extra-env {"GAFFER_HOME" gaffer "AGENT_TOPOLOGY" "orchestrator"}}
              "bb" cli "59999" "map" role "1" "probe"))

(let [director (run "director") unknown (run "made-up")]
  (check "orchestrator role is rejected before batch registration"
         (and (not (zero? (:exit director)))
              (str/includes? (:err director) "terminal worker preset")
              (not (str/includes? (str (:out director) (:err director)) "Connection refused"))))
  (check "unknown role is rejected before batch registration"
         (and (not (zero? (:exit unknown)))
              (str/includes? (:err unknown) "unknown Gaffer worker preset")
              (not (str/includes? (str (:out unknown) (:err unknown)) "Connection refused")))))

(let [results @checks passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nmap contract: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
