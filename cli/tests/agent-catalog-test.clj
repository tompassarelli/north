(ns beagle.user
  (:require [cheshire.core :as json]
            [babashka.process :as p]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [clojure.lang ExceptionInfo]))

(defn new-final-artifacts []
  (let [cell (atom nil)]
  cell))

(def ^String root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/agent-catalog.clj"))

(def checks (atom []))

(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn ^Boolean throws-containing? [^String text f]
  (try
  (f)
  false
  (catch ExceptionInfo error
    (str/includes? (.getMessage error) text))))

(def tmp (.toFile (java.nio.file.Files/createTempDirectory "north-agent-catalog-test-" (make-array java.nio.file.attribute.FileAttribute 0))))

(def base-catalog (north.agent-catalog/load-catalog!))

(def base (:catalog base-catalog))

(def catalog-schema-value (json/parse-string (slurp (str root "/agent-catalog/catalog.schema.json"))))

(def activation-schema-value (json/parse-string (slurp (str root "/agent-catalog/activation.schema.json"))))

(def catalog-unit-fields #{"id" "kind" "title" "triggerDescription" "category" "owner" "members" "supports" "distributions"})

(def resolved-unit-fields #{"id" "kind" "title" "triggerDescription" "category" "permission" "active" "owner" "ownerProvenance" "members" "supports" "distributions" "activationPaths"})

(def ^String initialization-path (str tmp "/agent-initialization.json"))

(def ^String test-retired-id "retired-test-skill")

(def test-permissions (into (sorted-map) (map (fn [unit] [(get unit "id") "on"]) (get base "units"))))

(def test-initialization {"schema" "north.agent-initialization/v1" "id" "catalog-initialization-test" "permissionAuthorities" [{"id" "test-authority" "source" "generated catalog regression input" "permissions" test-permissions}] "permissionResolutions" {} "skillLinks" (vec (concat (for [unit (get base "units")
   :when (and (= "skill" (get unit "kind")) (some (fn [distribution] (and (= "skill" (get distribution "type")) (some? (some (fn [target] (= "shared" target)) (get distribution "targets"))))) (get unit "distributions")) (not= "build-vs-reuse" (get unit "id")))]
  {"id" (get unit "id") "owner" (get unit "owner") "action" "adopt"}) [{"id" test-retired-id "owner" {"repo" "north" "path" "README.md"} "action" "retire"}])) "retiredPermissionObservations" [{"authority" "test-authority" "sourceId" test-retired-id "permission" "on" "reason" "Synthetic retired identity exercises one-shot cleanup."}]})

(spit initialization-path (json/generate-string test-initialization))

(defn initial-permissions [catalog]
  (:permissions (north.agent-catalog/load-initialization! catalog initialization-path)))

(defn load-value [value]
  (with-redefs-fn {(ns-resolve 'north.agent-catalog 'load-effective-catalog!) (constantly value)} north.agent-catalog/load-catalog!))

(defn ^Boolean loads? [value]
  (try
  (load-value value)
  true
  (catch ExceptionInfo _
    false)))

(defn mutate-unit [catalog ^String id f]
  (update catalog "units" (fn [units] (mapv (fn [unit] (if (= id (get unit "id")) (f unit) unit)) units))))

(def lifecycle-hook-ids ["north-on-spawn" "north-on-tooluse" "north-on-stop" "north-on-terminal" "north-mark-delegated"])

(def PATH-SEPARATOR-RE (re-pattern (java.util.regex.Pattern/quote java.io.File/pathSeparator)))

(defn executable-on-path [^String name]
  (or (some (fn [^String directory] (let [candidate (io/file directory name)]
  (if (and (.isFile candidate) (.canExecute candidate)) (do
  (.toRealPath (.toPath candidate) (make-array java.nio.file.LinkOption 0)))))) (str/split (or (System/getenv "PATH") "") PATH-SEPARATOR-RE)) (throw (ex-info (str "missing executable on PATH: " name) {:name name}))))

(defn write-executable! [path ^String content]
  (let [file (io/file path)]
  (.mkdirs (.getParentFile file))
  (spit file content)
  (if (not (.setExecutable file true false)) (do
  (throw (ex-info (str "cannot make fixture executable: " file) {:path (str file)}))))
  file))

(def project-module-fixture {"id" "beagle-tools" "kind" "module" "title" "Beagle Tools" "triggerDescription" "Project-packaged Beagle authoring behavior." "owner" {"repo" "north" "path" "agent-catalog/north.json"} "members" ["code-as-facts" "code-upstream-guard"] "distributions" [{"type" "projectPackage" "targets" ["project:beagle"] "owner" {"repo" "north" "path" "agent-catalog/north.json"}}]})

(defn resolved-link-target [path]
  (if (java.nio.file.Files/isSymbolicLink path) (do
  (-> (.resolve (.getParent path) (java.nio.file.Files/readSymbolicLink path)) .toAbsolutePath .normalize))))

(defn populate-initial-links! [directory initialization]
  (java.nio.file.Files/createDirectories directory (make-array java.nio.file.attribute.FileAttribute 0))
  (doseq [[id entry] (:skill-links initialization)]
  (java.nio.file.Files/createSymbolicLink (.resolve directory id) (get entry "target") (make-array java.nio.file.attribute.FileAttribute 0))))

(defn ^Boolean no-follow-exists? [path]
  (java.nio.file.Files/exists path (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])))

(defn path-state [path]
  (cond
  (not (no-follow-exists? path)) ["absent" nil]
  (java.nio.file.Files/isSymbolicLink path) ["link" (str (java.nio.file.Files/readSymbolicLink path))]
  (java.nio.file.Files/isRegularFile path (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])) ["file" (slurp (str path))]
  (java.nio.file.Files/isDirectory path (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])) ["directory" nil]
  :else ["other" nil]))

