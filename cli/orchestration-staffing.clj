;; One fail-closed boundary for Orchestration's canonical v2 staffing catalog.
(ns north.orchestration-staffing
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def stock-preset-names
  #{"executor" "curator" "implementer" "integrator" "designer" "director" "scout"
    "analyst" "guardian" "reviewer" "verifier" "judge" "scientist" "experimenter"
    "team-lead" "program" "portfolio"})

(def stock-authoring-roles #{"executor" "curator" "implementer" "integrator" "experimenter"})
(def preset-capabilities
  #{"filesystem.read" "filesystem.search" "filesystem.write" "shell"
    "shell.readonly" "web" "coordination"})
(def exact-wire-vocabulary
  {"taskGrades" #{"novice" "junior" "mid" "senior" "staff" "principal" "distinguished"}
   "semanticTiers" #{"economy" "standard" "senior" "frontier"}
   "deliberations" #{"low" "medium" "high" "xhigh" "max"}
   "topologies" #{"worker" "orchestrator"}
   "postures" #{"explore" "evaluate" "deliver" "preserve" "prune"}
   "capabilities" #{"filesystem.read" "filesystem.search" "filesystem.write" "shell"
                    "shell.readonly" "web" "coordination"}})

;; Derive the repo root from THIS file's own location (<root>/cli/…), never
;; user.dir: an MCP subprocess inherits whatever cwd its parent had, which
;; resolved one directory too high and produced the obsolete sibling location.
(def ^:private this-root
  (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile .getPath))

(defn- orchestration-root []
  (or (System/getenv "NORTH_ORCHESTRATION_HOME")
      (str (or (System/getenv "NORTH_HOME") this-root (System/getProperty "user.dir")) "/orchestration")))

(defn catalog-path []
  (or (System/getenv "ORCHESTRATION_STAFFING_CATALOG")
      (str (orchestration-root) "/staffing/catalog.json")))


