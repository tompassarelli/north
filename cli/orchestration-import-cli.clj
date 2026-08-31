(ns beagle.user
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [cheshire.core :as json]
            [store.rpc-limits :as rpc-limits]))

(def CLI-DIR (.getParent (io/file *file*)))

(load-file (str CLI-DIR "/coord.clj"))

(load-file (str CLI-DIR "/orchestration-selection.clj"))

(def enumerate-selection-rules north.orchestration-selection/enumerate-selection-rules)

(def rules-digest north.orchestration-selection/rules-digest)

(def ^:private this-root (.getParent (io/file CLI-DIR)))

(defn ^String orchestration-home []
  (str this-root "/agent-machinery"))

(defn ^String agent-runtime-home []
  (or (System/getenv "NORTH_AGENT_RUNTIME_HOME") (str (or (System/getenv "NORTH_HOME") this-root (System/getProperty "user.dir")) "/agent-runtime/orchestration")))

(defn read-json [^String root & $beagle$rest$host]
  (let [segs (vec $beagle$rest$host)]
  (json/parse-string (slurp (apply io/file root segs)))))

(defn extract-section-fence [^String text ^String heading]
  (let [lines (str/split text #"\n" -1)
   want (str "## " (str/lower-case heading))
   start (some (fn [[i l]] (if (= want (str/lower-case (str/trim l))) (do
  (inc (int i))))) (map-indexed vector lines))]
  (if start (do
  (loop [i start
   open nil]
  (if (< i (count lines)) (do
  (let [t (str/trim (nth lines i))]
  (cond
  (and (nil? open) (str/starts-with? t "## ")) nil
  (and (nil? open) (str/starts-with? t "```")) (recur (inc i) (inc i))
  (and (some? open) (str/starts-with? t "```")) (str/join "\n" (subvec lines open i))
  :else (recur (inc i) open))))))))))

(defn extract-first-fence [^String text]
  (let [lines (str/split text #"\n" -1)]
  (loop [i 0
   open nil]
  (if (< i (count lines)) (do
  (let [t (str/trim (nth lines i))]
  (cond
  (and (nil? open) (str/starts-with? t "```")) (recur (inc i) (inc i))
  (and (some? open) (str/starts-with? t "```")) (str/join "\n" (subvec lines open i))
  :else (recur (inc i) open))))))))

(defn ^String section-fence [^String root ^String doc ^String heading]
  (let [text (slurp (io/file root "docs" doc))
   block (extract-section-fence text heading)]
  (if (and block (seq (str/trim block))) block (throw (ex-info (str "no fenced block: " doc " ## " heading) {})))))

(defn selection-signal-values []
  {"decisionOwnership" ["none" "bounded" "cross-boundary" "system-shaping" "open-solution-class"] "seamScope" ["none" "established" "consequential" "system-wide"] "errorExposure" ["contained-reversible" "material-recoverable" "high-or-hard-to-reverse"] "oracleStrength" ["not-applicable" "objective-local" "objective-end-to-end" "partial" "judgment-only"] "foundationalImpact" ["none" "implementation-only" "invariant-decision-owned"] "dependencyShape" ["atomic-cohesive" "deterministic-workflow" "parallel-breadth" "dynamic-decomposition" "tightly-coupled-sequential"] "reasoningShape" ["deterministic" "bounded-branching" "multi-hypothesis" "system-synthesis" "exceptional"]})

(def ^String POINTER "@catalog:current")

(defn exact-values! [port ^String subject ^String predicate]
  (->> (north.coord/query-rows! port {:find "v" :rules [{:head {:rel "v" :args [{:var "v"}]} :body [{:rel "triple" :args [subject predicate {:var "v"}]}]}]}) (map first)))

(defn current-version! [port]
  (some-> (first (exact-values! port POINTER "catalog_version")) parse-long))

(defn ^String ns-subject [ver & $beagle$rest$host]
  (let [parts (vec $beagle$rest$host)]
  (str "@catalog:v" ver ":" (str/join ":" parts))))

(defn parse-version [arg]
  (if (some? arg) (do
  (let [bind__1 (re-matches #"v?(\d+)" (str arg))]
  (if bind__1 (let [[_ n] bind__1]
  (parse-long n)) (throw (ex-info (str "bad catalog version " (pr-str arg) " — expected N or vN") {:type :catalog-bad-version :arg arg})))))))

(defn version-arg! [port arg]
  (or (parse-version arg) (current-version! port) (throw (ex-info "no @catalog:current pointer — import first" {}))))

(defn publish-actions! [port actions]
  (let [result (north.coord/publish! port (vec actions))]
  (if (:reject result) (do
  (throw (ex-info "Store RPC rejected delegation composition catalog publication" {:type :catalog-publication-rejected :result result}))))
  result))

(defn staging-batches [actions]
  (loop [remaining (seq actions)
   batch []
   batch-cost 0
   batches []]
  (let [action (first remaining)]
  (if action (let [cost (max 1 (count (:values action)))]
  (if (> cost rpc-limits/rpc-v2-max-batch-actions) (do
  (throw (ex-info "one catalog staging action exceeds the Store mutation bound" {:type :catalog-staging-action-too-large :cost cost :limit rpc-limits/rpc-v2-max-batch-actions :action action}))))
  (if (and (seq batch) (> (+ batch-cost cost) rpc-limits/rpc-v2-max-batch-actions)) (recur remaining [] 0 (conj batches batch)) (recur (next remaining) (conj batch action) (+ batch-cost cost) batches))) (cond-> batches (seq batch) (conj batch))))))

(defn publish-staging! [port actions]
  (doseq [batch (staging-batches actions)]
  (publish-actions! port batch)))

(defn flip! [port ver]
  (publish-actions! port [{:op :set :subject "@catalog_version" :predicate "cardinality" :values ["single"] :cardinality :one} {:op :set :subject POINTER :predicate "catalog_version" :values [(str ver)] :cardinality :one}]))

(def ^:dynamic *publication-actions* nil)

(defn queue-set! [port ^String subject ^String predicate values cardinality]
  (let [encoded-values (vec (map str values))]
  (if *publication-actions* (swap! *publication-actions* update [subject predicate cardinality] (fn [current] {:op :set :subject subject :predicate predicate :values (if (= :one cardinality) encoded-values (vec (distinct (concat (:values current) encoded-values)))) :cardinality cardinality})) (publish-actions! port [{:op :set :subject subject :predicate predicate :values encoded-values :cardinality cardinality}]))))

(defn s1! [port ^String subj ^String p v]
  (if (some? v) (do
  (queue-set! port subj p [v] :one))))

(defn smulti! [port ^String subj ^String p vs]
  (queue-set! port subj p vs :many))

(defn ^String emit-staffing! [port ver catalog]
  (let [subj (ns-subject ver "staffing")
   d (get catalog "defaults")]
  (s1! port subj "kind" "staffing_catalog")
  (s1! port subj "catalog_version" (get catalog "version"))
  (s1! port subj "default_task_grade" (get d "taskGrade"))
  (s1! port subj "default_tier" (get d "tier"))
  (s1! port subj "default_reasoning" (get d "deliberation"))
  (s1! port subj "default_topology" (get d "topology"))
  (s1! port subj "default_posture" (get d "posture"))
  subj))

(def AXIS-DOC {"taskGrades" {:axis "task_grade" :doc "task-grades.md"} "topologies" {:axis "topology" :doc "topologies.md"} "postures" {:axis "posture" :doc "postures.md"}})

(def AXIS-PLAIN {"semanticTiers" "tier" "deliberations" "reasoning" "capabilities" "capability"})

(defn emit-axis-values! [port ver ^String root catalog]
  (let [vocab (get catalog "vocabulary")]
  (doseq [[vkey {:keys [axis doc]}] AXIS-DOC
   [rank v] (map-indexed vector (get vocab vkey))]
  (let [subj (ns-subject ver "axis" axis v)]
  (s1! port subj "kind" "axis_value")
  (s1! port subj "axis" axis)
  (s1! port subj "rank" rank)
  (s1! port subj "prompt_block" (section-fence root doc v))
  (s1! port subj "doctrine_source" (str "docs/" doc "#" v))))
  (doseq [[vkey axis] AXIS-PLAIN
   [rank v] (map-indexed vector (get vocab vkey))]
  (let [subj (ns-subject ver "axis" axis v)]
  (s1! port subj "kind" "axis_value")
  (s1! port subj "axis" axis)
  (s1! port subj "rank" rank)))))

(defn emit-comms! [port ver ^String root]
  (let [subj (ns-subject ver "comms" "universal")]
  (s1! port subj "kind" "doctrine_block")
  (s1! port subj "prompt_block" (section-fence root "comms.md" "universal"))
  (s1! port subj "doctrine_source" "docs/comms.md#universal")))

(defn emit-templates! [port ver ^String root catalog]
  (doseq [preset (get catalog "presets")]
  (let [name (get preset "name")
   subj (ns-subject ver "template" name)]
  (s1! port subj "kind" "template")
  (s1! port subj "task_grade" (get preset "taskGrade"))
  (s1! port subj "topology" (get preset "topology"))
  (s1! port subj "tier" (get preset "tier"))
  (s1! port subj "reasoning" (get preset "deliberation"))
  (s1! port subj "posture" (get preset "posture"))
  (s1! port subj "tagline" (get preset "tagline"))
  (s1! port subj "doc" (get preset "description"))
  (smulti! port subj "capability" (get preset "capabilities"))
  (s1! port subj "prompt_block" (section-fence root "roles.md" name))
  (s1! port subj "doctrine_source" (str "docs/roles.md#" name)))))

(defn emit-provider! [port ver ^String _root ^String provider]
  (let [runtime (agent-runtime-home)
   cat (read-json runtime "providers" (str provider ".json"))
   psubj (ns-subject ver "provider" provider)
   prov (get cat "provenance")]
  (s1! port psubj "kind" "provider_catalog")
  (s1! port psubj "as_of" (get prov "asOf"))
  (s1! port psubj "review_after" (get prov "reviewAfter"))
  (smulti! port psubj "transport" (get cat "transports"))
  (smulti! port psubj "provenance_source" (map json/generate-string (get prov "sources")))
  (let [aliases (get cat "modelAliases")
   alias-of (reduce (fn [m [a model]] (update m model (fnil conj []) a)) {} aliases)
   deltas (get cat "modelDeltas")]
  (doseq [[model spec] (get cat "models")]
  (let [msubj (ns-subject ver "model" provider model)
   levels (or (get spec "efforts") (get spec "reasoning"))
   routes (get spec "routes")
   cw (get spec "contextWindow")
   delta (get deltas model)]
  (s1! port msubj "kind" "model")
  (smulti! port msubj "alias" (sort (get alias-of model)))
  (smulti! port msubj "deliberation_support" levels)
  (smulti! port msubj "calibrated_route" (for [[tier ls] routes
   l ls]
  (str tier "/" l)))
  (s1! port msubj "context_window_tokens" (get cw "tokens"))
  (s1! port msubj "context_window_from" (get cw "effectiveFrom"))
  (s1! port msubj "delta_kind" (get delta "kind"))
  (if (= "none" (get delta "kind")) (do
  (s1! port msubj "delta_reason" (get delta "reason"))))
  (if (= "calibrated" (get delta "kind")) (do
  (let [path (get delta "path")]
  (s1! port msubj "doctrine_source" path)
  (s1! port msubj "prompt_block" (extract-first-fence (slurp (io/file runtime path))))))))))
  (doseq [[tier spec] (get cat "tiers")]
  (let [tsubj (ns-subject ver "tier-row" provider tier)]
  (s1! port tsubj "kind" "tier_row")
  (s1! port tsubj "tier" tier)
  (s1! port tsubj "model" (get spec "model"))
  (smulti! port tsubj "level" (or (get spec "efforts") (get spec "reasoning")))
  (s1! port tsubj "default_level" (or (get spec "defaultEffort") (get spec "defaultReasoning")))))))

(defn emit-selection! [port ver ^String root]
  (let [signals (selection-signal-values)
   rules (enumerate-selection-rules root)
   policy (ns-subject ver "selection-policy" "minimum-sufficient-v1")]
  (doseq [[sig vals] signals]
  (let [subj (ns-subject ver "signal" sig)]
  (s1! port subj "kind" "selection_signal")
  (smulti! port subj "one_of" vals)))
  (s1! port policy "kind" "selection_policy")
  (doseq [r rules]
  (let [code (get r "rule_code")
   subj (ns-subject ver "rule" code)]
  (s1! port subj "kind" "selection_rule")
  (s1! port subj "signal" (get r "signal"))
  (s1! port subj "signal_value" (get r "signal_value"))
  (s1! port subj "min_tier" (get r "min_tier"))
  (s1! port subj "min_reasoning" (get r "min_reasoning"))
  (s1! port subj "rule_code" code)
  (smulti! port policy "rule" [subj])))
  (s1! port policy "policy_sha256" (rules-digest rules))))

(defn import! [port ^String root]
  (let [ver (inc (int (or (current-version! port) 0)))
   catalog (read-json root "delegation" "catalog.json")
   actions (atom {})]
  (binding [*publication-actions* actions]
  (emit-staffing! port ver catalog)
  (emit-axis-values! port ver root catalog)
  (emit-comms! port ver root)
  (emit-templates! port ver root catalog)
  (emit-provider! port ver root "anthropic")
  (emit-provider! port ver root "openai")
  (emit-selection! port ver root))
  (publish-staging! port (map second (sort-by first (deref actions))))
  (flip! port ver)
  ver))

(defn retract-version! [port ver]
  (let [prefix (str "@catalog:v" ver ":")
   rows (north.coord/query-rows! port {:find "s,p,o" :rules [{:head {:rel "s,p,o" :args [{:var "s"} {:var "p"} {:var "o"}]} :body [{:rel "triple" :args [{:var "s"} {:var "p"} {:var "o"}]}]}]})
   mine (filter (fn [[s _predicate _value]] (str/starts-with? s prefix)) rows)]
  (publish-actions! port (concat (for [[subject predicate] (distinct (map (juxt first second) mine))]
  {:op :set :subject subject :predicate predicate :values [] :cardinality :many}) (if (= ver (current-version! port)) [{:op :set :subject POINTER :predicate "catalog_version" :values [] :cardinality :one}] [])))
  (count mine)))

(defn show! [port ver]
  (let [prefix (str "@catalog:v" ver ":")
   rows (north.coord/query-rows! port {:find "s" :rules [{:head {:rel "s" :args [{:var "s"}]} :body [{:rel "triple" :args [{:var "s"} "kind" {:var "k"}]}]}]})
   mine (sort (distinct (filter (fn [subject] (str/starts-with? subject prefix)) (map first rows))))]
  (println (format "pointer @catalog:current -> v%s (%d subjects)" ver (count mine)))
  (doseq [s mine]
  (println "  " s))))

(defn -main [& $beagle$rest$host]
  (let [argv (vec $beagle$rest$host)]
  (let [[ps verb arg] argv
   port (Integer/parseInt (or ps "7977"))]
  (case verb
    "import" (let [ver (import! port (orchestration-home))]
  (println (format "✓ imported catalog v%d on :%d; @catalog:current -> v%d" ver port ver)))
    "retract" (let [ver (version-arg! port arg)
   n (retract-version! port ver)]
  (println (format "✓ retracted %d facts under @catalog:v%d:" n ver)))
    "show" (show! port (version-arg! port arg))
    (do
  (println "usage: orchestration-import-cli.clj <port> {import|retract <N|vN>|show [N|vN]}")
  (System/exit 2))))))

(if (= *file* (System/getProperty "babashka.file")) (do
  (apply -main *command-line-args*)))