(defn shallow-snapshot [directory]
  (if (no-follow-exists? directory) (with-open [stream (java.nio.file.Files/list directory)]
  (into (sorted-map) (for [path (iterator-seq (.iterator stream))]
  [(str (.getFileName path)) (path-state path)]))) {}))

(defn delete-scratch! [path]
  (if (.exists (io/file path)) (do
  (with-open [stream (java.nio.file.Files/walk (.toPath (io/file path)) (make-array java.nio.file.FileVisitOption 0))]
  (doseq [entry (reverse (iterator-seq (.iterator stream)))]
  (java.nio.file.Files/deleteIfExists entry))))))

(try
  (let [catalog (north.agent-catalog/load-catalog!)
   activation (north.agent-catalog/compile-activation! catalog (initial-permissions catalog))
   by-id (into {} (map (juxt (fn [unit] (get unit "id")) identity) (get activation "units")))]
  (check! "catalog inventories every audited module, skill, and hook" (and (every? (fn [id] (contains? (:by-id catalog) id)) (concat ["agent-machinery" "work-ownership-distilled" "agent-run-design-distilled" "coordination" "agent-run-lifecycle-distilled" "referents-distilled" "elicit" "settle-work" "store-modeling" "code-as-facts" "code-upstream-guard"] lifecycle-hook-ids)) (not-any? (fn [id] (contains? (:by-id catalog) id)) ["orchestration" "compose-distilled" "compose-reference" "messages-distilled" "messages-reference" "assignments-distilled" "assignments-reference"])))
  (check! "coordination catalogs only Referents and run-lifecycle skills" (= #{"referents-distilled" "agent-run-lifecycle-distilled" "agent-run-lifecycle-reference"} (->> (get base "units") (filter (fn [unit] (and (= "skill" (get unit "kind")) (= "north" (get-in unit ["owner" "repo"])) (str/starts-with? (get-in unit ["owner" "path"]) "coordination/")))) (map (fn [unit] (get unit "id"))) set)))
  (check! "generation exposes the stable root contract" (and (= "north.agent-activation/v1" (get activation "schema")) (= "north.agent-catalog/v1" (get activation "catalogSchema")) (re-matches #"sha256:[0-9a-f]{64}" (get activation "catalogDigest")) (re-matches #"sha256:[0-9a-f]{64}" (get activation "generationId")) (= (set (map (fn [unit] (get unit "id")) (:units catalog))) (set (keys (get activation "permissions"))))))
  (check! "unit schemas and records expose only the global UnitId contract" (and (= catalog-unit-fields (set (keys (get-in catalog-schema-value ["$defs" "unit" "properties"])))) (= resolved-unit-fields (set (keys (get-in activation-schema-value ["$defs" "unit" "properties"])))) (every? (fn [unit] (every? (fn [field] (contains? catalog-unit-fields field)) (keys unit))) (get base "units")) (every? (fn [unit] (every? (fn [field] (contains? resolved-unit-fields field)) (keys unit))) (get activation "units"))))
  (check! "generation identity commits deterministic owner content and revision provenance" (and (= (get activation "generationId") (get (north.agent-catalog/compile-activation! catalog (initial-permissions catalog)) "generationId")) (every? (fn [unit] (boolean (and (re-matches #"sha256:[0-9a-f]{64}" (get-in unit ["ownerProvenance" "contentDigest"])) (re-matches #"[0-9a-f]{40,64}" (get-in unit ["ownerProvenance" "revision"]))))) (get activation "units"))))
  (check! "every resolved unit carries stored permission and all consumer fields" (every? (fn [unit] (let [fields (set (keys unit))]
  (every? (fn [field] (contains? fields field)) ["id" "kind" "title" "triggerDescription" "permission" "active" "owner" "members" "supports" "distributions" "activationPaths"]))) (get activation "units")))
  (check! "current package and North modules expose their direct member paths" (and (some (fn [path] (= ["agent-machinery" "work-ownership-distilled"] path)) (get-in by-id ["work-ownership-distilled" "activationPaths"])) (some (fn [path] (= ["agent-machinery" "agent-run-design-distilled"] path)) (get-in by-id ["agent-run-design-distilled" "activationPaths"])) (some (fn [path] (= ["coordination" "referents-distilled"] path)) (get-in by-id ["referents-distilled" "activationPaths"])) (some (fn [path] (= ["coordination" "agent-run-lifecycle-distilled"] path)) (get-in by-id ["agent-run-lifecycle-distilled" "activationPaths"]))))
  (check! "activation paths contain no retired lifecycle UnitIds" (let [retired #{"orchestration" "compose-distilled" "compose-reference" "messages-distilled" "messages-reference" "assignments-distilled" "assignments-reference"}]
  (not-any? (fn [id] (contains? retired id)) (mapcat (fn [unit] (mapcat identity (get unit "activationPaths"))) (get activation "units")))))
  (check! "Codex lifecycle hooks have five exact independent projection outputs" (let [owner-for (fn [^String id] {"repo" "nixos-config" "path" (str "dotfiles/codex/hooks/" id "-codex")})
   entries (->> (get-in activation ["projectionPlan" "providerAdapter" "codex"]) (filter (fn [entry] (some? (some (fn [^String id] (= (get entry "unitId") id)) lifecycle-hook-ids)))) (group-by (fn [entry] (get entry "unitId"))))]
  (and (= (set lifecycle-hook-ids) (set (keys entries))) (every? (fn [^String id] (let [[entry :as matches] (get entries id)
   owner (owner-for id)]
  (boolean (and (= 1 (count matches)) (= #{"unitId" "owner" "adapterId" "provenance"} (set (keys entry))) (= id (get entry "unitId")) (= (str id "-codex") (get entry "adapterId")) (= owner (get entry "owner")) (= owner (get-in entry ["provenance" "owner"])) (re-matches #"[0-9a-f]{40,64}" (get-in entry ["provenance" "revision"])) (re-matches #"sha256:[0-9a-f]{64}" (get-in entry ["provenance" "contentDigest"])))))) lifecycle-hook-ids))))
  (check! "project-private units retain project distributions when permitted" (every? (fn [^String id] (and (= "on" (get-in by-id [id "permission"])) (true? (get-in by-id [id "active"])) (= [["project:beagle"]] (mapv (fn [distribution] (get distribution "targets")) (get-in by-id [id "distributions"]))))) ["code-as-facts" "code-upstream-guard"]))
  (check! "catalog category agrees with source frontmatter when declared" (= "nixos" (get-in by-id ["firn" "category"])))
  (check! "independent lifecycle hooks activate from explicit permissions" (every? (fn [^String id] (and (= "on" (get-in by-id [id "permission"])) (true? (get-in by-id [id "active"])))) lifecycle-hook-ids)))
  (let [catalog (north.agent-catalog/load-catalog!)]
  (check! "fresh catalog permissions are all explicit off" (= #{"off"} (set (vals (north.agent-catalog/default-permissions catalog))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   catalog-ids (set (map (fn [unit] (get unit "id")) (:units catalog)))
   permissions (with-redefs [north.agent-catalog/current-activation (constantly {"permissions" {"webdev" "off" "retired-unit" "on"}})] (north.agent-catalog/current-permissions catalog))]
  (check! "catalog upgrade retains an existing permission override" (= "off" (get permissions "webdev")))
  (check! "catalog upgrade defaults a newly introduced UnitId off" (= "off" (get permissions "importing-skills")))
  (check! "catalog upgrade prunes removed UnitIds without blocking compilation" (and (= catalog-ids (set (keys permissions))) (not (contains? permissions "retired-unit")) (= catalog-ids (set (keys (get (north.agent-catalog/compile-activation! catalog permissions) "permissions")))))))
  (check! "UnitIds are globally unique across modules, skills, and hooks" (throws-containing? "duplicate catalog unit ids" (fn [] (load-value (mutate-unit base "code-upstream-guard" (fn [unit] (assoc unit "id" "webdev")))))))
  (check! "duplicate module members are rejected" (throws-containing? "duplicate or invalid members" (fn [] (load-value (mutate-unit base "coordination" (fn [unit] (update unit "members" conj "messages")))))))
  (check! "unknown member references are rejected" (throws-containing? "unknown member" (fn [] (load-value (mutate-unit base "coordination" (fn [unit] (update unit "members" conj "missing")))))))
  (check! "broadly distributed modules reject project-only members" (throws-containing? "contains project-only member" (fn [] (load-value (mutate-unit base "coordination" (fn [unit] (update unit "members" conj "code-as-facts")))))))
  (check! "project-only module membership and hook support remain valid" (loads? (update base "units" conj project-module-fixture)))
  (check! "broadly distributed claimants reject project-only hooks" (throws-containing? "cannot depend on project-only hook" (fn [] (load-value (mutate-unit base "code-upstream-guard" (fn [unit] (assoc unit "supports" ["webdev"])))))))
  (check! "owner escapes are rejected" (throws-containing? "owner escapes" (fn [] (load-value (mutate-unit base "webdev" (fn [unit] (assoc-in unit ["owner" "path"] "../escape")))))))
  (check! "undeclared unit fields are rejected" (throws-containing? "unsupported fields" (fn [] (load-value (mutate-unit base "webdev" (fn [unit] (assoc unit "legacyField" true)))))))
  (check! "catalog categories reject disagreement with source frontmatter" (throws-containing? "source declares category" (fn [] (load-value (mutate-unit base "firn" (fn [unit] (assoc unit "category" "uncategorized")))))))
  (check! "unknown distribution targets are rejected" (throws-containing? "invalid or duplicate targets" (fn [] (load-value (mutate-unit base "webdev" (fn [unit] (assoc-in unit ["distributions" 0 "targets"] ["unknown"])))))))
  (check! "exact recursive module cycles are rejected" (throws-containing? "catalog module cycle" (fn [] (load-value (mutate-unit base "coordination" (fn [unit] (update unit "members" conj "coordination")))))))
  (let [catalog (load-value (-> base (update "units" conj project-module-fixture) (update "rootOrder" conj "beagle-tools")))
   permissions (into (sorted-map) (map (fn [unit] [(get unit "id") "on"]) (:units catalog)))
   activation (north.agent-catalog/compile-activation! catalog permissions)
   by-id (into {} (map (juxt (fn [unit] (get unit "id")) identity) (get activation "units")))]
  (check! "modules compose direct skill and hook members alongside nested modules" (and (some (fn [path] (= ["coordination"] path)) (get-in by-id ["coordination" "activationPaths"])) (some (fn [path] (= ["beagle-tools" "code-as-facts"] path)) (get-in by-id ["code-as-facts" "activationPaths"])) (some (fn [path] (= ["beagle-tools" "code-upstream-guard"] path)) (get-in by-id ["code-upstream-guard" "activationPaths"])))))
  (let [conflicting (update (json/parse-string (slurp initialization-path)) "permissionAuthorities" conj {"id" "conflicting-authority" "source" "generated conflict" "permissions" {"webdev" "off"}})
   path (str tmp "/unresolved-initialization.json")]
  (spit path (json/generate-string conflicting))
  (check! "unresolved overlapping initialization authorities abort" (throws-containing? "conflicting initialization authorities" (fn [] (north.agent-catalog/load-initialization! (north.agent-catalog/load-catalog!) path)))))
  (let [future-unit {"id" "future-hook" "kind" "hook" "category" "authoring" "title" "Future Hook" "triggerDescription" "A catalog-upgrade permission fixture." "owner" {"repo" "north" "path" "agent-runtime/hooks/agent-spawn-guard.bjs"} "distributions" [{"type" "hook" "targets" ["codex"]}]}
   upgraded (load-value (update base "units" conj future-unit))]
  (with-redefs [north.agent-catalog/current-activation (constantly {"schema" "north.agent-activation/v1" "permissions" {"webdev" "on" "retired-unit" "malformed-but-retired"}})] (let [permissions (north.agent-catalog/current-permissions upgraded)]
  (check! "catalog upgrades retain surviving state, default new IDs off, and drop retired IDs" (and (= "on" (get permissions "webdev")) (= "off" (get permissions "future-hook")) (not (contains? permissions "retired-unit")))))))
  (with-redefs [north.agent-catalog/current-activation (constantly {"schema" "north.agent-activation/v1" "permissions" {"webdev" "malformed"}})] (check! "malformed surviving permissions still abort catalog upgrades" (throws-containing? "invalid permission" (fn [] (north.agent-catalog/current-permissions (north.agent-catalog/load-catalog!))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   activation (north.agent-catalog/compile-activation! catalog (reduce (fn [permissions ^String id] (assoc permissions id "on")) (assoc (initial-permissions catalog) "code-as-facts" "on") lifecycle-hook-ids))
   state-root (str tmp "/state")
   codex-skills (.toPath (io/file tmp "direct-codex-skills"))]
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (north.agent-catalog/publish! activation) (let [current (io/file state-root "current")
   published (north.agent-catalog/current-activation)
   after (north.agent-catalog/compile-activation! (north.agent-catalog/load-catalog!) (initial-permissions catalog))
   shared (set (map (fn [file] (.getName file)) (or (.listFiles (io/file current "skills/shared")) (make-array java.io.File 0))))
   planned-shared (set (map (fn [entry] (get entry "unitId")) (get-in activation ["projectionPlan" "skill" "shared"])))]
  (check! "one atomic current pointer names the content-addressed generation" (and (java.nio.file.Files/isSymbolicLink (.toPath current)) (str/starts-with? (str (java.nio.file.Files/readSymbolicLink (.toPath current))) "gen-") (= (get activation "generationId") (get published "generationId"))))
  (check! "publication and scratch cleanup never mutate owner payloads" (= (into {} (map (fn [unit] [(get unit "id") (get-in unit ["ownerProvenance" "contentDigest"])]) (get activation "units"))) (into {} (map (fn [unit] [(get unit "id") (get-in unit ["ownerProvenance" "contentDigest"])]) (get after "units")))))
  (check! "generation has one materialized shared skill farm" (and (= planned-shared shared) (not (.exists (io/file current "skills/codex"))) (every? (fn [id] (.isDirectory (io/file current "skills/shared" id))) shared)))
  (check! "Codex links are exactly the active shared skill plan" (and (java.nio.file.Files/isSymbolicLink (.resolve codex-skills "importing-skills")) (java.nio.file.Files/isSymbolicLink (.resolve codex-skills "webdev")) (not (java.nio.file.Files/exists (.resolve codex-skills "code-as-facts") (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])))))
  (check! "project targets materialize explicitly permitted units only in their package" (and (.isFile (io/file current "projects/beagle/skill/code-as-facts/SKILL.md")) (.isFile (io/file current "projects/beagle/hook/code-upstream-guard")) (not (contains? shared "code-as-facts")) (not (.exists (io/file current "skills/shared/code-as-facts")))))
  (check! "every declared instruction target and provider adapter is materialized" (every? (fn [^String path] (.isFile (io/file current path))) ["instructions/shared/AGENTS.md" "instructions/codex/AGENTS.md" "instructions/code/AGENTS.md" "instructions/north/AGENTS.md" "instructions/bridge/AGENTS.md" "provider-hooks/lib/harness-dial.sh" "provider-hooks/logcompress.py"]))
  (check! "every declared agent-template target is materialized by UnitId" (every? (fn [^String path] (.isFile (io/file current path))) ["agent-templates/north/delegation/integrator.md" "agent-templates/claude/delegation/integrator.md"]))
  (check! "provider activation helper and lifecycle adapters materialize from exact owners" (let [support (first (filter (fn [entry] (= "north-agent-activation" (get entry "id"))) (get activation "providerSupport")))
   helper (io/file current "provider-hooks/lib/north-agent-activation.sh")]
  (and (= {"repo" "nixos-config" "path" "dotfiles/agents/lib/north-agent-activation.sh"} (get support "owner")) (= "lib/north-agent-activation.sh" (get support "path")) (.isFile helper) (= (slurp (north.agent-catalog/owner-path! (get support "owner") "activation helper test")) (slurp helper)) (every? (fn [^String id] (.isFile (io/file current "provider-hooks" (str id "-codex")))) lifecycle-hook-ids))))
  (check! "materialized generation never links back into owner trees" (not-any? (fn [path] (java.nio.file.Files/isSymbolicLink path)) (with-open [walk (java.nio.file.Files/walk (.toPath (.getCanonicalFile current)) (make-array java.nio.file.FileVisitOption 0))]
  (vec (iterator-seq (.iterator walk))))))
  (let [hooks-root (io/file current "provider-hooks")
   runtime-root (io/file hooks-root "runtime")
   target-root (io/file hooks-root "north/bin")
   gate-state (io/file tmp "adapter-gate-state")
   activation-file (io/file gate-state "current/activation.json")
   run-adapter (fn [^String id] (p/shell {:out :string :err :string :continue true :in "" :extra-env {"NORTH_AGENT_STATE_ROOT" (str gate-state) "NORTH_MANAGED_LANE" "0" "AGENT_TOPOLOGY" "" "AGENT_ID" ""}} (str (io/file hooks-root (str id "-codex")))))]
  (.mkdirs runtime-root)
  (.mkdirs target-root)
  (.mkdirs (.getParentFile activation-file))
  (java.nio.file.Files/createSymbolicLink (.toPath (io/file runtime-root "bash")) (executable-on-path "bash") (make-array java.nio.file.attribute.FileAttribute 0))
  (java.nio.file.Files/createSymbolicLink (.toPath (io/file runtime-root "python3")) (executable-on-path "python3") (make-array java.nio.file.attribute.FileAttribute 0))
  (doseq [id lifecycle-hook-ids]
  (write-executable! (io/file target-root id) (str "printf 'executed:" id ":%s\\n' \"${AGENT_PROVIDER:-}\"\n")))
  (spit activation-file (json/generate-string activation))
  (check! "materialized lifecycle adapters execute when their exact hooks are active" (every? (fn [[id result]] (and (zero? (:exit result)) (= (str "executed:" id ":openai\n") (:out result)) (str/blank? (:err result)))) (mapv (fn [^String id] [id (run-adapter id)]) lifecycle-hook-ids)))
  (check! "materialized lifecycle adapters stay inert when only their exact hook is off" (every? (fn [[_id result]] (and (zero? (:exit result)) (str/blank? (:out result)) (str/blank? (:err result)))) (mapv (fn [^String id] (spit activation-file (json/generate-string (north.agent-catalog/compile-activation! catalog (assoc (get activation "permissions") id "off"))))
  [id (run-adapter id)]) lifecycle-hook-ids)))))))
  (let [cli (str root "/cli/config-cli.clj")
   cli-home (str tmp "/cli-home")
   cli-state (str tmp "/cli-state")
   configured-roots (some-> (System/getenv "NORTH_REPO_ROOTS") json/parse-string)
   roots (merge {"north" root "beagle" "/home/tom/code/beagle/main" "nixos-config" "/home/tom/code/nixos-config/main"} configured-roots {"north" root})
   repo-roots (json/generate-string roots)
   env {"HOME" cli-home "NORTH_HOME" root "NORTH_AGENT_STATE_ROOT" cli-state "NORTH_REPO_ROOTS" repo-roots}
   run-with (fn [extra & $beagle$rest$host] (let [args (vec $beagle$rest$host)]
  (apply p/shell {:out :string :err :string :continue true :extra-env (merge env extra)} (into ["bb" cli] args))))
   run (fn [& $beagle$rest$host] (let [args (vec $beagle$rest$host)]
  (apply run-with {} args)))
   codex-root (.toPath (io/file cli-home ".codex/skills"))
   init-document (json/parse-string (slurp initialization-path))
   skill-links (get init-document "skillLinks")
   legacy-target (fn [entry] (.normalize (.toAbsolutePath (.toPath (io/file (get roots (get-in entry ["owner" "repo"])) (get-in entry ["owner" "path"]))))))
   _dirs (do
  (.mkdirs (io/file cli-home ".codex/skills/.system"))
  (.mkdirs (io/file cli-home ".codex/skills/unowned-real")))
   _markers (do
  (spit (io/file cli-home ".codex/skills/.system/marker") "owned by Codex\n")
  (spit (io/file cli-home ".codex/skills/unowned-real/marker") "owned by user\n"))
   user-target (.toPath (io/file tmp "user-skill-target"))
   _user-target (java.nio.file.Files/createDirectories user-target (make-array java.nio.file.attribute.FileAttribute 0))
   user-link (.resolve codex-root "unowned-link")
   _user-link (java.nio.file.Files/createSymbolicLink user-link user-target (make-array java.nio.file.attribute.FileAttribute 0))
   user-link-before (java.nio.file.Files/readSymbolicLink user-link)
   _legacy (doseq [entry skill-links]
  (java.nio.file.Files/createSymbolicLink (.resolve codex-root (get entry "id")) (legacy-target entry) (make-array java.nio.file.attribute.FileAttribute 0)))
   synced (run-with {"NORTH_AGENT_INITIALIZATION" initialization-path} "agents" "sync" "--json")
   initialized (json/parse-string (:out synced))
   receipt (json/parse-string (slurp (io/file cli-state "initialization-receipt.json")))
   adopted-exact? (every? (fn [entry] (if (= "adopt" (get entry "action")) (= (.normalize (.toAbsolutePath (.toPath (io/file cli-state "current/skills/shared" (get entry "id"))))) (resolved-link-target (.resolve codex-root (get entry "id")))) (not (java.nio.file.Files/exists (.resolve codex-root (get entry "id")) (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS]))))) skill-links)
   status (run "agents" "status" "--json")
   enabled (run "agents" "on" "build-vs-reuse")
   inspected (run "agents" "inspect" "build-vs-reuse" "--json")
   path-result (run "agents" "path" "build-vs-reuse")
   before-unknown (run "agents" "status" "--json")
   unknown (run "agents" "off" "unknown-unit")
   after-unknown (run "agents" "status" "--json")
   module-disabled (run "agents" "off" "coordination")
   module-inspected (run "agents" "inspect" "coordination" "--json")
   hook-disabled (run "agents" "off" "tripwire-guard")
   hook-inspected (run "agents" "inspect" "tripwire-guard" "--json")
   before-repeat (get (json/parse-string (:out hook-inspected)) "permission")
   repeated (run-with {"NORTH_AGENT_INITIALIZATION" initialization-path} "agents" "sync" "--json")
   after-repeat (run "agents" "inspect" "tripwire-guard" "--json")]
  (check! "north route config agents sync and JSON status expose the exact contract" (and (zero? (:exit synced)) (zero? (:exit status)) (= "north.agent-activation/v1" (get (json/parse-string (:out status)) "schema"))))
  (check! "direct UnitId mutations publish through the one generation" (let [unit (json/parse-string (:out inspected))]
  (and (zero? (:exit enabled)) (zero? (:exit inspected)) (= "on" (get unit "permission")) (true? (get unit "active")))))
  (check! "UnitId inspection and path resolution use the current catalog API" (let [unit (json/parse-string (:out inspected))]
  (and (zero? (:exit path-result)) (= (get unit "resolvedOwnerPath") (str/trim (:out path-result))))))
  (check! "an unknown UnitId mutation is rejected without publishing" (and (not (zero? (:exit unknown))) (str/includes? (:err unknown) "unknown unit: unknown-unit") (= (get (json/parse-string (:out before-unknown)) "generationId") (get (json/parse-string (:out after-unknown)) "generationId"))))
  (check! "direct UnitId ABI accepts modules and hooks" (and (zero? (:exit module-disabled)) (zero? (:exit hook-disabled)) (= "off" (get (json/parse-string (:out module-inspected)) "permission")) (= "off" (get (json/parse-string (:out hook-inspected)) "permission"))))
  (check! "one-time adoption preserves system and genuine user entries exactly" (and (.isFile (io/file cli-home ".codex/skills/.system/marker")) (= "owned by Codex\n" (slurp (io/file cli-home ".codex/skills/.system/marker"))) (.isDirectory (io/file cli-home ".codex/skills/unowned-real")) (= "owned by user\n" (slurp (io/file cli-home ".codex/skills/unowned-real/marker"))) (= user-link-before (java.nio.file.Files/readSymbolicLink user-link))))
  (check! "active legacy links are adopted through the stable generation target" adopted-exact?)
  (check! "initialization receipt audits adopted and explicitly retired links" (and (= "north.agent-initialization-receipt/v1" (get receipt "schema")) (= (get initialized "generationId") (get receipt "generationId")) (= #{test-retired-id} (set (get receipt "retired"))) (= #{"build-vs-reuse"} (set (get receipt "created"))) (= (set (for [entry skill-links
   :when (= "adopt" (get entry "action"))]
  (get entry "id"))) (set (get receipt "adopted"))) (= "on" (get-in receipt ["permissions" "webdev"])) (every? (fn [^String id] (= "on" (get-in receipt ["permissions" id]))) lifecycle-hook-ids)))
  (check! "initialization cannot run twice or mutate the current generation" (and (not (zero? (:exit repeated))) (str/includes? (:err repeated) "one-shot") (= before-repeat (get (json/parse-string (:out after-repeat)) "permission")))))
  (let [catalog (north.agent-catalog/load-catalog!)
   activation (north.agent-catalog/compile-activation! catalog (initial-permissions catalog))
   state-root (str tmp "/collision-state")
   codex-skills (.toPath (io/file tmp "collision-codex-skills"))]
  (java.nio.file.Files/createDirectories (.resolve codex-skills "webdev") (make-array java.nio.file.attribute.FileAttribute 0))
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (check! "an unowned Codex collision aborts before current changes" (and (throws-containing? "collides with an unowned entry" (fn [] (north.agent-catalog/publish! activation))) (not (java.nio.file.Files/exists (.toPath (io/file state-root "current")) (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS])))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   initialization (north.agent-catalog/load-initialization! catalog initialization-path)
   activation (north.agent-catalog/compile-activation! catalog (:permissions initialization))
   state-root (str tmp "/legacy-target-collision-state")
   codex-skills (.toPath (io/file tmp "legacy-target-collision-skills"))
   wrong-target (.toPath (io/file tmp "not-the-owned-skill"))
   collision (.resolve codex-skills "importing-skills")]
  (populate-initial-links! codex-skills initialization)
  (java.nio.file.Files/delete collision)
  (java.nio.file.Files/createSymbolicLink collision wrong-target (make-array java.nio.file.attribute.FileAttribute 0))
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (check! "a legacy-name link with an unowned target aborts before adoption" (and (throws-containing? "collides with an unowned entry" (fn [] (north.agent-catalog/publish! activation initialization))) (= wrong-target (java.nio.file.Files/readSymbolicLink collision)) (not (java.nio.file.Files/exists (.toPath (io/file state-root "current")) (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS]))) (not (.exists (io/file state-root "codex-managed-skills.json")))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   initialization (north.agent-catalog/load-initialization! catalog initialization-path)
   activation (north.agent-catalog/compile-activation! catalog (:permissions initialization))]
  (doseq [[action missing-id] [["adopt" "importing-skills"] ["retire" test-retired-id]]]
  (let [state-root (str tmp "/missing-initial-" action "-state")
   codex-skills (.toPath (io/file tmp (str "missing-initial-" action "-skills")))
   snapshot (fn [] (into (sorted-map) (for [id (keys (:skill-links initialization))]
  [id (resolved-link-target (.resolve codex-skills id))])))]
  (populate-initial-links! codex-skills initialization)
  (java.nio.file.Files/delete (.resolve codex-skills missing-id))
  (let [before (snapshot)]
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (check! (str "a missing declared " action " link aborts before adoption") (and (throws-containing? (str "initial Codex skill link is missing: " missing-id) (fn [] (north.agent-catalog/publish! activation initialization))) (= before (snapshot)) (not (java.nio.file.Files/exists (.toPath (io/file state-root "current")) (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS]))) (not (.exists (io/file state-root "codex-managed-skills.json"))) (not (.exists (io/file state-root "initialization-receipt.json"))))))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   initialization (north.agent-catalog/load-initialization! catalog initialization-path)
   activation (north.agent-catalog/compile-activation! catalog (:permissions initialization))
   state-root (str tmp "/temporary-collision-state")
   codex-skills (.toPath (io/file tmp "temporary-collision-skills"))
   collision (io/file state-root ".codex-managed-skills.initialization.tmp")]
  (populate-initial-links! codex-skills initialization)
  (.mkdirs (io/file state-root))
  (spit collision "user-owned temporary name\n")
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (check! "an unowned initialization temporary collision aborts before journaling" (and (throws-containing? "temporary path is already occupied" (fn [] (north.agent-catalog/publish! activation initialization))) (= "user-owned temporary name\n" (slurp collision)) (not (.exists (io/file state-root "initialization-transaction.json"))) (not (no-follow-exists? (.toPath (io/file state-root "current")))) (not (.exists (io/file state-root "codex-managed-skills.json"))) (not (.exists (io/file state-root "initialization-receipt.json")))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   initialization (north.agent-catalog/load-initialization! catalog initialization-path)
   activation (north.agent-catalog/compile-activation! catalog (assoc (:permissions initialization) "build-vs-reuse" "on"))
   state-root (str tmp "/rollback-state")
   codex-skills (.toPath (io/file tmp "rollback-skills"))
   skill-links (:skill-links initialization)
   adopted-target (get-in skill-links ["agent-policy" "target"])
   retired-target (get-in skill-links [test-retired-id "target"])
   adopted-path (.resolve codex-skills "agent-policy")
   retired-path (.resolve codex-skills test-retired-id)
   created-path (.resolve codex-skills "build-vs-reuse")]
  (populate-initial-links! codex-skills initialization)
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (check! "a failed adoption restores every replaced, created, and retired link" (and (throws-containing? "injected adoption failure" (fn [] (binding [north.agent-catalog/*codex-publication-stage!* (fn [stage id] (if (and (= stage :link-retired) (= id test-retired-id)) (do
  (throw (ex-info "injected adoption failure" {})))))]
  (north.agent-catalog/publish! activation initialization)))) (= adopted-target (resolved-link-target adopted-path)) (= retired-target (resolved-link-target retired-path)) (not (java.nio.file.Files/exists created-path (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS]))) (not (java.nio.file.Files/exists (.toPath (io/file state-root "current")) (into-array java.nio.file.LinkOption [java.nio.file.LinkOption/NOFOLLOW_LINKS]))) (not (.exists (io/file state-root "codex-managed-skills.json"))) (not (.exists (io/file state-root "initialization-receipt.json")))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   initialization (north.agent-catalog/load-initialization! catalog initialization-path)
   input-document (json/parse-string (slurp initialization-path))
   different-path (str tmp "/different-initialization.json")
   crash-runner (str tmp "/initialization-crash-runner.clj")
   cli (str root "/cli/config-cli.clj")
   repo-roots (json/generate-string (merge {"north" root "beagle" "/home/tom/code/beagle/main" "nixos-config" "/home/tom/code/nixos-config/main"} (some-> (System/getenv "NORTH_REPO_ROOTS") json/parse-string) {"north" root}))
   final-artifacts (new-final-artifacts)]
  (spit different-path (json/generate-string (assoc input-document "id" "different-cutover-input")))
  (spit crash-runner (str "(load-file " (pr-str (str root "/cli/agent-catalog.clj")) ")\n" "(let [wanted (keyword (System/getenv \"CRASH_STAGE\"))\n" "      wanted-id (System/getenv \"CRASH_ID\")]\n" "  (binding [north.agent-catalog/*codex-publication-stage!*\n" "            (fn [stage id]\n" "              (when (and (= wanted stage)\n" "                         (or (= \"\" wanted-id) (= wanted-id id)))\n" "                (.halt (Runtime/getRuntime) 86)))]\n" "    (north.agent-catalog/sync!\n" "     (System/getenv \"NORTH_AGENT_INITIALIZATION\"))))\n"))
  (doseq [[stage crash-id] [["manifest-temporary-staged" ""] ["receipt-temporary-staged" ""] ["current-transitioned" ""] ["link-transitioned" "agent-policy"] ["link-retired" test-retired-id] ["receipt-transitioned" ""] ["manifest-transitioned" ""]]]
  (let [case-name (str/replace stage #"-transitioned$" "")
   case-root (io/file tmp (str "crash-" case-name))
   cli-home (str (io/file case-root "home"))
   state-root (str (io/file case-root "state"))
   codex-skills (.toPath (io/file cli-home ".codex/skills"))
   transaction (io/file state-root "initialization-transaction.json")
   receipt-file (io/file state-root "initialization-receipt.json")
   manifest-file (io/file state-root "codex-managed-skills.json")
   env {"HOME" cli-home "NORTH_HOME" root "NORTH_AGENT_STATE_ROOT" state-root "NORTH_REPO_ROOTS" repo-roots}
   run-sync (fn [input] (p/shell {:out :string :err :string :continue true :extra-env (assoc env "NORTH_AGENT_INITIALIZATION" (or input ""))} "bb" cli "agents" "sync" "--json"))]
  (.mkdirs (io/file cli-home ".codex/skills/.system"))
  (spit (io/file cli-home ".codex/skills/.system/marker") "system\n")
  (.mkdirs (io/file cli-home ".codex/skills/user-owned"))
  (spit (io/file cli-home ".codex/skills/user-owned/marker") "user\n")
  (populate-initial-links! codex-skills initialization)
  (let [crashed (p/shell {:out :string :err :string :continue true :extra-env (merge env {"NORTH_AGENT_INITIALIZATION" initialization-path "CRASH_STAGE" stage "CRASH_ID" crash-id})} "bb" crash-runner)
   transaction-after-crash? (.isFile transaction)
   crash-state [(shallow-snapshot (.toPath (io/file state-root))) (shallow-snapshot codex-skills)]
   ordinary-before-replay (run-sync nil)
   ordinary-state [(shallow-snapshot (.toPath (io/file state-root))) (shallow-snapshot codex-skills)]
   different-before-replay (run-sync different-path)
   different-state [(shallow-snapshot (.toPath (io/file state-root))) (shallow-snapshot codex-skills)]
   replayed (run-sync initialization-path)
   _replayed (if (not (zero? (:exit replayed))) (do
  (throw (ex-info "initialization replay failed" {:stage stage :out (:out replayed) :err (:err replayed)}))))
   receipt (json/parse-string (slurp receipt-file))
   manifest (slurp manifest-file)
   artifacts [(get receipt "generationId") (get receipt "initializationDigest") manifest]
   ordinary-after-replay (run-sync nil)
   repeated (run-sync initialization-path)
   state-names (keys (shallow-snapshot (.toPath (io/file state-root))))
   codex-names (keys (shallow-snapshot codex-skills))]
  (if (nil? (deref final-artifacts)) (do
  (reset! final-artifacts artifacts)))
  (check! (str "process-crash replay converges after " stage) (and (= 86 (:exit crashed)) transaction-after-crash? (not (zero? (:exit ordinary-before-replay))) (str/includes? (:err ordinary-before-replay) "requires exact replay") (= crash-state ordinary-state) (not (zero? (:exit different-before-replay))) (str/includes? (:err different-before-replay) "requires the exact initialization input") (= ordinary-state different-state) (zero? (:exit replayed)) (= (deref final-artifacts) artifacts) (= "north.agent-initialization-receipt/v1" (get receipt "schema")) (not (.exists transaction)) (not-any? (fn [^String name] (or (str/includes? name ".initialization.tmp") (str/starts-with? name ".north-write-"))) state-names) (not-any? (fn [^String name] (str/starts-with? name ".north-initialization-")) codex-names) (= "system\n" (slurp (io/file cli-home ".codex/skills/.system/marker"))) (= "user\n" (slurp (io/file cli-home ".codex/skills/user-owned/marker"))) (zero? (:exit ordinary-after-replay)) (not (zero? (:exit repeated))) (str/includes? (:err repeated) "one-shot")))))))
  (let [catalog (north.agent-catalog/load-catalog!)
   activation (north.agent-catalog/compile-activation! catalog (initial-permissions catalog))
   state-root (str tmp "/symlink-state")
   target (.toPath (io/file tmp "legacy-skills-target"))
   codex-skills (.toPath (io/file tmp "legacy-codex/skills"))]
  (java.nio.file.Files/createDirectories target (make-array java.nio.file.attribute.FileAttribute 0))
  (java.nio.file.Files/createDirectories (.getParent codex-skills) (make-array java.nio.file.attribute.FileAttribute 0))
  (java.nio.file.Files/createSymbolicLink codex-skills target (make-array java.nio.file.attribute.FileAttribute 0))
  (with-redefs [north.agent-catalog/agents-root (constantly state-root) north.agent-catalog/codex-skills-dir (constantly codex-skills)] (check! "a legacy whole-directory Codex link is rejected without write-through" (and (throws-containing? "must be a real directory" (fn [] (north.agent-catalog/publish! activation))) (java.nio.file.Files/isSymbolicLink codex-skills) (empty? (seq (.listFiles (.toFile target))))))))
  (finally
    (delete-scratch! tmp)))

(let [results (deref checks)
   passed (count (filter (fn [result] (second result)) results))]
  (doseq [[label ok] results]
  (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nagent catalog: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