(defn- provider-supports-route? [provider tier reasoning]
  (try
    (let [catalog (json/parse-string
                   (slurp (io/file (orchestration-root) "providers" (str provider ".json")))
                   false)
          entry (get-in catalog ["tiers" tier])
          levels (or (get entry "efforts") (get entry "reasoning"))]
      (boolean (some #{reasoning} levels)))
    (catch Exception _ false)))

(defn unsupported-route-problem [tier reasoning]
  (when (and tier reasoning
             (not (some #(provider-supports-route? % tier reasoning)
                        ["anthropic" "openai"])))
    (str "unsupported route: tier '" tier "' with deliberation '" reasoning
         "' resolves through no provider catalog")))

(defn posture-capability-problem [posture capabilities]
  (cond
    (and (= "preserve" posture)
         (or (contains? capabilities "filesystem.write") (contains? capabilities "shell")))
    "preserve posture requires a non-authoring capability boundary"

    (and (= "prune" posture)
         (or (not (contains? capabilities "filesystem.write"))
             (not (contains? capabilities "shell"))))
    "prune posture requires filesystem.write and shell capabilities"))

(defn- exact-keys! [value allowed required label path]
  (when-not (map? value)
    (throw (ex-info (str "staffing catalog: " label " must be an object")
                    {:path path :label label})))
  (let [actual (set (keys value))
        unknown (seq (sort (remove allowed actual)))
        missing (seq (sort (remove actual required)))]
    (when unknown
      (throw (ex-info (str "staffing catalog: " label " has unknown field(s): "
                           (str/join ", " unknown))
                      {:path path :unknown unknown})))
    (when missing
      (throw (ex-info (str "staffing catalog: " label " is missing field(s): "
                           (str/join ", " missing))
                      {:path path :missing missing})))))

(defn- unique-strings! [value label path]
  (when-not (and (vector? value) (seq value) (every? #(and (string? %) (seq %)) value)
                 (= (count value) (count (set value))))
    (throw (ex-info (str "staffing catalog: " label " must contain unique non-empty strings")
                    {:path path :label label})))
  value)

(defn normalize-catalog [catalog path]
  (let [version (get catalog "version")
        top-fields #{"$schema" "version" "vocabulary" "defaults" "presets" "aliases"}
        required-top #{"version" "vocabulary" "defaults" "presets" "aliases"}
        vocab-fields #{"taskGrades" "semanticTiers" "deliberations" "topologies" "postures" "capabilities"}
        default-fields #{"taskGrade" "tier" "deliberation" "topology" "posture"}
        preset-fields #{"name" "taskGrade" "tier" "deliberation" "topology" "posture"
                        "capabilities" "tagline" "description"}]
    (when-not (= 2 version)
      (throw (ex-info "staffing catalog: version must be 2" {:path path :version version})))
    (exact-keys! catalog top-fields required-top "top level" path)
    (exact-keys! (get catalog "vocabulary") vocab-fields vocab-fields "vocabulary" path)
    (doseq [axis vocab-fields]
      (unique-strings! (get-in catalog ["vocabulary" axis])
                       (str "vocabulary." axis) path)
      (when-not (= (get exact-wire-vocabulary axis)
                   (set (get-in catalog ["vocabulary" axis])))
        (throw (ex-info (str "Orchestration wire vocabulary drift at " path ": " axis)
                        {:path path :axis axis}))))
    (exact-keys! (get catalog "defaults") default-fields default-fields "defaults" path)
    (doseq [[field axis] [["taskGrade" "taskGrades"] ["tier" "semanticTiers"]
                          ["deliberation" "deliberations"] ["topology" "topologies"]
                          ["posture" "postures"]]]
      (when-not (some #{(get-in catalog ["defaults" field])}
                      (get-in catalog ["vocabulary" axis]))
        (throw (ex-info (str "staffing catalog: invalid defaults." field)
                        {:path path :field field}))))
    (let [presets (get catalog "presets")]
      (when-not (and (vector? presets) (seq presets) (vector? (get catalog "aliases")))
        (throw (ex-info (str "invalid Orchestration staffing catalog at " path)
                        {:path path :version version})))
      (doseq [preset presets]
        (exact-keys! preset preset-fields preset-fields
                     (str "preset " (or (get preset "name") "<unknown>")) path)
        (doseq [[field axis] [["taskGrade" "taskGrades"] ["tier" "semanticTiers"]
                              ["deliberation" "deliberations"] ["topology" "topologies"]
                              ["posture" "postures"]]]
          (when-not (some #{(get preset field)} (get-in catalog ["vocabulary" axis]))
            (throw (ex-info (str (get preset "name") ": invalid " field)
                            {:path path :preset (get preset "name") :field field}))))
        (unique-strings! (get preset "capabilities")
                         (str (get preset "name") ".capabilities") path)
        (when (some #(not (preset-capabilities %)) (get preset "capabilities"))
          (throw (ex-info (str (get preset "name") ": noncanonical capability")
                          {:path path :preset (get preset "name")})))
        (doseq [field ["tagline" "description"]]
          (when-not (and (string? (get preset field))
                         (not (str/blank? (get preset field))))
            (throw (ex-info (str (get preset "name") ": missing " field)
                            {:path path :preset (get preset "name") :field field})))))
    (let [names (mapv #(get % "name") presets)
          known (set names)]
      (when (or (some nil? names) (not= (count names) (count known)))
        (throw (ex-info (str "invalid or duplicate Orchestration preset name at " path)
                        {:path path :names names})))
      (when-not (= stock-preset-names known)
        (throw (ex-info (str "Orchestration stock preset set drift at " path)
                        {:path path :expected stock-preset-names :actual known})))
      (let [orchestrators (->> presets
                               (filter #(= "orchestrator" (get % "topology")))
                               (mapv #(get % "name")))]
        (when-not (= #{"director" "team-lead" "program" "portfolio"} (set orchestrators))
          (throw (ex-info (str "Orchestration stock topology drift at " path
                               ": orchestrator topology is the director plus the scope ladder")
                          {:path path :orchestrators orchestrators}))))
      (when (seq (get catalog "aliases"))
        (throw (ex-info (str "Orchestration stock alias drift at " path
                             ": canonical release has no aliases")
                        {:path path :aliases (get catalog "aliases")})))
      (doseq [preset presets]
        (let [name (get preset "name")
              capabilities (set (get preset "capabilities"))]
          (when-not (and (capabilities "filesystem.read")
                         (capabilities "filesystem.search"))
            (throw (ex-info (str "Orchestration stock role " name
                                 " must retain read and search authority")
                            {:path path :preset name})))
          (if (stock-authoring-roles name)
            (when-not (and (capabilities "filesystem.write")
                           (capabilities "shell"))
              (throw (ex-info (str "Orchestration stock authoring role " name
                                   " must retain write and shell authority")
                              {:path path :preset name})))
            (when (or (capabilities "filesystem.write")
                      (capabilities "shell")
                      (not (capabilities "shell.readonly")))
              (throw (ex-info (str "Orchestration stock nonauthoring role " name
                                   " must remain read-only")
                              {:path path :preset name}))))
          (when-not (= (contains? #{"director" "team-lead" "program" "portfolio"} name)
                       (boolean (capabilities "coordination")))
            (throw (ex-info "Orchestration stock coordination authority belongs to the orchestrator ladder"
                            {:path path :preset name})))
          (when (and (capabilities "shell") (capabilities "shell.readonly"))
            (throw (ex-info (str name ": shell and shell.readonly are mutually exclusive")
                            {:path path :preset name})))
          (when-let [problem (posture-capability-problem (get preset "posture") capabilities)]
            (throw (ex-info (str name ": " problem) {:path path :preset name})))))
      (doseq [alias (get catalog "aliases")]
        (when-not (and (string? (get alias "name"))
                       (contains? known (get alias "target")))
          (throw (ex-info (str "invalid Orchestration staffing alias at " path)
                          {:path path :alias alias}))))
      catalog))))

(defn load-catalog
  ([] (load-catalog (catalog-path)))
  ([path]
   (let [file (io/file path)]
     (normalize-catalog (json/parse-string (slurp file)) (.getPath file)))))

(defn presets-by-name [catalog]
  (into {} (map (juxt #(get % "name") identity) (get catalog "presets"))))
