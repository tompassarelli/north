#!/usr/bin/env bb
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/agent-catalog.clj"))

(def checks (atom []))
(defn check [label value]
  (let [pass? (boolean value)]
    (swap! checks conj pass?)
    (println (format "  %s %s" (if pass? "✓" "✗") label))))

(defn throws-containing? [fragment f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (str/includes? (.getMessage error) fragment))))

(defn throws-message? [message f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo error
      (= message (.getMessage error)))))

(defn throws? [f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo _ true)))

(defn read-json [path]
  (json/parse-string (slurp path)))

(defn write-json! [path value]
  (spit path (str (json/generate-string value {:pretty true}) "\n")))

(defn copy-tree! [source target]
  (let [source (.toPath (io/file source))
        target (.toPath (io/file target))]
    (with-open [stream (java.nio.file.Files/walk
                        source (make-array java.nio.file.FileVisitOption 0))]
      (doseq [path (iterator-seq (.iterator stream))
              :let [relative (.relativize source path)
                    destination (.resolve target relative)]]
        (if (java.nio.file.Files/isDirectory
             path (make-array java.nio.file.LinkOption 0))
          (java.nio.file.Files/createDirectories
           destination (make-array java.nio.file.attribute.FileAttribute 0))
          (do
            (java.nio.file.Files/createDirectories
             (.getParent destination)
             (make-array java.nio.file.attribute.FileAttribute 0))
            (java.nio.file.Files/copy
             path destination
             (into-array java.nio.file.CopyOption
                         [java.nio.file.StandardCopyOption/REPLACE_EXISTING]))))))))

(defn delete-scratch! [path]
  (when (.exists (io/file path))
    (with-open [stream (java.nio.file.Files/walk
                        (.toPath (io/file path))
                        (make-array java.nio.file.FileVisitOption 0))]
      (doseq [entry (reverse (iterator-seq (.iterator stream)))]
        (java.nio.file.Files/deleteIfExists entry)))))

(defn tree-bytes [path]
  (let [root (.toPath (io/file path))]
    (when (java.nio.file.Files/exists
           root (make-array java.nio.file.LinkOption 0))
      (if (java.nio.file.Files/isDirectory
           root (make-array java.nio.file.LinkOption 0))
        (with-open [stream (java.nio.file.Files/walk
                            root (make-array java.nio.file.FileVisitOption 0))]
          (into (sorted-map)
                (for [entry (iterator-seq (.iterator stream))
                      :when (java.nio.file.Files/isRegularFile
                             entry (make-array java.nio.file.LinkOption 0))]
                  [(str (.relativize root entry))
                   (vec (java.nio.file.Files/readAllBytes entry))])))
        {"" (vec (java.nio.file.Files/readAllBytes root))}))))

(defn canonical-value [value]
  (cond
    (map? value) (into (sorted-map)
                       (map (fn [[key item]] [(str key) (canonical-value item)]))
                       value)
    (vector? value) (mapv canonical-value value)
    (sequential? value) (mapv canonical-value value)
    :else value))

