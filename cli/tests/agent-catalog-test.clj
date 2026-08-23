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

(defn load-value [value]
  (let [path (str tmp "/fixture-" (java.util.UUID/randomUUID) ".json")]
    (spit path (json/generate-string value))
    (with-redefs [north.agent-catalog/catalog-path (constantly path)]
      (north.agent-catalog/load-catalog))))

(defn mutate-unit [catalog id f]
  (update catalog "units"
          (fn [units]
            (mapv #(if (= id (get % "id")) (f %) %) units))))

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
           (and (= 60 (count (:units catalog)))
                (every? (:by-id catalog)
                        ["coordination" "orchestration" "compose" "elicit"
                         "store-modeling" "code-as-facts"
                         "code-upstream-guard"])))
    (check "generation exposes the stable root contract"
           (and (= "north.agent-activation/v1" (get activation "schema"))
                (= "north.agent-catalog/v1" (get activation "catalogSchema"))
                (re-matches #"sha256:[0-9a-f]{64}" (get activation "catalogDigest"))
                (re-matches #"sha256:[0-9a-f]{64}" (get activation "generationId"))
                (= (set (map #(get % "id") (:units catalog)))
                   (set (keys (get activation "permissions"))))))
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
    (check "project-private units and inactive adapters seed inert"
           (every? #(and (= "off" (get-in by-id [% "permission"]))
                         (false? (get-in by-id [% "active"])))
                   ["code-as-facts" "code-upstream-guard"])))

  (check "duplicate UnitIds are rejected"
         (throws-containing?
          "duplicate catalog unit ids"
          #(load-value (update base "units" conj (first (get base "units"))))))
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
  (check "owner escapes are rejected"
         (throws-containing?
          "owner escapes"
          #(load-value (mutate-unit base "webdev"
                                    (fn [unit] (assoc-in unit ["owner" "path"] "../escape"))))))
  (check "unknown scopes are rejected"
         (throws-containing?
          "invalid scope"
          #(load-value (mutate-unit base "webdev"
                                    (fn [unit] (assoc unit "scope" "workspace"))))))
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
                    catalog (north.agent-catalog/seed-permissions catalog))
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
               (= (mapv #(get-in % ["ownerProvenance" "contentDigest"])
                        (get activation "units"))
                  (mapv #(get-in % ["ownerProvenance" "contentDigest"])
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
        (check "baseline instructions and inactive provider adapters are materialized"
               (every? #(.isFile (io/file current %))
                       ["instructions/shared/AGENTS.md"
                        "instructions/codex/AGENTS.md"
                        "instructions/code/AGENTS.md"
                        "provider-hooks/lib/harness-dial.sh"
                        "provider-hooks/logcompress.js"
                        "projects/beagle/hook/code-upstream-guard"]))
        (check "materialized generation never links back into owner trees"
               (not-any? #(java.nio.file.Files/isSymbolicLink %)
                         (with-open [walk (java.nio.file.Files/walk
                                          (.toPath (.getCanonicalFile current))
                                          (make-array java.nio.file.FileVisitOption 0))]
                           (vec (iterator-seq (.iterator walk)))))))))

  (let [cli (str root "/cli/config-cli.clj")
        cli-home (str tmp "/cli-home")
        cli-state (str tmp "/cli-state")
        repo-roots (json/generate-string
                    {"north" root
                     "beagle" "/home/tom/code/beagle/main"
                     "nixos-config" "/home/tom/code/nixos-config/main"})
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
