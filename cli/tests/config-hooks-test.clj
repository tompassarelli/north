(ns north.config-hooks-test
  (:require [babashka.process :as p]
            [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.nio.file Files]))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(def tmp (.toFile (Files/createTempDirectory "north-config-hooks-" (make-array java.nio.file.attribute.FileAttribute 0))))

(def ^String home (str tmp "/home"))

(def ^String state (str tmp "/agents"))

(def configured-roots (some-> (System/getenv "NORTH_REPO_ROOTS") json/parse-string))

(def ^String roots (json/generate-string (merge {"north" root "beagle" "/home/tom/code/beagle/main" "agent-machinery" "/home/tom/code/agent-machinery/main" "nixos-config" "/home/tom/code/nixos-config/main"} configured-roots {"north" root})))

(def env {"HOME" home "NORTH_HOME" root "NORTH_AGENT_STATE_ROOT" state "NORTH_REPO_ROOTS" roots})

(def checks (atom []))

(defn check! [^String label value]
  (do
  (swap! checks conj [label (boolean value)])
  nil))

(defn run [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (apply p/shell {:out :string :err :string :continue true :extra-env env} (into ["bb" (str root "/cli/config-cli.clj")] args))))

(try
  (.mkdirs (io/file home))
  (check! "sync succeeds" (zero? (:exit (run "agents" "sync"))))
  (let [hooks (run "hooks")
   skills (run "skills")
   modules (run "modules")
   guards (run "guards")
   filtered (run "agents" "hooks" "--json")]
  (check! "kind and guard views remain read-only catalog projections" (and (every? (fn [%1] (zero? (:exit %1))) [hooks skills modules guards filtered]) (str/includes? (:out hooks) "firn-system-policy") (str/includes? (:out skills) "webdev") (str/includes? (:out modules) "coordination") (str/includes? (:out guards) "tripwire-guard") (every? (fn [%1] (= "hook" (get %1 "kind"))) (get (json/parse-string (:out filtered)) "units")))))
  (let [disabled (run "agents" "off" "tripwire-guard")
   inspected (run "agents" "inspect" "tripwire-guard" "--json")
   unit (json/parse-string (:out inspected))]
  (check! "the agents UnitId ABI is the permission writer" (and (zero? (:exit disabled)) (zero? (:exit inspected)) (= "off" (get unit "permission")) (false? (get unit "active")))))
  (let [legacy-results (mapv (fn [%1] (apply run %1)) [["hooks" "off" "tripwire-guard"] ["hooks" "category" "off" "authoring"] ["hooks" "all" "off"] ["skills" "off" "webdev"] ["skills" "category" "off" "webdev"] ["skills" "all" "off"] ["skills" "sync"] ["modules" "off" "coordination"] ["guards" "off"] ["agents" "hooks" "off" "tripwire-guard"] ["agents" "skills" "off" "webdev"] ["agents" "modules" "off" "coordination"]])]
  (check! "kind, category, all, guard, and sync mutation aliases are absent" (every? (fn [%1] (not (zero? (:exit %1)))) legacy-results)))
  (check! "legacy harness permission state is not written" (not (.exists (io/file home ".local/state/north/harness.conf"))))
  (finally
    (doseq [file (reverse (file-seq tmp))]
  (io/delete-file file true))))

(doseq [[label ok] (deref checks)]
  (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))

(let [passed (count (filter second (deref checks)))]
  (println (format "\nconfig hooks: %d / %d PASS" passed (count (deref checks))))
  (System/exit (if (= passed (count (deref checks))) 0 1)))
