#!/usr/bin/env bb
(require '[cheshire.core :as json]
         '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/agent-catalog.clj"))

(def checks (atom []))
(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn throws-containing? [text f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (str/includes? (.getMessage error) text))))

(def tmp
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-agent-catalog-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))

(def catalog-path (str root "/agent-catalog/catalog.json"))
(def base (json/parse-string (slurp catalog-path)))
(def catalog-schema-value
  (json/parse-string (slurp (str root "/agent-catalog/catalog.schema.json"))))
(def activation-schema-value
  (json/parse-string (slurp (str root "/agent-catalog/activation.schema.json"))))
(def catalog-unit-fields
  #{"id" "kind" "title" "triggerDescription" "category" "seedPermission"
    "owner" "members" "supports" "distributions"})
(def resolved-unit-fields
  #{"id" "kind" "title" "triggerDescription" "category" "permission" "active"
    "owner" "ownerProvenance" "members" "supports" "distributions"
    "activationPaths"})

(defn load-value [value]
  (let [path (str tmp "/fixture-" (java.util.UUID/randomUUID) ".json")]
    (spit path (json/generate-string value))
    (with-redefs [north.agent-catalog/catalog-path (constantly path)]
      (north.agent-catalog/load-catalog))))

(defn loads? [value]
  (try
    (load-value value)
    true
    (catch clojure.lang.ExceptionInfo _ false)))

