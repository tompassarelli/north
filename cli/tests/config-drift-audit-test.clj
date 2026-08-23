#!/usr/bin/env bb
;; Read-only contract for shared/Codex skill provenance and MCP inventory.
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def tmp-dir (fs/create-temp-dir {:prefix "north-config-audit-test-"}))
(def scratch-home (str tmp-dir "/home"))
(def farm (str tmp-dir "/farm"))
(def codex-home (str tmp-dir "/codex"))
(def codex-system (str tmp-dir "/codex-system-skills"))
(def codex-config (str tmp-dir "/config.toml"))
(def codex-inventory (str tmp-dir "/codex-plugins.json"))
(def plugin-root (str tmp-dir "/plugins/enabled"))
(def disabled-root (str tmp-dir "/plugins/disabled"))
(def checks (atom []))

(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn write-skill-as! [root folder name]
  (let [path (str root "/" folder "/SKILL.md")]
    (io/make-parents path)
    (spit path (str "---\nname: " name "\ndescription: fixture\n---\n"))))

(defn write-skill! [root name]
  (write-skill-as! root name name))

(defn write-plugin! [root]
  (let [manifest (str root "/.codex-plugin/plugin.json")]
    (io/make-parents manifest)
    (spit manifest (json/generate-string {:name (.getName (io/file root))}))))

(defn install-fixture! []
  (.mkdirs (io/file scratch-home))
  (doseq [name ["collision" "shared-only"]]
    (write-skill! farm name))
  (write-skill! codex-system "system-only")

  (write-plugin! plugin-root)
  (write-skill! (str plugin-root "/skills") "collision")
  (write-skill! (str plugin-root "/skills") "plugin-only")
  (write-skill-as! (str plugin-root "/skills") "folder-name" "frontmatter-name")
  (write-plugin! disabled-root)
  (write-skill! (str disabled-root "/skills") "disabled-only")

  (spit codex-inventory
        (json/generate-string
         {:installed
          [{:pluginId "enabled@market" :name "Enabled"
            :marketplaceName "market" :version "1" :installed true :enabled true
            :source {:source "local" :path plugin-root}}
           {:pluginId "disabled@market" :name "Disabled"
            :marketplaceName "market" :version "1" :installed true :enabled false
            :source {:source "local" :path disabled-root}}]
          :available []}))

  (spit codex-config
        (str
         "[mcp_servers.stdio]\ncommand = \"/bin/server\"\nargs = [\"same\"]\n"
         "env_vars = [\"FORWARDED_MCP_TOKEN\"]\n"
         "[mcp_servers.stdio.env]\nTOKEN = \"ENV_SECRET_CANARY\"\n\n"
         "[mcp_servers.http]\nurl = \"https://same.example/mcp\"\n"
         "http_headers = { Authorization = \"HEADER_SECRET_CANARY\" }\n")))

(defn run-cli [& args]
  (apply p/shell
         {:out :string
          :err :string
          :continue true
          :extra-env {"HOME" scratch-home
                      "NORTH_HOME" root
                      "NORTH_AGENT_SKILLS" farm
                      "CODEX_HOME" codex-home
                      "NORTH_CODEX_SYSTEM_SKILLS" codex-system
                      "NORTH_CODEX_CONFIG" codex-config
                      "NORTH_CODEX_PLUGIN_INVENTORY" codex-inventory
                      "FORWARDED_MCP_TOKEN" "FORWARDED_SECRET_VALUE_CANARY"}}
         (into ["bb" cli] args)))

(defn server [report name]
  (some #(when (= name (:name %)) %) (get-in report [:mcp :servers])))

(defn complete-case []
  (let [result (run-cli "audit" "--json")
        report (when (zero? (:exit result)) (json/parse-string (:out result) true))
        skills (get-in report [:skills :entries])
        collision (filter #(= "collision" (:name %)) skills)
        stdio (server report "stdio")
        http (server report "http")]
    (check "audit JSON succeeds" (zero? (:exit result)))
    (check "shared and Codex plugin skills expose collision uncertainty"
           (and (= 2 (count collision))
                (every? :collision collision)
                (every? #(= "uncertain" (:precedence %)) collision)))
    (check "Codex system and plugin skills retain provenance"
           (and (some #(and (= "system-only" (:name %))
                            (= "builtin" (:plugin %))) skills)
                (some #(and (= "plugin-only" (:name %))
                            (= "enabled@market" (:plugin %))) skills)))
    (check "disabled plugin skills are absent"
           (not-any? #(= "disabled-only" (:name %)) skills))
    (check "frontmatter names define invocation identity"
           (and (some #(= "frontmatter-name" (:name %)) skills)
                (not-any? #(= "folder-name" (:name %)) skills)))
    (check "Codex MCP declarations retain structural identity"
           (and (= "complete" (get-in report [:mcp :state]))
                (= #{"stdio" "http"} (set (map :name (get-in report [:mcp :servers]))))
                (= ["FORWARDED_MCP_TOKEN"]
                   (get-in stdio [:normalized :forwardedEnvironmentVariables]))
                (= ["Authorization"] (get-in http [:normalized :headers :keys]))
                (str/starts-with? (get-in http [:normalized :headers :digest]) "sha256:")))
    (check "protected values never appear in machine output"
           (not-any? #(str/includes? (:out result) %)
                     ["ENV_SECRET_CANARY" "HEADER_SECRET_CANARY"
                      "FORWARDED_SECRET_VALUE_CANARY"]))

    (let [mcp-only (run-cli "mcp" "list" "--json")
          human (run-cli "audit")]
      (check "mcp list exposes the same Codex-only machine audit"
             (and (zero? (:exit mcp-only))
                  (nil? (:skills (json/parse-string (:out mcp-only) true)))
                  (= "complete" (get-in (json/parse-string (:out mcp-only) true)
                                         [:mcp :state]))))
      (check "human audit names the Codex MCP inventory"
             (and (zero? (:exit human))
                  (str/includes? (:out human) "CODEX MCP DECLARATIONS")
                  (not (str/includes? (:out human) "HEADER_SECRET_CANARY")))))))

(defn partial-case []
  (spit codex-config "[mcp_servers.broken\n")
  (let [result (run-cli "audit" "--json")
        report (when (zero? (:exit result)) (json/parse-string (:out result) true))]
    (check "malformed Codex config produces a partial audit"
           (and (zero? (:exit result))
                (= "partial" (get-in report [:mcp :state]))
                (some #(and (= "codex" (:provider %)) (= "malformed" (:kind %)))
                      (get-in report [:mcp :diagnostics]))))))

(try
  (install-fixture!)
  (complete-case)
  (partial-case)
  (finally
    (fs/delete-tree tmp-dir)))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nconfig audit: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
