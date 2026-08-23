#!/usr/bin/env bb
(require '[babashka.process :as p]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def tmp (.toFile (java.nio.file.Files/createTempDirectory
                   "north-config-hooks-" (make-array java.nio.file.attribute.FileAttribute 0))))
(def home (str tmp "/home"))
(def state (str tmp "/agents"))
(def roots (json/generate-string
            {"north" root "beagle" "/home/tom/code/beagle/main"
             "nixos-config" "/home/tom/code/nixos-config/main"}))
(def env {"HOME" home "NORTH_HOME" root "NORTH_AGENT_STATE_ROOT" state
          "NORTH_REPO_ROOTS" roots})
(def checks (atom []))
(defn check [label value] (swap! checks conj [label (boolean value)]))
(defn run [& args]
  (apply p/shell {:out :string :err :string :continue true :extra-env env}
         (into ["bb" (str root "/cli/config-cli.clj")] args)))

(try
  (.mkdirs (io/file home))
  (check "sync succeeds" (zero? (:exit (run "agents" "sync"))))
  (let [listed (run "hooks")]
    (check "hook view comes from the catalog"
           (and (zero? (:exit listed))
                (str/includes? (:out listed) "firn-system-policy"))))
  (let [disabled (run "hooks" "off" "tripwire-guard" "--until" "2099-01-01T00:00:00Z")
        inspected (run "agents" "inspect" "tripwire-guard" "--json")
        unit (json/parse-string (:out inspected))]
    (check "hook mutation writes the one UnitId permission"
           (and (zero? (:exit disabled)) (zero? (:exit inspected))
                (= "off:until=2099-01-01T00:00:00Z" (get unit "permission"))
                (false? (get unit "active")))))
  (let [guards (run "guards" "off")
        status (json/parse-string (:out (run "agents" "status" "--json")))
        authoring (filter #(and (= "hook" (get % "kind"))
                                (= "authoring" (get % "category")))
                          (get status "units"))]
    (check "guards is a thin authoring-hook batch client"
           (and (zero? (:exit guards)) (seq authoring)
                (every? #(= "off" (get % "permission")) authoring))))
  (check "legacy harness permission state is not written"
         (not (.exists (io/file home ".local/state/north/harness.conf"))))
  (finally
    (doseq [file (reverse (file-seq tmp))] (io/delete-file file true))))

(doseq [[label ok] @checks]
  (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
(let [passed (count (filter second @checks))]
  (println (format "\nconfig hooks: %d / %d PASS" passed (count @checks)))
  (System/exit (if (= passed (count @checks)) 0 1)))