(defn mutate-unit [catalog id f]
  (update catalog "units"
          (fn [units]
            (mapv #(if (= id (get % "id")) (f %) %) units))))

(def lifecycle-hook-ids
  ["north-on-spawn" "north-on-tooluse" "north-on-stop"
   "north-on-terminal" "north-mark-delegated"])

(defn executable-on-path [name]
  (or (some (fn [directory]
              (let [candidate (io/file directory name)]
                (when (and (.isFile candidate) (.canExecute candidate))
                  (.toRealPath (.toPath candidate)
                               (make-array java.nio.file.LinkOption 0)))))
            (str/split (or (System/getenv "PATH") "")
                       (re-pattern (java.util.regex.Pattern/quote
                                    java.io.File/pathSeparator))))
      (throw (ex-info (str "missing executable on PATH: " name) {:name name}))))

(defn write-executable! [path content]
  (let [file (io/file path)]
    (.mkdirs (.getParentFile file))
    (spit file content)
    (when-not (.setExecutable file true false)
      (throw (ex-info (str "cannot make fixture executable: " file)
                      {:path (str file)})))
    file))

(def project-set-fixture
  {"id" "beagle-tools"
   "kind" "set"
   "title" "Beagle Tools"
   "triggerDescription" "Project-packaged Beagle authoring behavior."
   "seedPermission" "off"
   "owner" {"repo" "north" "path" "agent-catalog/catalog.json"}
   "members" ["code-as-facts"]
   "distributions" [{"type" "projectPackage"
                     "targets" ["project:beagle"]
                     "owner" {"repo" "north" "path" "agent-catalog/catalog.json"}}]})

(defn delete-scratch! [path]
  ;; Files.walk does not follow projection symlinks without FOLLOW_LINKS.
  (when (.exists (io/file path))
    (with-open [stream (java.nio.file.Files/walk
                        (.toPath (io/file path))
                        (make-array java.nio.file.FileVisitOption 0))]
      (doseq [entry (reverse (iterator-seq (.iterator stream)))]
        (java.nio.file.Files/deleteIfExists entry)))))

(try
  (let [catalog (north.agent-catalog/load-catalog)
        activation (north.agent-catalog/compile-activation
                    catalog (north.agent-catalog/seed-permissions catalog))
        by-id (into {} (map (juxt #(get % "id") identity)) (get activation "units"))]
    (check "catalog inventories every audited skill, hook, and set"
           (and (= 65 (count (:units catalog)))
                (every? (:by-id catalog)
                        (concat ["coordination" "orchestration" "compose" "elicit"
                                 "store-modeling" "code-as-facts"
                                 "code-upstream-guard"]
                                lifecycle-hook-ids))))
    (check "generation exposes the stable root contract"
           (and (= "north.agent-activation/v1" (get activation "schema"))
                (= "north.agent-catalog/v1" (get activation "catalogSchema"))
                (re-matches #"sha256:[0-9a-f]{64}" (get activation "catalogDigest"))
                (re-matches #"sha256:[0-9a-f]{64}" (get activation "generationId"))
                (= (set (map #(get % "id") (:units catalog)))
                   (set (keys (get activation "permissions"))))))
    (check "unit schemas and records expose only the global UnitId contract"
           (and (= catalog-unit-fields
                   (set (keys (get-in catalog-schema-value
                                      ["$defs" "unit" "properties"]))))
                (= resolved-unit-fields
                   (set (keys (get-in activation-schema-value
                                      ["$defs" "unit" "properties"]))))
                (every? #(every? catalog-unit-fields (keys %)) (get base "units"))
                (every? #(every? resolved-unit-fields (keys %))
                        (get activation "units"))))
    (check "generation identity commits deterministic owner content and revision provenance"
           (and (= (get activation "generationId")
                   (get (north.agent-catalog/compile-activation
                         catalog (north.agent-catalog/seed-permissions catalog))
                        "generationId"))
                (every? #(and (re-matches #"sha256:[0-9a-f]{64}"
                                          (get-in % ["ownerProvenance" "contentDigest"]))
                               (re-matches #"[0-9a-f]{40,64}"
                                           (get-in % ["ownerProvenance" "revision"])))
                        (get activation "units"))))
    (check "every resolved unit carries stored permission and all consumer fields"
           (every?
            #(every? (set (keys %))
                     ["id" "kind" "title" "triggerDescription" "permission"
                      "active" "owner" "members" "supports" "distributions"
                      "activationPaths"])
            (get activation "units")))
    (check "sets expand depth-first in declared member order"
           (= ["orchestration" "staffing" "compose" "elicit" "coordination"
               "messages" "threads" "assignments"]
              (mapv #(get % "id") (take 8 (get activation "units")))))
    (check "supported hooks retain every activation path after unit dedupe"
           (= [["orchestration" "session-kill-guard"]
               ["repo-safety" "session-kill-guard"]]
              (get-in by-id ["session-kill-guard" "activationPaths"])))
    (check "Codex lifecycle hooks have five exact independent projection outputs"
           (let [owner-for (fn [id]
                             {"repo" "nixos-config"
                              "path" (str "dotfiles/codex/hooks/" id "-codex")})
                 entries (->> (get-in activation
                                      ["projectionPlan" "providerAdapter" "codex"])
                              (filter #(some #{(get % "unitId")} lifecycle-hook-ids))
                              (group-by #(get % "unitId")))]
             (and (= (set lifecycle-hook-ids) (set (keys entries)))
                  (every?
                   (fn [id]
                     (let [[entry :as matches] (get entries id)
                           owner (owner-for id)]
                       (and (= 1 (count matches))
                            (= #{"unitId" "owner" "adapterId" "provenance"}
                               (set (keys entry)))
                            (= id (get entry "unitId"))
                            (= (str id "-codex") (get entry "adapterId"))
                            (= owner (get entry "owner"))
                            (= owner (get-in entry ["provenance" "owner"]))
                            (re-matches #"[0-9a-f]{40,64}"
                                        (get-in entry ["provenance" "revision"]))
                            (re-matches #"sha256:[0-9a-f]{64}"
                                        (get-in entry ["provenance" "contentDigest"])))))
                   lifecycle-hook-ids))))
    (check "retired lifecycle umbrella identity is absent"
           (let [retired-id (str "north-session-" "lifecycle")]
             (and (nil? (get (:by-id catalog) retired-id))
                  (nil? (get by-id retired-id))
                  (not (contains? (get activation "permissions") retired-id))
                  (not-any? #(= retired-id (get % "unitId"))
                            (for [[_ targets] (get activation "projectionPlan")
                                  [_ entries] targets
                                  entry entries]
                              entry)))))
    (check "project-packaged units are global IDs that seed inert"
           (every? #(and (= "off" (get-in by-id [% "permission"]))
                         (false? (get-in by-id [% "active"]))
                         (= [["project:beagle"]]
                            (mapv (fn [distribution] (get distribution "targets"))
                                  (get-in by-id [% "distributions"]))))
                   ["code-as-facts" "code-upstream-guard"]))
    (check "catalog category agrees with source frontmatter when declared"
           (= "nixos" (get-in by-id ["firn" "category"]))))

  (let [catalog (north.agent-catalog/load-catalog)
        catalog-ids (set (map #(get % "id") (:units catalog)))
        permissions
        (with-redefs [north.agent-catalog/current-activation
                      (constantly
                       {"permissions" {"webdev" "off"
                                       "retired-unit" "on"}})]
          (north.agent-catalog/current-permissions catalog))]
    (check "catalog upgrade retains an existing permission override"
           (= "off" (get permissions "webdev")))
    (check "catalog upgrade defaults a newly introduced UnitId off"
           (= "off" (get permissions "importing-skills")))
    (check "catalog upgrade prunes removed UnitIds without blocking compilation"
           (and (= catalog-ids (set (keys permissions)))
                (not (contains? permissions "retired-unit"))
                (= catalog-ids
                   (set (keys (get (north.agent-catalog/compile-activation
                                    catalog permissions)
                                   "permissions")))))))

  (check "UnitIds are globally unique across sets, skills, and hooks"
         (throws-containing?
          "duplicate catalog unit ids"
          #(load-value (mutate-unit base "code-upstream-guard"
                                    (fn [unit] (assoc unit "id" "webdev"))))))
  (check "duplicate set members are rejected"
         (throws-containing?
          "duplicate or invalid members"
          #(load-value (mutate-unit base "coordination"
                                    (fn [unit] (update unit "members" conj "messages"))))))
  (check "unknown member references are rejected"
         (throws-containing?
          "unknown member"
          #(load-value (mutate-unit base "coordination"
                                    (fn [unit] (update unit "members" conj "missing"))))))
  (check "broadly distributed sets reject project-only members"
         (throws-containing?
          "contains project-only member"
          #(load-value (mutate-unit base "coordination"
                                    (fn [unit]
                                      (update unit "members" conj "code-as-facts"))))))
  (check "project-only set membership and hook support remain valid"
         (loads? (update base "units" conj project-set-fixture)))
  (check "broadly distributed claimants reject project-only hooks"
         (throws-containing?
          "cannot depend on project-only hook"
          #(load-value (mutate-unit base "code-upstream-guard"
                                    (fn [unit] (assoc unit "supports" ["webdev"]))))))
  (check "owner escapes are rejected"
         (throws-containing?
          "owner escapes"
          #(load-value (mutate-unit base "webdev"
                                    (fn [unit] (assoc-in unit ["owner" "path"] "../escape"))))))
  (check "undeclared unit fields are rejected"
         (throws-containing?
          "unsupported fields"
          #(load-value (mutate-unit base "webdev"
                                    (fn [unit] (assoc unit "legacyField" true))))))
  (check "catalog categories reject disagreement with source frontmatter"
         (throws-containing?
          "source declares category"
          #(load-value (mutate-unit base "firn"
                                    (fn [unit]
                                      (assoc unit "category" "uncategorized"))))))
  (check "unknown distribution targets are rejected"
         (throws-containing?
          "invalid or duplicate targets"
          #(load-value (mutate-unit
                        base "webdev"
                        (fn [unit]
                          (assoc-in unit ["distributions" 0 "targets"] ["unknown"]))))))
  (check "exact recursive set cycles are rejected"
         (throws-containing?
          "catalog set cycle"
          #(load-value (mutate-unit base "coordination"
                                    (fn [unit] (update unit "members" conj "orchestration"))))))

  (let [catalog (north.agent-catalog/load-catalog)
        activation (north.agent-catalog/compile-activation
                    catalog (assoc (north.agent-catalog/seed-permissions catalog)
                                   "code-as-facts" "on"))
        state-root (str tmp "/state")
        codex-skills (.toPath (io/file tmp "direct-codex-skills"))]
    (with-redefs [north.agent-catalog/agents-root (constantly state-root)
                  north.agent-catalog/codex-skills-dir (constantly codex-skills)]
      (north.agent-catalog/publish! activation)
      (let [current (io/file state-root "current")
            published (north.agent-catalog/current-activation)
            after (north.agent-catalog/compile-activation
                   (north.agent-catalog/load-catalog)
                   (north.agent-catalog/seed-permissions catalog))
            shared (set (map #(.getName %) (or (.listFiles (io/file current "skills/shared"))
                                               (make-array java.io.File 0))))]
        (check "one atomic current pointer names the content-addressed generation"
               (and (java.nio.file.Files/isSymbolicLink (.toPath current))
                    (str/starts-with? (str (java.nio.file.Files/readSymbolicLink (.toPath current)))
                                      "gen-")
                    (= (get activation "generationId") (get published "generationId"))))
        (check "publication and scratch cleanup never mutate owner payloads"
               (= (into {} (map (fn [unit]
                                  [(get unit "id")
                                   (get-in unit ["ownerProvenance" "contentDigest"])]))
                         (get activation "units"))
                  (into {} (map (fn [unit]
                                  [(get unit "id")
                                   (get-in unit ["ownerProvenance" "contentDigest"])]))
                         (get after "units"))))
        (check "generation has one materialized shared skill farm"
               (and (= 43 (count shared))
                    (not (.exists (io/file current "skills/codex")))
                    (every? #(.isDirectory (io/file current "skills/shared" %)) shared)))
        (check "Codex links are exactly the active shared skill plan"
               (and (java.nio.file.Files/isSymbolicLink
                     (.resolve codex-skills "webdev"))
                    (not (java.nio.file.Files/exists
                          (.resolve codex-skills "code-as-facts")
                          (into-array java.nio.file.LinkOption
                                      [java.nio.file.LinkOption/NOFOLLOW_LINKS])))))
        (check "project targets materialize explicitly permitted units only in their package"
               (and (.isFile (io/file current
                                      "projects/beagle/skill/code-as-facts/SKILL.md"))
                    (.isFile (io/file current
                                      "projects/beagle/hook/code-upstream-guard"))
                    (not (contains? shared "code-as-facts"))
                    (not (.exists (io/file current "skills/shared/code-as-facts")))))
        (check "every declared instruction target and provider adapter is materialized"
               (every? #(.isFile (io/file current %))
                       ["instructions/shared/AGENTS.md"
                        "instructions/codex/AGENTS.md"
                        "instructions/code/AGENTS.md"
                        "instructions/north/AGENTS.md"
                        "instructions/bridge/AGENTS.md"
                        "provider-hooks/lib/harness-dial.sh"
                        "provider-hooks/logcompress.js"]))
        (check "every declared agent-template target is materialized by UnitId"
               (every? #(.isFile (io/file current %))
                       ["agent-templates/north/staffing/integrator.md"
                        "agent-templates/claude/staffing/integrator.md"]))
        (check "provider activation helper and lifecycle adapters materialize from exact owners"
               (let [support (first (filter #(= "north-agent-activation" (get % "id"))
                                            (get activation "providerSupport")))
                     helper (io/file current "provider-hooks/lib/north-agent-activation.sh")]
                 (and (= {"repo" "nixos-config"
                          "path" "dotfiles/agents/lib/north-agent-activation.sh"}
                         (get support "owner"))
                      (= "lib/north-agent-activation.sh" (get support "path"))
                      (.isFile helper)
                      (= (slurp (north.agent-catalog/owner-path
                                 (get support "owner") "activation helper test"))
                         (slurp helper))
                      (every? #(.isFile (io/file current "provider-hooks"
                                                  (str % "-codex")))
                              lifecycle-hook-ids))))
        (check "materialized generation never links back into owner trees"
               (not-any? #(java.nio.file.Files/isSymbolicLink %)
                         (with-open [walk (java.nio.file.Files/walk
                                          (.toPath (.getCanonicalFile current))
                                          (make-array java.nio.file.FileVisitOption 0))]
                           (vec (iterator-seq (.iterator walk))))))
        (let [hooks-root (io/file current "provider-hooks")
              runtime-root (io/file hooks-root "runtime")
              target-root (io/file hooks-root "north/bin")
              gate-state (io/file tmp "adapter-gate-state")
              activation-file (io/file gate-state "current/activation.json")
              run-adapter
              (fn [id]
                (p/shell {:out :string :err :string :continue true :in ""
                          :extra-env
                          {"NORTH_AGENT_STATE_ROOT" (str gate-state)
                           "NORTH_MANAGED_LANE" "0"
                           "AGENT_TOPOLOGY" ""
                           "AGENT_ID" ""}}
                         (str (io/file hooks-root (str id "-codex")))))]
          (.mkdirs runtime-root)
          (.mkdirs target-root)
          (.mkdirs (.getParentFile activation-file))
          (java.nio.file.Files/createSymbolicLink
           (.toPath (io/file runtime-root "bash")) (executable-on-path "bash")
           (make-array java.nio.file.attribute.FileAttribute 0))
          (java.nio.file.Files/createSymbolicLink
           (.toPath (io/file runtime-root "python3")) (executable-on-path "python3")
           (make-array java.nio.file.attribute.FileAttribute 0))
          (doseq [id lifecycle-hook-ids]
            (write-executable!
             (io/file target-root id)
             (str "printf 'executed:" id ":%s\\n' \"${AGENT_PROVIDER:-}\"\n")))
          (spit activation-file (json/generate-string activation))
          (check "materialized lifecycle adapters execute when their exact hooks are active"
                 (every?
                  (fn [[id result]]
                    (and (zero? (:exit result))
                         (= (str "executed:" id ":openai\n") (:out result))
                         (str/blank? (:err result))))
                  (mapv (fn [id] [id (run-adapter id)]) lifecycle-hook-ids)))
          (check "materialized lifecycle adapters stay inert when only their exact hook is off"
                 (every?
                  (fn [[_id result]]
                    (and (zero? (:exit result))
                         (str/blank? (:out result))
                         (str/blank? (:err result))))
                  (mapv (fn [id]
                          (spit activation-file
                                (json/generate-string
                                 (north.agent-catalog/compile-activation
                                  catalog
                                  (assoc (get activation "permissions") id "off"))))
                          [id (run-adapter id)])
                        lifecycle-hook-ids)))))))

  (let [cli (str root "/cli/config-cli.clj")
        cli-home (str tmp "/cli-home")
        cli-state (str tmp "/cli-state")
        configured-roots (some-> (System/getenv "NORTH_REPO_ROOTS")
                                 json/parse-string)
        repo-roots (json/generate-string
                    (merge {"north" root
                            "beagle" "/home/tom/code/beagle/main"
                            "nixos-config" "/home/tom/code/nixos-config/main"}
                           configured-roots
                           {"north" root}))
        env {"HOME" cli-home "NORTH_HOME" root
             "NORTH_AGENT_STATE_ROOT" cli-state "NORTH_REPO_ROOTS" repo-roots}
        run (fn [& args]
              (apply p/shell {:out :string :err :string :continue true :extra-env env}
                     (into ["bb" cli] args)))
        _ (.mkdirs (io/file cli-home ".codex/skills/.system"))
        _ (spit (io/file cli-home ".codex/skills/.system/marker") "owned by Codex")
        _ (.mkdirs (io/file cli-home ".codex/skills/unowned-real"))
        _ (.mkdirs (io/file cli-state))
        _ (spit (io/file cli-state "codex-managed-skills.json")
                "{\"schema\":\"north.codex-managed-skills/v1\",\"ids\":[\"retired-skill\"]}")
        _ (java.nio.file.Files/createSymbolicLink
           (.toPath (io/file cli-home ".codex/skills/retired-skill"))
           (.toPath (io/file cli-state "current/skills/shared/retired-skill"))
           (make-array java.nio.file.attribute.FileAttribute 0))
        synced (run "agents" "sync" "--json")
        status (run "agents" "status" "--json")
        disabled (run "agents" "off" "build-vs-reuse")
        inspected (run "agents" "inspect" "build-vs-reuse" "--json")
        before-unknown (run "agents" "status" "--json")
        unknown (run "agents" "off" "unknown-unit")
        after-unknown (run "agents" "status" "--json")
        set-disabled (run "agents" "off" "coordination")
        set-inspected (run "agents" "inspect" "coordination" "--json")
        hook-disabled (run "agents" "off" "tripwire-guard")
        hook-inspected (run "agents" "inspect" "tripwire-guard" "--json")]
    (check "north config agents sync and JSON status expose the exact contract"
           (and (zero? (:exit synced)) (zero? (:exit status))
                (= "north.agent-activation/v1"
                   (get (json/parse-string (:out status)) "schema"))))
    (check "direct UnitId mutations publish through the one generation"
           (let [unit (json/parse-string (:out inspected))]
             (and (zero? (:exit disabled)) (zero? (:exit inspected))
                  (= "off" (get unit "permission"))
                  (false? (get unit "active")))))
    (check "an unknown UnitId mutation is rejected without publishing"
           (and (not (zero? (:exit unknown)))
                (str/includes? (:err unknown) "unknown unit: unknown-unit")
                (= (get (json/parse-string (:out before-unknown)) "generationId")
                   (get (json/parse-string (:out after-unknown)) "generationId"))))
    (check "direct UnitId ABI accepts sets and hooks"
           (and (zero? (:exit set-disabled)) (zero? (:exit hook-disabled))
                (= "off" (get (json/parse-string (:out set-inspected)) "permission"))
                (= "off" (get (json/parse-string (:out hook-inspected)) "permission"))))
    (check "Codex compatibility preserves system and unowned real entries"
           (and (.isFile (io/file cli-home ".codex/skills/.system/marker"))
                (.isDirectory (io/file cli-home ".codex/skills/unowned-real"))
                (java.nio.file.Files/isSymbolicLink
                 (.toPath (io/file cli-home ".codex/skills/webdev")))
                (not (java.nio.file.Files/exists
                      (.toPath (io/file cli-home ".codex/skills/retired-skill"))
                      (into-array java.nio.file.LinkOption
                                  [java.nio.file.LinkOption/NOFOLLOW_LINKS])))
                (not (.exists (io/file cli-home ".codex/skills/build-vs-reuse"))))))

  (let [catalog (north.agent-catalog/load-catalog)
        activation (north.agent-catalog/compile-activation
                    catalog (north.agent-catalog/seed-permissions catalog))
        state-root (str tmp "/collision-state")
        codex-skills (.toPath (io/file tmp "collision-codex-skills"))]
    (java.nio.file.Files/createDirectories
     (.resolve codex-skills "webdev")
     (make-array java.nio.file.attribute.FileAttribute 0))
    (with-redefs [north.agent-catalog/agents-root (constantly state-root)
                  north.agent-catalog/codex-skills-dir (constantly codex-skills)]
      (check "an unowned Codex collision aborts before current changes"
             (and (throws-containing?
                   "collides with an unowned entry"
                   #(north.agent-catalog/publish! activation))
                  (not (java.nio.file.Files/exists
                        (.toPath (io/file state-root "current"))
                        (into-array java.nio.file.LinkOption
                                    [java.nio.file.LinkOption/NOFOLLOW_LINKS])))))))

  (let [catalog (north.agent-catalog/load-catalog)
        activation (north.agent-catalog/compile-activation
                    catalog (north.agent-catalog/seed-permissions catalog))
        state-root (str tmp "/symlink-state")
        target (.toPath (io/file tmp "legacy-skills-target"))
        codex-skills (.toPath (io/file tmp "legacy-codex/skills"))]
    (java.nio.file.Files/createDirectories
     target (make-array java.nio.file.attribute.FileAttribute 0))
    (java.nio.file.Files/createDirectories
     (.getParent codex-skills) (make-array java.nio.file.attribute.FileAttribute 0))
    (java.nio.file.Files/createSymbolicLink
     codex-skills target (make-array java.nio.file.attribute.FileAttribute 0))
    (with-redefs [north.agent-catalog/agents-root (constantly state-root)
                  north.agent-catalog/codex-skills-dir (constantly codex-skills)]
      (check "a legacy whole-directory Codex link is rejected without write-through"
             (and (throws-containing?
                   "must be a real directory"
                   #(north.agent-catalog/publish! activation))
                  (java.nio.file.Files/isSymbolicLink codex-skills)
                  (empty? (seq (.listFiles (.toFile target))))))))

  (finally
    (delete-scratch! tmp)))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nagent catalog: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
