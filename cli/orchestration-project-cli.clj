(ns user
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [cheshire.core :as json])
  (:import [java.security MessageDigest]
           [java.nio.charset StandardCharsets]
           [java.nio.file Files]))

(def CLI-DIR (.getParent (io/file *file*)))

(load-file (str CLI-DIR "/coord.clj"))

(load-file (str CLI-DIR "/orchestration-selection.clj"))

(def enumerate-selection-rules north.orchestration-selection/enumerate-selection-rules)

(def rule-map north.orchestration-selection/rule-map)

(def rules-digest north.orchestration-selection/rules-digest)

(def POINTER "@catalog:current")

(def REASONING-RANK ["low" "medium" "high" "xhigh" "max"])

(defn reasoning-rank [^String value]
  (loop [index 0]
  (if (= index (count REASONING-RANK)) (count REASONING-RANK) (if (= value (nth REASONING-RANK index)) index (recur (inc index))))))

(defn by-reasoning [xs]
  (sort-by reasoning-rank xs))

(defn query-rows! [port query ^String context]
  (try
  (north.coord/query-rows! port query)
  (catch clojure.lang.ExceptionInfo error
    (throw (ex-info (str "catalog projection query failed for " context) (merge {:type :catalog-projection-query-failed :context context} (select-keys (ex-data error) [:error :code])) error)))))

(defn current-version [port]
  (let [resp (try
  (north.coord/resolved-envelope! port POINTER "catalog_version")
  (catch clojure.lang.ExceptionInfo error
    (throw (ex-info "catalog projection failed resolving @catalog:current" (merge {:type :catalog-projection-query-failed :context "@catalog:current version"} (select-keys (ex-data error) [:error :code])) error))))]
  (if (:ambiguous? resp) (do
  (throw (ex-info "@catalog:current holds multiple catalog_version values — pointer flip did not supersede" {:type :catalog-pointer-ambiguous :values (:values resp)}))))
  (or (some-> (:value resp) parse-long) (throw (ex-info "no @catalog:current pointer — import first" {:type :catalog-pointer-missing})))))

(defn facts
  "All (p o) facts for one subject through the coordinator's indexed show op.\n   A Datalog query for this exact shape still pays the per-version query-engine\n   warmup; under concurrent admission that warmup exhausted :query-time-limit\n   before the already-ground subject lookup ran." [port ^String subj]
  (try
  (reduce (fn [m [p o]] (update m p (fnil conj []) o)) {} (:rows (north.coord/show-envelope! port subj)))
  (catch clojure.lang.ExceptionInfo error
    (throw (ex-info (str "catalog projection query failed for facts of " subj) (merge {:type :catalog-projection-query-failed :context (str "facts of " subj)} (select-keys (ex-data error) [:error :code])) error)))))

(defn one [f p]
  (first (get f p)))

(defn many [f p]
  (vec (get f p)))

(defn one! [f p ^String subj]
  (or (one f p) (throw (ex-info (str "catalog projection: " subj " is missing required field " p) {:type :catalog-projection-missing-field :subject subj :field p}))))

(defn long! [f p ^String subj]
  (parse-long (one! f p subj)))

