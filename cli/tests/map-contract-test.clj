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
(load-file (str root "/cli/batch-id.clj"))
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

(let [now (java.time.LocalDateTime/of 2026 7 19 23 59 59)
      ids (mapv #(north.batch-id/fresh-id
                  now (java.util.UUID. 0 (long %)))
                (range 10000))]
  (check "same-second fixture batches retain 10,000 unique durable subjects"
         (= (count ids) (count (set ids))))
  (check "fixture batch id keeps sortable time plus a full canonical UUID"
         (every?
          #(re-matches
            #"20260719-235959-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
            %)
          ids)))

(let [marker (io/file (System/getProperty "java.io.tmpdir")
                      (str "north-map-spawn-marker-" (java.util.UUID/randomUUID)))
      sentinel (io/file (System/getProperty "java.io.tmpdir")
                        (str "north-map-spawn-sentinel-" (java.util.UUID/randomUUID)))
      _ (spit sentinel (str "#!/bin/sh\nprintf called > " (.getCanonicalPath marker) "\n"))
      _ (.setExecutable sentinel true)
      result
      (proc/shell {:out :string :err :string :continue true
                   :extra-env {"GAFFER_HOME" gaffer
                               "AGENT_TOPOLOGY" "orchestrator"
                               "NORTH_BUN" (.getCanonicalPath sentinel)}}
                  "bb" cli "59999" "map" "verifier" "1" "probe")]
  (check "ambient map fails closed before graph access or a spawn callback"
         (and (= 2 (:exit result))
              (str/includes? (:err result) "lane spawning is retired")
              (not (str/includes? (str (:out result) (:err result)) "Connection refused"))
              (not (.exists marker))))
  (check "legacy direct bb/Bun SDK spawn code is absent"
         (let [source (slurp cli)]
           (and (not (str/includes? source "(sh \"bb\""))
                (not (str/includes? source "sdk/src/spawn.ts")))))
  (.delete sentinel)
  (.delete marker))

(let [results @checks passed (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nmap contract: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
