#!/usr/bin/env bb
;; Scratch-only contract for `north config skills`. No case can reach the live
;; profile, state file, farm, provider homes, or account homes.
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def tmp-dir
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-config-skills-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def scratch-home (str tmp-dir "/home"))
(def state (str tmp-dir "/harness.conf"))
(def profile (str tmp-dir "/profile/skills"))
(def farm (str scratch-home "/.local/state/north/skills"))
(def generations (str farm ".d"))
(def checks (atom []))

(def skill-specs
  [["alpha" "graph"]
   ["beta" "graph"]
   ["gamma" "graph"]
   ["firn" "nixos"]
   ["plain" nil]])

(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn skill-text [id category]
  (str "---\n"
       "name: " id "\n"
       (when category (str "category: " category "\n"))
       "description: fixture\n"
       "---\n\n"
       "# " id "\n"))

(defn install-profile! []
  (doseq [[id category] skill-specs]
    (let [path (str profile "/" id "/SKILL.md")]
      (io/make-parents path)
      (spit path (skill-text id category)))))

(defn run-cli [& args]
  (apply p/shell
         {:out :string
          :err :string
          :continue true
          :extra-env {"HOME" scratch-home
                      "NORTH_HOME" root
                      "NORTH_HARNESS_STATE" state
                      "NORTH_SKILLS_PROFILE" profile
                      "NORTH_SKILLS_FARM" farm}}
         (into ["bb" cli "skills"] args)))

(defn stored [key]
  (let [prefix (str key "=")]
    (some->> (when (.isFile (io/file state)) (slurp state))
             str/split-lines
             (filter #(str/starts-with? % prefix))
             last
             (#(subs % (count prefix))))))

(defn seed-state! [text]
  (io/make-parents state)
  (spit state text))

(defn farm-entries [path]
  (let [dir (io/file path)]
    (if (.isDirectory dir)
      (->> (or (.list dir) (make-array String 0)) set)
      #{})))

(defn farm-pointer []
  (let [path (.toPath (io/file farm))]
    (when (java.nio.file.Files/isSymbolicLink path)
      (str (java.nio.file.Files/readSymbolicLink path)))))

(defn link-target [path]
  (let [link (.toPath (io/file path))]
    (when (java.nio.file.Files/isSymbolicLink link)
      (.toAbsolutePath
       (.normalize
        (.resolve (.getParent link)
                  (java.nio.file.Files/readSymbolicLink link)))))))

(defn projection-case []
  (let [all-ids (set (map first skill-specs))
        synced (run-cli "sync")]
    (check "default sync succeeds" (zero? (:exit synced)))
    (check "default sync publishes the complete source set"
           (= all-ids (farm-entries farm)))
    (check "farm entries point back into the profile"
           (every?
            (fn [id]
              (= (.toAbsolutePath (.normalize (.toPath (io/file profile id))))
                 (link-target (str farm "/" id))))
            all-ids))
    (let [disabled (run-cli "off" "firn")]
      (check "skills off firn succeeds and synchronizes" (zero? (:exit disabled)))
      (check "item state is durable" (= "off" (stored "skills.skill.firn")))
      (check "only firn is removed"
             (= (disj all-ids "firn") (farm-entries farm))))))

(defn atomic-case []
  (let [synced (run-cli "sync")
        baseline-ready? (zero? (:exit synced))
        before-pointer (farm-pointer)
        before-entries (farm-entries farm)
        broken (str profile "/broken/SKILL.md")]
    (io/make-parents broken)
    (spit broken "---\nname: not-broken\ndescription: invalid fixture\n---\n")
    (let [failed (run-cli "sync")]
      (check "initial farm sync succeeds" baseline-ready?)
      (check "invalid inventory makes sync fail"
             (and baseline-ready? (not (zero? (:exit failed)))))
      (check "failed sync leaves the selected generation unchanged"
             (and baseline-ready? (= before-pointer (farm-pointer))))
      (check "failed sync exposes no partial farm"
             (and baseline-ready? (= before-entries (farm-entries farm))))
      (check "failed skill never appears"
             (and baseline-ready?
                  (not (contains? (farm-entries farm) "broken")))))))

(defn precedence-case []
  (seed-state!
   (str "skills=off\n"
        "skills.cat.graph=on\n"
        "skills.cat.uncategorized=on\n"
        "skills.skill.alpha=off:until=2020-01-01T00:00:00Z\n"
        "skills.skill.beta=off:until=2099-01-01T00:00:00Z\n"))
  (let [synced (run-cli "sync")
        listed (run-cli "list")]
    (check "precedence fixture sync succeeds" (zero? (:exit synced)))
    (check "resolved farm follows item/category/all precedence and TTL"
           (= #{"alpha" "gamma" "plain"} (farm-entries farm)))
    (check "list reports expired item TTL as on/item"
           (re-find #"(?m)^alpha\s+graph\s+on\s+item\b" (:out listed)))
    (check "list reports future item TTL as off/item"
           (re-find #"(?m)^beta\s+graph\s+off\s+item\b" (:out listed)))
    (check "list reports category provenance"
           (and (re-find #"(?m)^gamma\s+graph\s+on\s+category\b" (:out listed))
                (re-find #"(?m)^plain\s+uncategorized\s+on\s+category\b" (:out listed))))
    (check "list reports all provenance"
           (re-find #"(?m)^firn\s+nixos\s+off\s+all\b" (:out listed)))))

(defn link! [link target]
  (io/make-parents link)
  (java.nio.file.Files/createSymbolicLink
   (.toPath (io/file link))
   (.toPath (io/file target))
   (make-array java.nio.file.attribute.FileAttribute 0)))

(defn aggregate-case []
  (let [agents (str scratch-home "/.agents/skills")
        claude (str scratch-home "/.claude/skills")
        account (str scratch-home "/.local/state/north/accounts/anthropic/fixture/skills")
        all-ids (set (map first skill-specs))
        synced (run-cli "sync")]
    (link! agents farm)
    (link! claude agents)
    (link! account claude)
    (check "aggregate default sync succeeds" (zero? (:exit synced)))
    (check "agents, Claude, and account projections all see the complete farm"
           (every? #(= all-ids (farm-entries %)) [agents claude account]))
    (let [disabled (run-cli "off" "firn")
          expected (disj all-ids "firn")]
      (check "aggregate firn disable succeeds" (zero? (:exit disabled)))
      (check "agents, Claude, and account projections all follow the new farm atomically"
             (every? #(= expected (farm-entries %)) [agents claude account])))))

(def requested (or (first *command-line-args*) "projection"))

(try
  (install-profile!)
  (case requested
    "projection" (projection-case)
    "atomic" (atomic-case)
    "precedence" (precedence-case)
    "aggregate" (aggregate-case)
    (do
      (binding [*out* *err*]
        (println "usage: bb cli/tests/config-skills-test.clj [projection|atomic|precedence|aggregate]"))
      (System/exit 2)))
  (finally
    (doseq [file (reverse (file-seq tmp-dir))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nconfig skills (%s): %d / %d PASS"
                   requested passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