(defn ^String id-name [^String subj]
  (last (str/split subj #":")))

(defn subjects-of-kind!
  "Version-scoped subject ids carrying kind=k." [port ver ^String k]
  (let [prefix (str "@catalog:v" ver ":")]
  (->> (query-rows! port {:find "s" :rules [{:head {:rel "s" :args [{:var "s"}]} :body [{:rel "triple" :args [{:var "s"} "kind" k]}]}]} (str "subjects of kind " k)) (map first) (filter (fn [^String subject] (str/starts-with? subject prefix))) sort)))

(def AXIS-KEY {"task_grade" "taskGrades" "tier" "semanticTiers" "reasoning" "deliberations" "topology" "topologies" "posture" "postures" "capability" "capabilities"})

(defn project-staffing! [port]
  (let [ver (current-version port)
   st (facts port (str "@catalog:v" ver ":staffing"))
   axis-values (subjects-of-kind! port ver "axis_value")
   by-axis (reduce (fn [m ^String s] (let [f (facts port s)]
  (update m (one! f "axis" s) (fnil conj []) [(long! f "rank" s) (id-name s)]))) {} axis-values)
   vocab (reduce (fn [m [axis vk]] (assoc m vk (mapv second (sort-by first (get by-axis axis))))) {} AXIS-KEY)
   cap-rank (into {} (map (fn [[r n]] [n r]) (get by-axis "capability")))
   presets (for [s (subjects-of-kind! port ver "template")]
  (let [f (facts port s)]
  {"name" (id-name s) "taskGrade" (one f "task_grade") "tier" (one f "tier") "deliberation" (one f "reasoning") "topology" (one f "topology") "posture" (one f "posture") "capabilities" (vec (sort-by cap-rank (many f "capability"))) "tagline" (one f "tagline") "description" (one f "doc")}))]
  {"$schema" "./catalog.schema.json" "version" (long! st "catalog_version" "@catalog:staffing") "vocabulary" vocab "defaults" {"taskGrade" (one st "default_task_grade") "tier" (one st "default_tier") "deliberation" (one st "default_reasoning") "topology" (one st "default_topology") "posture" (one st "default_posture")} "presets" (vec presets) "aliases" []}))

(defn project-provider! [port ^String provider]
  (let [ver (current-version port)
   prefix (str "@catalog:v" ver ":")
   p (facts port (str prefix "provider:" provider))
   model-subjs (filter (fn [^String subject] (str/starts-with? subject (str prefix "model:" provider ":"))) (subjects-of-kind! port ver "model"))
   tier-subjs (filter (fn [^String subject] (str/starts-with? subject (str prefix "tier-row:" provider ":"))) (subjects-of-kind! port ver "tier_row"))
   model-facts (into {} (map (fn [^String s] [(id-name s) (facts port s)]) model-subjs))
   aliases (into {} (for [[m f] model-facts
   a (many f "alias")]
  [a m]))
   models (into {} (for [[m f] model-facts]
  (let [routes (reduce (fn [acc ^String r] (let [[tier lvl] (str/split r #"/")]
  (update acc tier (fnil conj []) lvl))) {} (many f "calibrated_route"))]
  [m (cond-> {"reasoning" (by-reasoning (many f "deliberation_support")) "contextWindow" {"tokens" (long! f "context_window_tokens" (str provider ":" m)) "effectiveFrom" (one! f "context_window_from" (str provider ":" m))}} (not (empty? routes)) (assoc "routes" (into {} (map (fn [[t ls]] [t (by-reasoning ls)]) routes))))])))
   deltas (into {} (for [[m f] model-facts]
  [m (if (= "calibrated" (one f "delta_kind")) {"kind" "calibrated" "path" (one f "doctrine_source")} {"kind" (one f "delta_kind") "reason" (one f "delta_reason")})]))
   tiers (into {} (for [s tier-subjs]
  (let [f (facts port s)]
  [(one f "tier") {"model" (one f "model") "reasoning" (by-reasoning (many f "level")) "defaultReasoning" (one f "default_level")}])))]
  {"$schema" "./catalog.schema.json" "provider" provider "provenance" {"asOf" (one p "as_of") "reviewAfter" (one p "review_after") "sources" (mapv json/parse-string (many p "provenance_source"))} "transports" (many p "transport") "modelAliases" aliases "models" models "modelDeltas" deltas "tiers" tiers}))

(def ^:private this-root (.getParent (io/file CLI-DIR)))

(defn ^String orchestration-root []
  (str this-root "/agent-machinery"))

(def MAX-POLICY-RULES 128)

(def MAX-POLICY-RULE-FACTS 4096)

(def POLICY-SCOPED-PROJECTION-DEADLINE-MS 5000)

(defn- fold-rule-rows [rows]
  (reduce-kv (fn [out ^String subject facts] (reduce (fn [projected [predicate value]] (update-in projected [subject predicate] (fnil conj []) value)) out facts)) {} rows))

(defn- scoped-rule-facts [port rule-subjs]
  (let [allowed (set rule-subjs)
   response (binding [north.coord/*request-deadline-ns* (north.coord/request-deadline-ns POLICY-SCOPED-PROJECTION-DEADLINE-MS)]
  (north.coord/show-many-in-domain! port :coordination rule-subjs))
   rows (:rows response)
   fact-count (reduce + 0 (map count (vals rows)))]
  (if (not (and (map? response) (= #{:version :rows} (set (keys response))) (integer? (:version response)) (not (neg? (:version response))) (map? rows) (= allowed (set (keys rows))) (<= fact-count MAX-POLICY-RULE-FACTS) (every? (fn [[subject facts]] (and (contains? allowed subject) (vector? facts) (seq facts) (every? (fn [row] (and (vector? row) (= 2 (count row)) (every? string? row))) facts))) rows))) (do
  (throw (ex-info "scoped policy rule projection was malformed" {:type :catalog-projection-query-failed :context "selection policy rule subjects"}))))
  (fold-rule-rows rows)))

(defn- project-rule-facts [port rule-subjs]
  (if (> (count rule-subjs) MAX-POLICY-RULES) (do
  (throw (ex-info "selection policy links too many rules" {:type :catalog-projection-query-failed :context "selection policy rule subjects" :count (count rule-subjs)}))))
  (if (empty? rule-subjs) {} (scoped-rule-facts port rule-subjs)))

(defn project-policy-pin [port]
  (let [ver (current-version port)
   policy (str "@catalog:v" ver ":selection-policy:minimum-sufficient-v1")
   pf (facts port policy)
   stored (one pf "policy_sha256")
   rule-subjs (vec (distinct (many pf "rule")))
   rule-facts (project-rule-facts port rule-subjs)
   graph-rules (for [s rule-subjs]
  (let [f (get rule-facts s)]
  (rule-map (one f "signal") (one f "signal_value") (one f "rule_code") (one f "min_tier") (one f "min_reasoning"))))
   validator-rules (enumerate-selection-rules (orchestration-root))]
  {"policyVersion" "minimum-sufficient-v1" "catalogVersion" ver "storedSha256" stored "projectionSha256" (rules-digest graph-rules) "validatorSha256" (rules-digest validator-rules)}))

(defn- canon
  "Recursively sort map keys so the JSON serialization is order-independent." [x]
  (cond
  (map? x) (into (sorted-map) (map (fn [[k v]] [k (canon v)]) x))
  (sequential? x) (mapv canon x)
  :else x))

(defn- ^String sha256-hex [^String s]
  (let [md (MessageDigest/getInstance "SHA-256")
   bs (.digest md (.getBytes s StandardCharsets/UTF_8))]
  (str/join (map (fn [%1] (format "%02x" (bit-and %1 0xff))) bs))))

(def MAX-CATALOG-CACHE-BYTES (* 4 1024 1024))

(defn ^String catalog-projection-cache-path []
  (or (some-> (System/getenv "NORTH_ORCHESTRATION_CATALOG_CACHE") str/trim not-empty) (str (or (some-> (System/getenv "XDG_STATE_HOME") str/trim not-empty) (str (or (System/getenv "HOME") (System/getProperty "user.home")) "/.local/state")) "/north/orchestration-catalog-projection-cache.json")))

(defn cached-catalog-pin
  "Validate the durable projection record without touching the graph's costly\n   kind scans. Imported @catalog:vN namespaces are write-once drafts followed\n   by one atomic @catalog:current flip, so catalogVersion is the invalidation\n   boundary; ordinary coordination writes only advance coordinatorVersion." [ver coord-ver]
  (try
  (let [f (io/file (catalog-projection-cache-path))]
  (if (and (.isFile f) (pos? (.length f)) (<= (.length f) MAX-CATALOG-CACHE-BYTES) (not (Files/isSymbolicLink (.toPath f)))) (do
  (let [record (json/parse-string (slurp f))
   bundle (get record "bundle")
   subgraph {"staffing" (get bundle "staffing") "providers" (get bundle "providers")}
   digest (sha256-hex (json/generate-string (canon subgraph)))
   recorded (get record "catalogDigestSha256")]
  (if (and (= 1 (get record "version")) (map? bundle) (map? (get bundle "staffing")) (map? (get bundle "providers")) (= ver (get record "catalogVersion")) (= ver (get bundle "catalogVersion")) (integer? (get record "coordinatorVersion")) (string? recorded) (re-matches #"[0-9a-f]{64}" recorded) (= digest recorded)) (do
  {"catalogVersion" ver "coordinatorVersion" coord-ver "catalogDigestSha256" recorded}))))))
  (catch Exception _
    nil)))

(defn project-catalog-pin! [port]
  (let [ver (current-version port)
   coord-ver (north.coord/cur-ver! port)]
  (or (cached-catalog-pin ver coord-ver) (let [subgraph {"staffing" (project-staffing! port) "providers" {"anthropic" (project-provider! port "anthropic") "openai" (project-provider! port "openai")}}]
  {"catalogVersion" ver "coordinatorVersion" coord-ver "catalogDigestSha256" (sha256-hex (json/generate-string (canon subgraph)))}))))

(defn project-bundle! [port]
  (let [ver (current-version port)]
  {"catalogVersion" ver "staffing" (project-staffing! port) "providers" {"anthropic" (project-provider! port "anthropic") "openai" (project-provider! port "openai")}}))

(def subjects-of-kind subjects-of-kind!)

(def project-staffing project-staffing!)

(def project-provider project-provider!)

(def project-catalog-pin project-catalog-pin!)

(def project-bundle project-bundle!)

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (let [[ps verb arg] args
   port (Integer/parseInt (or ps "7977"))]
  (case verb
    "staffing" (println (json/generate-string (project-staffing! port)))
    "provider" (println (json/generate-string (project-provider! port arg)))
    "bundle" (println (json/generate-string (project-bundle! port)))
    "policy-pin" (println (json/generate-string (project-policy-pin port)))
    "catalog-pin" (println (json/generate-string (project-catalog-pin! port)))
    (do
  (println "usage: orchestration-project-cli.clj <port> {staffing | provider <name> | bundle | policy-pin | catalog-pin}")
  (System/exit 2))))))

(if (= *file* (System/getProperty "babashka.file")) (do
  (apply -main *command-line-args*)))
