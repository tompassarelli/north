#!/usr/bin/env bb
;; Read-only contract for provider/plugin skill provenance and MCP drift.
(require '[babashka.fs :as fs]
         '[babashka.process :as p]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def tmp-dir (fs/create-temp-dir {:prefix "north-config-drift-audit-test-"}))
(def scratch-home (str tmp-dir "/home"))
(def farm (str tmp-dir "/farm"))
(def claude-settings (str tmp-dir "/claude-settings.json"))
(def claude-ledger (str tmp-dir "/installed-plugins.json"))
(def claude-mcp (str tmp-dir "/claude.json"))
(def codex-home (str tmp-dir "/codex"))
(def codex-system (str tmp-dir "/codex-system-skills"))
(def codex-config (str tmp-dir "/config.toml"))
(def codex-inventory (str tmp-dir "/codex-plugins.json"))
(def checks (atom []))

(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn write-skill! [root name]
  (let [path (str root "/" name "/SKILL.md")]
    (io/make-parents path)
    (spit path (str "---\nname: " name "\ndescription: fixture\n---\n"))))

(defn write-plugin! [root provider]
  (let [manifest (str root "/."
                      (if (= provider "claude") "claude" "codex")
                      "-plugin/plugin.json")]
    (io/make-parents manifest)
    (spit manifest (json/generate-string {:name (.getName (io/file root))}))))

(def enabled-root (str tmp-dir "/plugins/enabled"))
(def disabled-root (str tmp-dir "/plugins/disabled"))
(def malformed-root (str tmp-dir "/plugins/malformed"))
(def escape-root (str tmp-dir "/plugins/escape"))
(def codex-plugin-root (str tmp-dir "/plugins/codex"))
(def escaped-target (str tmp-dir "/outside/escaped"))

(defn install-fixture! []
  (.mkdirs (io/file scratch-home))
  (doseq [name ["collision" "shared-only"]] (write-skill! farm name))
  (write-skill! codex-system "system-only")

  (write-plugin! enabled-root "claude")
  (write-skill! (str enabled-root "/skills") "collision")
  (write-skill! (str enabled-root "/skills") "plugin-only")

  (write-plugin! disabled-root "claude")
  (write-skill! (str disabled-root "/skills") "collision")
  (write-skill! (str disabled-root "/skills") "disabled-only")

  (let [manifest (str malformed-root "/.claude-plugin/plugin.json")]
    (io/make-parents manifest)
    (spit manifest "{\"name\":")
    (write-skill! (str malformed-root "/skills") "malformed-skill"))

  (write-plugin! escape-root "claude")
  (write-skill! (str escape-root "/skills") "inside")
  (write-skill! (str tmp-dir "/outside") "escaped")
  (java.nio.file.Files/createSymbolicLink
   (.toPath (io/file escape-root "skills/escaped"))
   (.toPath (io/file escaped-target))
   (make-array java.nio.file.attribute.FileAttribute 0))

  (write-plugin! codex-plugin-root "codex")
  (write-skill! (str codex-plugin-root "/skills") "codex-plugin-only")

  (spit claude-settings
        (json/generate-string
         {:enabledPlugins {"enabled@market" true
                           "disabled@market" false
                           "malformed@market" true
                           "escape@market" true}}))
  (spit claude-ledger
        (json/generate-string
         {:plugins
          {"enabled@market" [{:installPath enabled-root :scope "user"}]
           "disabled@market" [{:installPath disabled-root :scope "user"}]
           "malformed@market" [{:installPath malformed-root :scope "user"}]
           "escape@market" [{:installPath escape-root :scope "user"}]}}))
  (spit codex-inventory
        (json/generate-string
         {:installed
          [{:pluginId "codex-plugin@market" :name "Codex fixture"
            :marketplaceName "market" :version "1" :installed true :enabled true
            :source {:source "local" :path codex-plugin-root}}
           {:pluginId "disabled-codex@market" :name "Disabled"
            :marketplaceName "market" :version "1" :installed true :enabled false
            :source {:source "local" :path disabled-root}}]
          :available []}))

  (spit claude-mcp
        (json/generate-string
         {:mcpServers
          {"aligned-stdio" {:type "stdio" :command "/bin/server"
                            :args ["--mode" "same"] :cwd "/work"
                            :env {:TOKEN "ENV_SECRET_CANARY"}}
           "drift-stdio" {:command "/bin/server" :args ["left"]}
           "aligned-http" {:type "http" :url "https://same.example/mcp"
                           :headers {:Authorization "HEADER_SECRET_CANARY"}}
           "drift-http" {:type "http" :url "https://left.example/mcp"
                         :headers {:Authorization "LEFT_HEADER_CANARY"}}
           "claude-alias" {:command "/bin/alias" :args ["same"]}
           "claude-only" {:command "/bin/claude-only"}}}))
  (spit codex-config
        (str
         "[mcp_servers.\"aligned-stdio\"]\n"
         "command = \"/bin/server\"\nargs = [\"--mode\", \"same\"]\ncwd = \"/work\"\n"
         "[mcp_servers.\"aligned-stdio\".env]\nTOKEN = \"ENV_SECRET_CANARY\"\n\n"
         "[mcp_servers.\"drift-stdio\"]\ncommand = \"/bin/server\"\nargs = [\"right\"]\n\n"
         "[mcp_servers.\"aligned-http\"]\nurl = \"https://same.example/mcp\"\n"
         "http_headers = { Authorization = \"HEADER_SECRET_CANARY\" }\n\n"
         "[mcp_servers.\"drift-http\"]\nurl = \"https://right.example/mcp\"\n"
         "http_headers = { Authorization = \"RIGHT_HEADER_CANARY\" }\n\n"
         "[mcp_servers.\"codex-alias\"]\ncommand = \"/bin/alias\"\nargs = [\"same\"]\n\n"
         "[mcp_servers.\"codex-only\"]\ncommand = \"/bin/codex-only\"\n")))

(defn run-cli [& args]
  (apply p/shell
         {:out :string
          :err :string
          :continue true
          :extra-env {"HOME" scratch-home
                      "NORTH_HOME" root
                      "NORTH_SKILLS_FARM" farm
                      "NORTH_CLAUDE_SETTINGS" claude-settings
                      "NORTH_CLAUDE_PLUGIN_LEDGER" claude-ledger
                      "NORTH_CLAUDE_MCP_CONFIG" claude-mcp
                      "CODEX_HOME" codex-home
                      "NORTH_CODEX_SYSTEM_SKILLS" codex-system
                      "NORTH_CODEX_CONFIG" codex-config
                      "NORTH_CODEX_PLUGIN_INVENTORY" codex-inventory}}
         (into ["bb" cli] args)))

(defn comparison [report name]
  (some #(when (= name (:name %)) %) (get-in report [:mcp :comparisons])))

(defn complete-case []
  (let [result (run-cli "audit" "--json")
        report (when (zero? (:exit result)) (json/parse-string (:out result) true))
        skills (get-in report [:skills :entries])
        collision (some #(when (= "collision" (:name %)) %) skills)]
    (check "audit JSON succeeds" (zero? (:exit result)))
    (check "enabled plugin collides with the shared farm and precedence stays uncertain"
           (and (:collision collision)
                (= "uncertain" (:precedence collision))
                (= 2 (count (filter #(= "collision" (:name %)) skills)))))
    (check "disabled plugin skills are not effective inventory"
           (not-any? #(= "disabled-only" (:name %)) skills))
    (check "distinct provider and plugin skills retain provenance and canonical paths"
           (every?
            identity
            [(some #(and (= "system-only" (:name %))
                         (= "codex" (:provider %)) (= "builtin" (:plugin %))
                         (.isAbsolute (io/file (:path %)))) skills)
             (some #(and (= "plugin-only" (:name %))
                         (= "enabled@market" (:plugin %)) (= "user" (:scope %))
                         (not (:collision %))) skills)
             (some #(and (= "codex-plugin-only" (:name %))
                         (= "codex-plugin@market" (:plugin %))) skills)]))
    (check "escaping symlink is excluded with a containment diagnostic"
           (and (not-any? #(= "escaped" (:name %)) skills)
                (some #(= "containment" (:kind %))
                      (get-in report [:skills :diagnostics]))))
    (check "malformed plugin manifest preserves useful partial inventory"
           (and (= "partial" (get-in report [:skills :state]))
                (some #(and (= "malformed" (:kind %))
                            (str/includes? (:source %) "malformed"))
                      (get-in report [:skills :diagnostics]))
                (some #(= "plugin-only" (:name %)) skills)))

    (check "aligned stdio and HTTP declarations are recognized"
           (every? #(= "aligned" (:state (comparison report %)))
                   ["aligned-stdio" "aligned-http"]))
    (check "same-name stdio and HTTP drift names their structural fields"
           (and (= "same-name-drift" (:state (comparison report "drift-stdio")))
                (some #{"arguments"} (:differences (comparison report "drift-stdio")))
                (= "same-name-drift" (:state (comparison report "drift-http")))
                (some #{"endpoint"} (:differences (comparison report "drift-http")))))
    (check "equivalent differently named declarations are reported as aliases"
           (some #(and (= "claude-alias" (get-in % [:claude :name]))
                       (= "codex-alias" (get-in % [:codex :name])))
                 (get-in report [:mcp :aliases])))
    (check "one-provider declarations remain explicit"
           (and (= "claude-only" (:state (comparison report "claude-only")))
                (= "codex-only" (:state (comparison report "codex-only")))))
    (check "environment and header values never appear in machine output"
           (not-any? #(str/includes? (:out result) %)
                     ["ENV_SECRET_CANARY" "HEADER_SECRET_CANARY"
                      "LEFT_HEADER_CANARY" "RIGHT_HEADER_CANARY"]))
    (check "protected values retain structural keys and digests"
           (let [server (some #(when (and (= "claude" (:provider %))
                                          (= "aligned-http" (:name %))) %)
                              (get-in report [:mcp :servers]))]
             (and (= ["Authorization"] (get-in server [:normalized :headers :keys]))
                  (str/starts-with? (get-in server [:normalized :headers :digest]) "sha256:"))))

    (let [mcp-only (run-cli "mcp" "list" "--json")
          skills-list (run-cli "skills" "list")
          human (run-cli "audit")]
      (check "mcp list exposes the same machine-readable audit"
             (and (zero? (:exit mcp-only))
                  (nil? (:skills (json/parse-string (:out mcp-only) true)))
                  (= "complete" (get-in (json/parse-string (:out mcp-only) true)
                                         [:mcp :state]))))
      (check "skills list appends effective external collision provenance"
             (and (zero? (:exit skills-list))
                  (str/includes? (:out skills-list) "EFFECTIVE SKILL PROVENANCE")
                  (re-find #"(?m)^  collision  .*COLLISION.*precedence=uncertain"
                           (:out skills-list))))
      (check "human audit also withholds protected values"
             (and (zero? (:exit human))
                  (not-any? #(str/includes? (:out human) %)
                            ["ENV_SECRET_CANARY" "HEADER_SECRET_CANARY"
                             "LEFT_HEADER_CANARY" "RIGHT_HEADER_CANARY"]))))))

(defn partial-case []
  (spit claude-mcp "{\"mcpServers\":")
  (let [result (run-cli "audit" "--json")
        report (when (zero? (:exit result)) (json/parse-string (:out result) true))]
    (check "malformed provider state does not suppress the readable provider"
           (and (zero? (:exit result))
                (= "partial" (get-in report [:mcp :state]))
                (some #(= "codex-only" (:name %)) (get-in report [:mcp :servers]))
                (= "uncertain" (:certainty (comparison report "codex-only")))))
    (check "partial state names the malformed source without echoing its contents"
           (some #(and (= "claude" (:provider %)) (= "malformed" (:kind %)))
                 (get-in report [:mcp :diagnostics])))))

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
  (println (format "\nconfig drift audit: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