(defn sha256 [text]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (.update digest (.getBytes text java.nio.charset.StandardCharsets/UTF_8))
    (str "sha256:"
         (apply str (map #(format "%02x" (bit-and 0xff %)) (.digest digest))))))

(defn repo-revision [path]
  (-> (shell/sh "git" "-C" path "rev-parse" "HEAD")
      :out str/trim))

(defn source-entry [sources role]
  (some #(when (= role (get % "role")) %) (get sources "sources")))

(defn owner-file [owner]
  (io/file (north.agent-catalog/repo-root (get owner "repo"))
           (get owner "path")))

(defn mutate-package [catalog id f]
  (update catalog "units"
          (fn [units]
            (mapv #(if (= id (get % "id")) (f %) %) units))))

(def sources-path (io/file root "agent-catalog/sources.json"))
(def scratch
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-agent-catalog-import-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))

(def package-root
  (.getCanonicalFile
   (io/file (or (System/getenv "AGENT_MACHINERY_HOME")
                (str (System/getenv "HOME") "/code/agent-machinery/main")))))
(def operator-root (io/file scratch "operator-owner"))

(defn distribution [type targets owner]
  {"type" type "targets" targets "owner" owner})

(defn fixture-activation [source-catalog package-catalog]
  (let [package-owner #(hash-map "repo" "agent-machinery" "path" %)
        north-owner #(hash-map "repo" "north" "path" %)
        package-units
        (into {}
              (for [unit (get package-catalog "units")
                    :let [id (get unit "id")
                          source (get unit "source")]]
                [id
                 {"distributions"
                  (cond
                    (= id "agent-machinery")
                    [(distribution "instructions" ["shared" "codex" "north" "bridge"]
                                   (package-owner "doctrine.md"))]

                    (= id "orchestration")
                    [(distribution "instructions" ["firn"]
                                   (package-owner "docs/routing.md"))]

                    (= id "agent-practice")
                    [(distribution "instructions" ["firn"]
                                   (package-owner "docs/method.md"))]

                    (= id "staffing")
                    [(distribution "skill" ["shared"]
                                   (package-owner "staffing"))
                     (distribution "agentTemplates" ["north" "claude"]
                                   (package-owner "agents"))]

                    :else
                    [(distribution "skill" ["shared"]
                                   (package-owner
                                    (str (.getParent (io/file source)))))])}]))
        source-units
        (into {}
              (for [unit (get source-catalog "units")
                    :let [id (get unit "id")
                          kind (get unit "kind")
                          owner (get unit "owner")]]
                [id
                 (cond->
                  {"distributions"
                   [(distribution
                     (case kind
                       "skill" "skill"
                       "hook" "hook"
                       "module" "instructions")
                     [(if (= kind "module") "shared" "codex")]
                     (if (= kind "skill")
                       (north-owner (str (.getParent (io/file (get owner "path")))))
                       owner))]}
                   (= id "agent-spawn-guard")
                   (assoc "supports" ["staffing"])
                   (#{"north-on-spawn" "north-on-tooluse" "north-on-stop"
                      "north-on-terminal" "north-mark-delegated"} id)
                   (assoc "supports" ["coordination"]))]))]
    (merge package-units source-units)))

(defn prepare-operator-fixture! [source-catalog package-catalog]
  (let [path (io/file operator-root "dotfiles/agents/catalog-config.json")
        activation (fixture-activation source-catalog package-catalog)
        ids (set (keys activation))
        roots (vec (concat ["agent-machinery" "coordination"]
                           (sort (remove #{"agent-machinery" "coordination"} ids))))]
    (.mkdirs (.getParentFile path))
    (write-json!
     path
     {"$schema" "catalog-config.schema.json"
      "schema" "north.agent-catalog-config/v1"
      "role" "operator"
      "rootOrder" roots
      "baselines"
      [{"id" "code-bootstrap"
        "owner" {"repo" "north" "path" "agent-catalog/instructions/code/AGENTS.md"}
        "targets" ["code"]}]
      "providerSupport"
      [{"id" "activation-gate"
        "owner" {"repo" "north" "path" "agent-runtime/hooks/lib/harness-dial.sh"}
        "path" "lib/harness-dial.sh"}]
      "registrations" {}
      "activation" activation})
    (doseq [args [["init" "-q"]
                  ["add" "dotfiles/agents/catalog-config.json"]
                  ["-c" "user.name=North fixture" "-c"
                   "user.email=north-fixture@example.invalid" "commit" "-qm"
                   "fixture operator catalog"]]]
      (let [{:keys [exit err]} (apply shell/sh "git" "-C" (str operator-root) args)]
        (when-not (zero? exit)
          (throw (ex-info "cannot prepare operator catalog fixture" {:err err})))))))

(println "agent catalog explicit package import")

(try
  (let [sources (read-json sources-path)
        source-catalog (read-json (io/file root "agent-catalog/north.json"))
        package-catalog (read-json (io/file package-root "catalog.json"))
        _ (prepare-operator-fixture! source-catalog package-catalog)
        fixture-repo-root
        (fn [repo]
          (case repo
            "north" root
            "agent-machinery" (str package-root)
            "nixos-config" (str operator-root)
            (str (System/getenv "HOME") "/code/" repo "/main")))]
    (with-redefs [north.agent-catalog/repo-root fixture-repo-root]
      (if-not (.isFile sources-path)
        (check "catalog loading starts from the explicit source document" false)
        (let [sources sources
          package-entry (source-entry sources "package")
          source-entry-value (source-entry sources "source")
          operator-entry (source-entry sources "operator")
          package-owner (get package-entry "owner")
          package-catalog-path (owner-file package-owner)
          package-root (.getParentFile package-catalog-path)
          package-catalog (read-json package-catalog-path)
          package-by-id (into {} (map (juxt #(get % "id") identity))
                              (get package-catalog "units"))
          effective (north.agent-catalog/load-catalog)
          portable-ids (set (map #(get % "id") (get package-catalog "units")))
          effective-by-id (:by-id effective)]
      (check "sources declare exactly one package, source, and operator import"
             (and (= "north.agent-catalog-sources/v1" (get sources "schema"))
                  (= [{"id" "north" "role" "source"
                       "owner" {"repo" "north" "path" "agent-catalog/north.json"}}
                      {"id" "agent-machinery" "role" "package"
                       "owner" {"repo" "agent-machinery" "path" "catalog.json"}}
                      {"id" "operator" "role" "operator"
                       "owner" {"repo" "nixos-config"
                                "path" "dotfiles/agents/catalog-config.json"}}]
                     (get sources "sources"))))
      (check "the portable import is the export-only agent-machinery contract"
             (and (= "agent-machinery.catalog/v1"
                     (get package-catalog "schema"))
                  (= #{"$schema" "schema" "package" "units" "assets" "contracts"}
                     (set (keys package-catalog)))
                  (= ["orchestration" "agent-practice"]
                     (get-in package-by-id ["agent-machinery" "members"]))
                  (= ["staffing" "compose"]
                     (get-in package-by-id ["orchestration" "members"]))
                  (every? #(empty? (select-keys % ["supports" "distributions"
                                                   "providerAdapter" "active"]))
                          (get package-catalog "units"))))

      (check "the imported portable UnitId inventory is exact"
             (= #{"agent-machinery" "orchestration" "agent-practice"
                  "staffing" "compose" "build-vs-reuse" "external-code"
                  "greenfield" "planning" "prior-art" "production-hardening"
                  "program-craftsmanship" "program-stewardship"
                  "rust-development" "skill-maintenance" "terse" "verification"}
                portable-ids))

      (let [original-repo-root north.agent-catalog/repo-root
            case-root (io/file scratch "explicit-only")
            sources-copy (io/file case-root "sources.json")
            package-copy (io/file case-root "agent-machinery")
            decoy (io/file package-copy "unreferenced/catalog.json")]
        (copy-tree! package-root package-copy)
        (.mkdirs (.getParentFile decoy))
        (write-json! decoy
                     {"$schema" "./catalog.schema.json"
                      "schema" "agent-machinery.catalog/v1"
                      "package" {"name" "@tompassarelli/agent-machinery"
                                 "version" "0.1.0"
                                 "license" "MIT OR Apache-2.0"}
                      "units" [{"id" "scanned-decoy" "kind" "skill"
                                "source" "SKILL.md"}]
                      "assets" [] "contracts" []})
        (write-json! sources-copy sources)
        (with-redefs [north.agent-catalog/catalog-path #(str sources-copy)
                      north.agent-catalog/repo-root
                      (fn [repo]
                        (if (= repo (get package-owner "repo"))
                          (str package-copy)
                          (original-repo-root repo)))]
          (check "unreferenced package-shaped files inside the configured package root are not scanned"
                 (nil? (get (:by-id (north.agent-catalog/load-catalog))
                            "scanned-decoy")))))

      (let [sources-copy (io/file scratch "missing-source.json")]
        (write-json! sources-copy
                     (update sources "sources"
                             (fn [entries]
                               (mapv #(if (= "package" (get % "role"))
                                        (assoc-in % ["owner" "path"] "missing.json")
                                        %)
                                     entries))))
        (with-redefs [north.agent-catalog/catalog-path #(str sources-copy)]
          (check "an explicitly imported missing package source fails closed"
                 (throws-containing? "does not exist"
                                     north.agent-catalog/load-catalog))))

      (let [sources-copy (io/file scratch "escaping-source.json")]
        (write-json! sources-copy
                     (update sources "sources"
                             (fn [entries]
                               (mapv #(if (= "package" (get % "role"))
                                        (assoc-in % ["owner" "path"] "../catalog.json")
                                        %)
                                     entries))))
        (with-redefs [north.agent-catalog/catalog-path #(str sources-copy)]
          (check "an explicitly imported escaping package source fails closed"
                 (throws-containing? "escapes"
                                     north.agent-catalog/load-catalog))))

      (let [sources-copy (io/file scratch "duplicate-source.json")]
        (write-json! sources-copy
                     (update sources "sources" conj
                             (assoc package-entry "id" "package-shadow")))
        (with-redefs [north.agent-catalog/catalog-path #(str sources-copy)]
          (check "duplicate package source declarations are rejected"
                 (throws-message? "agent catalog sources must name exactly three owners"
                                  north.agent-catalog/load-catalog))))

      (let [original-repo-root north.agent-catalog/repo-root
            run-package-case
            (fn [name transform expected]
              (let [case-root (io/file scratch name)
                    sources-copy (io/file case-root "sources.json")
                    package-copy (io/file case-root "agent-machinery")]
                (copy-tree! package-root package-copy)
                (write-json! (io/file package-copy "catalog.json")
                             (transform package-catalog))
                (write-json! sources-copy sources)
                (with-redefs [north.agent-catalog/catalog-path #(str sources-copy)
                              north.agent-catalog/repo-root
                              (fn [repo]
                                (if (= repo (get package-owner "repo"))
                                  (str package-copy)
                                  (original-repo-root repo)))]
                  (if expected
                    (throws-containing? expected north.agent-catalog/load-catalog)
                    (throws? north.agent-catalog/load-catalog)))))]
        (check "duplicate imported UnitIds are rejected"
               (run-package-case
                "duplicate-id"
                #(update % "units" conj (first (get % "units")))
                "agent machinery unit ids contains duplicates: agent-machinery"))
        (check "package and local kind collisions are rejected"
               (run-package-case
                "kind-collision"
                #(update % "units" conj
                         {"id" "coordination" "kind" "skill"
                          "source" "skills/compose/SKILL.md"})
                "competing catalog declarations: coordination"))
        (check "cycles in imported module membership are rejected"
               (run-package-case
                "module-cycle"
                #(mutate-package % "orchestration"
                                 (fn [unit]
                                   (update unit "members" conj "agent-machinery")))
                "catalog module cycle: agent-machinery -> orchestration -> agent-machinery"))
        (check "package contracts must classify raw schemas as structural"
               (run-package-case
                "contract-schema-scope"
                #(assoc-in % ["contracts" 0 "schemaScope"] "semantic")
                "agent machinery contract 0 has an invalid schema scope"))
        (check "package contracts must name the composed validator"
               (run-package-case
                "contract-validator"
                #(assoc-in % ["contracts" 0 "validator"] "validateSchema")
                "agent machinery contract 0 has an invalid validator")))

      (let [portable-activation
            (north.agent-catalog/compile-activation
             effective
             (reduce #(assoc %1 %2 "on")
                     (north.agent-catalog/default-permissions effective)
                     ["agent-machinery" "orchestration" "staffing" "compose"]))
            active (set (for [unit (get portable-activation "units")
                              :when (get unit "active")]
                          (get unit "id")))]
        (check "portable orchestration activates without North coordination or hooks"
               (and (= #{"agent-machinery" "orchestration" "staffing" "compose"}
                       active)
                    (not (active "coordination"))
                    (not-any? #(and (= "hook" (get % "kind"))
                                    (get % "active"))
                              (get portable-activation "units")))))

      (let [composed-activation
            (north.agent-catalog/compile-activation
             effective
             (assoc (reduce #(assoc %1 %2 "on")
                            (north.agent-catalog/default-permissions effective)
                            portable-ids)
                    "coordination" "on"))
            active (set (for [unit (get composed-activation "units")
                              :when (get unit "active")]
                          (get unit "id")))]
        (check "North composes its local coordination root separately"
               (and (= ["agent-machinery" "coordination"]
                       (vec (take 2 (:root-order effective))))
                    (every? active (conj portable-ids "coordination")))))

      (let [portable (map effective-by-id portable-ids)
            old-prefixes ["orchestration/" "profiles/tom/skills/"]]
        (check "portable units have one agent-machinery source authority"
               (and (= portable-ids (set (map #(get % "id") portable)))
                    (every? #(= (get package-owner "repo")
                                (get-in % ["owner" "repo"]))
                            portable)
                    (= (count portable) (count (distinct (map #(get % "id") portable))))))
        (check "effective portable owners contain no retired North source paths"
               (not-any? (fn [unit]
                           (let [owner (get unit "owner")]
                             (and (= "north" (get owner "repo"))
                                  (some #(str/starts-with? (get owner "path") %)
                                        old-prefixes))))
                         portable)))

      (let [expected-digest
            (sha256 (json/generate-string (canonical-value (:catalog effective))))
            revisions (into {}
                            (for [repo ["north" "agent-machinery" "nixos-config"]]
                              [repo (repo-revision (north.agent-catalog/repo-root repo))]))
            portable (map effective-by-id portable-ids)]
        (check "the effective digest covers the composed effective catalog"
               (= expected-digest (:digest effective)))
        (check "every imported owner carries exact stable provenance"
               (every?
                (fn [unit]
                  (let [owner (get unit "owner")
                        provenance (get unit "ownerProvenance")]
                    (and (= #{"owner" "revision" "contentDigest"}
                            (set (keys provenance)))
                         (= owner (get provenance "owner"))
                         (= (get revisions (get owner "repo"))
                            (get provenance "revision"))
                         (re-matches #"sha256:[0-9a-f]{64}"
                                     (get provenance "contentDigest")))))
                portable))
        (check "repeat imports preserve digest and owner provenance"
               (let [reloaded (north.agent-catalog/load-catalog)]
                 (and (= (:digest effective) (:digest reloaded))
                      (= (into {} (map (juxt #(get % "id")
                                             #(get % "ownerProvenance"))) portable)
                         (into {} (map (juxt #(get % "id")
                                             #(get % "ownerProvenance")))
                               (map (:by-id reloaded) portable-ids)))))))

      (let [stage-generation (ns-resolve 'north.agent-catalog 'stage-generation!)
            state-root (io/file scratch "generation-state")
            activation
            (north.agent-catalog/compile-activation
             effective
             (reduce #(assoc %1 %2 "on")
                     (north.agent-catalog/default-permissions effective)
                     portable-ids))
            generation
            (with-redefs [north.agent-catalog/agents-root #(str state-root)]
              (stage-generation activation))
            generated-skills (io/file (str generation) "skills/shared")
            generated-templates (io/file (str generation)
                                         "agent-templates/north/staffing")
            generated-doctrine (io/file (str generation)
                                        "instructions/north/AGENTS.md")]
        (check "the canonical generation seam materializes a nonempty North-hosted projection"
               (and stage-generation
                    (seq (tree-bytes generated-skills))
                    (seq (tree-bytes generated-templates))
                    (seq (tree-bytes generated-doctrine))))
        (check "North-hosted portable skills are exact copies of independent package sources"
               (every?
                (fn [id]
                  (let [owner (get-in effective-by-id [id "owner"])
                        direct (.getParentFile (owner-file owner))
                        generated (io/file generated-skills id)]
                    (= (tree-bytes direct) (tree-bytes generated))))
                (for [id portable-ids
                      :when (= "skill" (get-in effective-by-id [id "kind"]))]
                  id)))
        (check "North-hosted agent templates are exact copies of the package output"
               (= (tree-bytes (io/file package-root "agents"))
                  (tree-bytes generated-templates)))
        (check "North-hosted instructions embed the exact package doctrine"
               (= (str "<!-- agent-machinery:doctrine.md -->\n"
                       (slurp (io/file package-root "doctrine.md")))
                  (slurp generated-doctrine))))))))
  (finally
    (delete-scratch! scratch)))

(let [total (count @checks)
      passed (count (filter true? @checks))]
  (println (format "%s %d/%d" (if (= total passed) "PASS" "FAIL") passed total))
  (System/exit (if (= total passed) 0 1)))
