(ns north.map-contract-test
  (:require [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.time LocalDateTime]
           [java.util UUID]))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(def ^String cli (str root "/cli/north-map.clj"))

(def ^String store (or (System/getenv "BEAGLE_STORE_HOME") (.getCanonicalPath (io/file root ".." ".." "store" "main"))))

(def ^String store-out (str store "/out"))

(def ^String agent-machinery (str root "/agent-machinery"))

(def ^String agent-runtime (or (System/getenv "NORTH_AGENT_RUNTIME_HOME") (str root "/agent-runtime/orchestration")))

(defrecord Check [label passed])

(defn check-label [r] (:label r))

(defn check-passed [r] (:passed r))

(def checks (atom []))

(defn check! [^String label value]
  (do
  (swap! checks conj (->Check label (boolean value)))
  nil))

(load-file (str root "/cli/batch-id.clj"))

(defn run [^String role]
  (proc/shell {:out :string :err :string :continue true :extra-env {"AGENT_MACHINERY_HOME" agent-machinery "NORTH_AGENT_RUNTIME_HOME" agent-runtime "AGENT_TOPOLOGY" "orchestrator"}} "bb" "-cp" store-out cli "59999" "map" role "1" "probe"))

(let [director (run "director")
   unknown (run "made-up")]
  (check! "orchestrator role is rejected before batch registration" (and (not (zero? (:exit director))) (str/includes? (:err director) "terminal worker preset") (not (str/includes? (str (:out director) (:err director)) "Connection refused"))))
  (check! "unknown role is rejected before batch registration" (and (not (zero? (:exit unknown))) (str/includes? (:err unknown) "unknown Orchestration worker preset") (not (str/includes? (str (:out unknown) (:err unknown)) "Connection refused")))))

(let [now (LocalDateTime/of 2026 7 19 23 59 59)
   ids (mapv (fn [index] (north.batch-id/fresh-id now (UUID. 0 (long index)))) (range 10000))]
  (check! "same-second fixture batches retain 10,000 unique durable subjects" (= (count ids) (count (set ids))))
  (check! "fixture batch id keeps sortable time plus a full canonical UUID" (every? (fn [^String id] (boolean (re-matches #"20260719-235959-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}" id))) ids)))

(let [marker (io/file (System/getProperty "java.io.tmpdir") (str "north-map-spawn-marker-" (UUID/randomUUID)))
   sentinel (io/file (System/getProperty "java.io.tmpdir") (str "north-map-spawn-sentinel-" (UUID/randomUUID)))
   _ (spit sentinel (str "#!/bin/sh\nprintf called > " (.getCanonicalPath marker) "\n"))
   _ (.setExecutable sentinel true)
   result (proc/shell {:out :string :err :string :continue true :extra-env {"AGENT_MACHINERY_HOME" agent-machinery "NORTH_AGENT_RUNTIME_HOME" agent-runtime "AGENT_TOPOLOGY" "orchestrator" "NORTH_BUN" (.getCanonicalPath sentinel)}} "bb" "-cp" store-out cli "59999" "map" "verifier" "1" "probe")]
  (check! "ambient map fails closed before graph access or a spawn callback" (and (= 2 (:exit result)) (str/includes? (:err result) "lane spawning is retired") (not (str/includes? (str (:out result) (:err result)) "Connection refused")) (not (.exists marker))))
  (check! "direct bb/Bun SDK spawn code is absent" (let [source (slurp cli)]
  (and (not (str/includes? source "(sh \"bb\"")) (not (str/includes? source "sdk/src/spawn.ts")))))
  (.delete sentinel)
  (.delete marker))

(let [results (deref checks)
   passed (count (filter (fn [^Check result] (check-passed result)) results))]
  (doseq [result results]
  (println (format "  [%s] %s" (if (check-passed result) "PASS" "FAIL") (check-label result))))
  (println (format "\nmap contract: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
