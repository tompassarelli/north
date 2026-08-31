(ns north.orchestration-staffing
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def stock-preset-names #{"executor" "curator" "implementer" "integrator" "designer" "director" "scout" "analyst" "guardian" "reviewer" "verifier" "judge" "scientist" "team-lead" "program" "portfolio"})

(def stock-authoring-roles #{"executor" "curator" "implementer" "integrator"})

(def stock-orchestrator-roles #{"director" "team-lead" "program" "portfolio"})

(def preset-capabilities #{"filesystem.read" "filesystem.search" "filesystem.write" "shell" "shell.readonly" "web" "coordination"})

(def route-axis-fields [["taskGrade" "taskGrades"] ["capabilityFloor" "capabilityFloors"] ["serviceClass" "serviceClasses"] ["deliberation" "deliberations"] ["topology" "topologies"] ["posture" "postures"]])

(def exact-wire-vocabulary {"taskGrades" #{"novice" "junior" "mid" "senior" "staff" "principal" "distinguished"} "capabilityFloors" #{"baseline" "standard" "advanced" "frontier"} "serviceClasses" #{"economy" "fast" "balanced" "premium"} "deliberations" #{"low" "medium" "high" "xhigh" "max"} "topologies" #{"worker" "orchestrator"} "postures" #{"explore" "evaluate" "deliver" "preserve" "prune"} "capabilities" #{"filesystem.read" "filesystem.search" "filesystem.write" "shell" "shell.readonly" "web" "coordination"}})

(def ^:private this-root (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile .getPath))

(defn- ^String agent-machinery-root []
  (let [configured (System/getenv "AGENT_MACHINERY_HOME")]
  (if (string? configured) configured (str this-root "/agent-machinery"))))

(defn- ^String agent-runtime-root []
  (let [configured (System/getenv "NORTH_AGENT_RUNTIME_HOME")]
  (if (string? configured) configured (str (or (System/getenv "NORTH_HOME") this-root (System/getProperty "user.dir")) "/agent-runtime/orchestration"))))

(defn ^String catalog-path []
  (let [configured (System/getenv "ORCHESTRATION_STAFFING_CATALOG")]
  (if (string? configured) configured (str (agent-machinery-root) "/staffing/catalog.json"))))

(defn- ^Boolean provider-supports-route? [^String provider tier reasoning]
  (try
  (let [catalog (json/parse-string (slurp (io/file (agent-runtime-root) "providers" (str provider ".json"))) false)
   entry (get-in catalog ["tiers" tier])
   levels (or (get entry "efforts") (get entry "reasoning"))]
  (boolean (some #{reasoning} levels)))
  (catch Exception _
    false)))

(defn unsupported-route-problem [tier reasoning]
  (if (and tier reasoning (not (some (fn [^String provider] (provider-supports-route? provider tier reasoning)) ["anthropic" "openai"]))) (do
  (str "unsupported route: tier '" tier "' with deliberation '" reasoning "' resolves through no provider catalog"))))

(defn posture-capability-problem [posture capabilities]
  (let [capability-set (set capabilities)]
  (cond
  (and (= "preserve" posture) (or (contains? capability-set "filesystem.write") (contains? capability-set "shell"))) "preserve posture requires a non-authoring capability boundary"
  (and (= "prune" posture) (or (not (contains? capability-set "filesystem.write")) (not (contains? capability-set "shell")))) "prune posture requires filesystem.write and shell capabilities")))

(defn- exact-keys [value allowed required ^String label ^String path]
  (if (not (map? value)) (do
  (throw (ex-info (str "delegation composition catalog: " label " must be an object") {:path path :label label}))))
  (let [actual (set (keys value))
   unknown (seq (sort (filter (fn [field] (not (contains? allowed field))) actual)))
   missing (seq (sort (filter (fn [field] (not (contains? actual field))) required)))]
  (if unknown (do
  (throw (ex-info (str "delegation composition catalog: " label " has unknown field(s): " (str/join ", " unknown)) {:path path :unknown unknown}))))
  (if missing (do
  (throw (ex-info (str "delegation composition catalog: " label " is missing field(s): " (str/join ", " missing)) {:path path :missing missing}))))))

(defn- unique-strings [value ^String label ^String path]
  (if (not (and (vector? value) (seq value) (every? (fn [item] (and (string? item) (boolean (seq item)))) value) (= (count value) (count (set value))))) (do
  (throw (ex-info (str "delegation composition catalog: " label " must contain unique non-empty strings") {:path path :label label}))))
  value)

(defn normalize-catalog [catalog ^String path]
  (let [version (get catalog "version")
   top-fields #{"$schema" "version" "vocabulary" "defaults" "presets"}
   required-top #{"version" "vocabulary" "defaults" "presets"}
   vocab-fields #{"taskGrades" "capabilityFloors" "serviceClasses" "deliberations" "topologies" "postures" "capabilities"}
   default-fields #{"taskGrade" "capabilityFloor" "serviceClass" "deliberation" "topology" "posture"}
   preset-fields #{"name" "taskGrade" "capabilityFloor" "serviceClass" "deliberation" "topology" "posture" "capabilities" "tagline" "description"}]
  (if (not (= 3 version)) (do
  (throw (ex-info "delegation composition catalog: version must be 3" {:path path :version version}))))
  (exact-keys catalog top-fields required-top "top level" path)
  (exact-keys (get catalog "vocabulary") vocab-fields vocab-fields "vocabulary" path)
  (doseq [axis vocab-fields]
  (unique-strings (get-in catalog ["vocabulary" axis]) (str "vocabulary." axis) path)
  (if (not (= (get exact-wire-vocabulary axis) (set (get-in catalog ["vocabulary" axis])))) (do
  (throw (ex-info (str "Agent Machinery wire vocabulary drift at " path ": " axis) {:path path :axis axis})))))
  (exact-keys (get catalog "defaults") default-fields default-fields "defaults" path)
  (doseq [[field axis] route-axis-fields]
  (if (not (some #{(get-in catalog ["defaults" field])} (get-in catalog ["vocabulary" axis]))) (do
  (throw (ex-info (str "delegation composition catalog: invalid defaults." field) {:path path :field field})))))
  (let [presets (get catalog "presets")]
  (if (not (and (vector? presets) (seq presets))) (do
  (throw (ex-info (str "invalid Agent Machinery delegation composition catalog at " path) {:path path :version version}))))
  (doseq [preset presets]
  (exact-keys preset preset-fields preset-fields (str "preset " (or (get preset "name") "<unknown>")) path)
  (doseq [[field axis] route-axis-fields]
  (if (not (some #{(get preset field)} (get-in catalog ["vocabulary" axis]))) (do
  (throw (ex-info (str (get preset "name") ": invalid " field) {:path path :preset (get preset "name") :field field})))))
  (unique-strings (get preset "capabilities") (str (get preset "name") ".capabilities") path)
  (if (some (fn [capability] (not (boolean (preset-capabilities capability)))) (get preset "capabilities")) (do
  (throw (ex-info (str (get preset "name") ": noncanonical capability") {:path path :preset (get preset "name")}))))
  (doseq [field ["tagline" "description"]]
  (if (not (and (string? (get preset field)) (not (str/blank? (get preset field))))) (do
  (throw (ex-info (str (get preset "name") ": missing " field) {:path path :preset (get preset "name") :field field}))))))
  (let [names (mapv (fn [preset] (get preset "name")) presets)
   known (set names)]
  (if (or (some nil? names) (not= (count names) (count known))) (do
  (throw (ex-info (str "invalid or duplicate Agent Machinery template name at " path) {:path path :names names}))))
  (if (not (= stock-preset-names known)) (do
  (throw (ex-info (str "Agent Machinery stock template set drift at " path) {:path path :expected stock-preset-names :actual known}))))
  (let [orchestrators (->> presets (filter (fn [preset] (= "orchestrator" (get preset "topology")))) (mapv (fn [preset] (get preset "name"))))]
  (if (not (= stock-orchestrator-roles (set orchestrators))) (do
  (throw (ex-info (str "Agent Machinery stock topology drift at " path ": orchestrator topology is the director plus the scope ladder") {:path path :orchestrators orchestrators})))))
  (doseq [preset presets]
  (let [name (get preset "name")
   capabilities (set (get preset "capabilities"))]
  (if (not (and (capabilities "filesystem.read") (capabilities "filesystem.search"))) (do
  (throw (ex-info (str "Agent Machinery stock template " name " must retain read and search authority") {:path path :preset name}))))
  (if (stock-authoring-roles name) (if (not (and (capabilities "filesystem.write") (capabilities "shell"))) (do
  (throw (ex-info (str "Agent Machinery stock authoring template " name " must retain write and shell authority") {:path path :preset name})))) (if (or (capabilities "filesystem.write") (capabilities "shell") (not (capabilities "shell.readonly"))) (do
  (throw (ex-info (str "Agent Machinery stock nonauthoring template " name " must remain read-only") {:path path :preset name})))))
  (if (not (= (contains? stock-orchestrator-roles name) (boolean (capabilities "coordination")))) (do
  (throw (ex-info "Agent Machinery stock coordination authority belongs to the orchestrator ladder" {:path path :preset name}))))
  (if (and (capabilities "shell") (capabilities "shell.readonly")) (do
  (throw (ex-info (str name ": shell and shell.readonly are mutually exclusive") {:path path :preset name}))))
  (let [problem (posture-capability-problem (get preset "posture") capabilities)]
  (if problem (do
  (throw (ex-info (str name ": " problem) {:path path :preset name})))))))
  catalog))))

(defn load-catalog
  ([]
    (load-catalog (catalog-path)))
  ([^String path]
    (let [file (io/file path)]
  (normalize-catalog (json/parse-string (slurp file)) (.getPath file)))))

(defn presets-by-name [catalog]
  (into {} (map (juxt (fn [preset] (get preset "name")) identity) (get catalog "presets"))))
