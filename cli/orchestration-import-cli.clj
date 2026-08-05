;; orchestration-import-cli.clj — canonical orchestration catalog importer
;; for the Orchestration -> North Orchestration migration (thread
;; 019f8f5c-74e0-7be7-ba65-3179f1bccde1; design doc:
;; north-orchestration-vocabulary-design.md in the repo's private docs —
;; packaged code must not embed checkout/home paths, per the package
;; path-hygiene lint, so the Orchestration source root is resolved at runtime from
;; $NORTH_ORCHESTRATION_HOME / $HOME, never a literal).
;;
;; Lifts the machine catalog into the coordination graph as DRAFT subjects under a
;; version namespace (@catalog:v<N>:*), then flips the @catalog:current
;; pointer in one atomic FRAMRPC transaction — the atomic pointer flip of
;; design R3. Consumers read the pointer, so a torn/partial import is never
;; visible. Sources (all Orchestration-repo-relative, read at runtime):
;;   staffing/catalog.json          templates + axis vocabulary + defaults
;;   providers/{anthropic,openai}.json  provider_catalog/model/tier_row
;;   docs/{roles,comms,task-grades,topologies,postures}.md  prompt_block fences
;;   docs/deltas/*.md               calibrated model delta prompt_blocks
;;   scripts/selection-assessment.mjs  selection_signal/policy/rule (via node,
;;                                     the canonical source — no hand-mirror)
;;
;; usage:
;;   bb orchestration-import-cli.clj <port> import   [orchestration-home]  stage + flip pointer
;;   bb orchestration-import-cli.clj <port> measure  [orchestration-home]  R2 throwaway interning probe
;;   bb orchestration-import-cli.clj <port> retract  <N|vN>         undo one imported version
;;   bb orchestration-import-cli.clj <port> show     [N|vN]         print the pointed subgraph ids
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[cheshire.core :as json]
         '[babashka.process :as p])

;; *file*, not babashka.file: under a test's load-file only *file* still names THIS
;; file, so the sibling loads below resolve either way.
(def CLI-DIR (.getParent (io/file *file*)))
(load-file (str CLI-DIR "/coord.clj"))
(load-file (str CLI-DIR "/orchestration-selection.clj"))
(def enumerate-selection-rules north.orchestration-selection/enumerate-selection-rules)
(def rules-digest              north.orchestration-selection/rules-digest)

;; ---------------------------------------------------------------------------
;; Source resolution — runtime only, never an embedded path. this-root derives
;; from THIS file's own location (never user.dir): a subprocess launched by
;; execFileSync/babashka.process inherits the CALLER's cwd, not the north
;; checkout, so a bare user.dir fallback silently walks to
;; <caller-cwd>/orchestration and misses the real one.
;; ---------------------------------------------------------------------------
(def ^:private this-root (.getParent (io/file CLI-DIR)))

(defn orchestration-home [arg]
  (or arg
      (System/getenv "NORTH_ORCHESTRATION_HOME")
      (str (or (System/getenv "NORTH_HOME") this-root (System/getProperty "user.dir")) "/orchestration")))

(defn read-json [root & segs]
  (json/parse-string (slurp (apply io/file root segs))))

;; ---------------------------------------------------------------------------
;; Prompt-block fence extraction — mirrors sdk/src/harness.ts
;; extractFenceFromSection / extractFirstFence so the imported prompt_block is
;; byte-identical to what the harness reads from NORTH_ORCHESTRATION_HOME today.
;; ---------------------------------------------------------------------------
(defn extract-section-fence [text heading]
  (let [lines (str/split text #"\n" -1)
        want (str "## " (str/lower-case heading))
        start (some (fn [[i l]] (when (= want (str/lower-case (str/trim l))) (inc i)))
                    (map-indexed vector lines))]
    (when start
      (loop [i start open nil]
        (when (< i (count lines))
          (let [t (str/trim (nth lines i))]
            (cond
              (and (nil? open) (str/starts-with? t "## ")) nil
              (and (nil? open) (str/starts-with? t "```")) (recur (inc i) (inc i))
              (and (some? open) (str/starts-with? t "```")) (str/join "\n" (subvec lines open i))
              :else (recur (inc i) open))))))))

(defn extract-first-fence [text]
  (let [lines (str/split text #"\n" -1)]
    (loop [i 0 open nil]
      (when (< i (count lines))
        (let [t (str/trim (nth lines i))]
          (cond
            (and (nil? open) (str/starts-with? t "```")) (recur (inc i) (inc i))
            (and (some? open) (str/starts-with? t "```")) (str/join "\n" (subvec lines open i))
            :else (recur (inc i) open)))))))

(defn section-fence [root doc heading]
  (let [text (slurp (io/file root "docs" doc))
        block (extract-section-fence text heading)]
    (when-not (and block (seq (str/trim block)))
      (throw (ex-info (str "no fenced block: " doc " ## " heading) {})))
    block))

;; Selection rules are enumerated FROM the canonical validator (never mirrored)
;; by the shared north.orchestration-selection ns — the same code the projector
;; runs at admission, so the policy_sha256 written here is byte-comparable to
;; the digest the §3.2 pin recomputes.

(defn selection-signal-values []
  {"decisionOwnership" ["none" "bounded" "cross-boundary" "system-shaping" "open-solution-class"]
   "seamScope" ["none" "established" "consequential" "system-wide"]
   "errorExposure" ["contained-reversible" "material-recoverable" "high-or-hard-to-reverse"]
   "oracleStrength" ["not-applicable" "objective-local" "objective-end-to-end" "partial" "judgment-only"]
   "foundationalImpact" ["none" "implementation-only" "invariant-decision-owned"]
   "dependencyShape" ["atomic-cohesive" "deterministic-workflow" "parallel-breadth" "dynamic-decomposition" "tightly-coupled-sequential"]
   "reasoningShape" ["deterministic" "bounded-branching" "multi-hypothesis" "system-synthesis" "exceptional"]})

;; ---------------------------------------------------------------------------
;; Pointer / version.
;; ---------------------------------------------------------------------------
(def POINTER "@catalog:current")

(defn exact-values [port subject predicate]
  (->> (north.coord/query-rows
        port
        {:find "v" :rules [{:head {:rel "v" :args [{:var "v"}]}
                             :body [{:rel "triple" :args [subject predicate {:var "v"}]}]}]})
       (map first)))

(defn current-version [port]
  (some-> (first (exact-values port POINTER "catalog_version")) parse-long))

(defn ns-subject [ver & parts] (str "@catalog:v" ver ":" (str/join ":" parts)))

;; Accepts both spellings the surface prints (`3`, `v3`); an unparseable arg must
;; fail loudly, never degrade to nil and silently read the pointer instead.
(defn parse-version [arg]
  (when (some? arg)
    (if-let [[_ n] (re-matches #"v?(\d+)" (str arg))]
      (parse-long n)
      (throw (ex-info (str "bad catalog version " (pr-str arg) " — expected N or vN")
                      {:type :catalog-bad-version :arg arg})))))

(defn version-arg [port arg]
  (or (parse-version arg)
      (current-version port)
      (throw (ex-info "no @catalog:current pointer — import first" {}))))

(defn publish-actions! [port actions]
  (let [result (north.coord/publish! port (vec actions))]
    (when (:reject result)
      (throw (ex-info "FRAMRPC rejected orchestration catalog publication"
                      {:type :catalog-publication-rejected :result result})))
    result))

(defn flip! [port ver]
  (publish-actions!
   port
   [{:op :set :subject "@catalog_version" :predicate "cardinality"
     :values ["single"] :cardinality :one}
    {:op :set :subject POINTER :predicate "catalog_version"
     :values [(str ver)] :cardinality :one}]))

;; ---------------------------------------------------------------------------
;; Emit — every write goes to the version namespace (draft) until the flip.
;; ---------------------------------------------------------------------------
(def ^:dynamic *publication-actions* nil)

(defn queue-set! [port subject predicate values cardinality]
  (let [values (vec (map str values))]
    (if *publication-actions*
      (swap! *publication-actions*
             update [subject predicate cardinality]
             (fn [current]
               {:op :set :subject subject :predicate predicate
                :values (if (= :one cardinality)
                          values
                          (vec (distinct (concat (:values current) values))))
                :cardinality cardinality}))
      (publish-actions!
       port
       [{:op :set :subject subject :predicate predicate
         :values values :cardinality cardinality}]))))

(defn s1! [port subj p v]
  (when (some? v)
    (queue-set! port subj p [v] :one)))

(defn smulti! [port subj p vs]
  (queue-set! port subj p vs :many))

(defn emit-staffing! [port ver catalog]
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

;; Axes whose values carry a doctrine prompt_block fence (design section 1.2).
(def AXIS-DOC
  {"taskGrades"   {:axis "task_grade" :doc "task-grades.md"}
   "topologies"   {:axis "topology"   :doc "topologies.md"}
   "postures"     {:axis "posture"    :doc "postures.md"}})
;; Axes that are enum-only vocabulary (no doctrine fence of their own).
(def AXIS-PLAIN
  {"semanticTiers" "tier"
   "deliberations" "reasoning"
   "capabilities"  "capability"})

(defn emit-axis-values! [port ver root catalog]
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

(defn emit-comms! [port ver root]
  (let [subj (ns-subject ver "comms" "universal")]
    (s1! port subj "kind" "doctrine_block")
    (s1! port subj "prompt_block" (section-fence root "comms.md" "universal"))
    (s1! port subj "doctrine_source" "docs/comms.md#universal")))

(defn emit-templates! [port ver root catalog]
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

;; Provider catalogs — provider derives from the subject namespace, so no
;; `provider` ref fact is emitted (the R9 ref/literal collision stays deferred).
(defn emit-provider! [port ver root provider]
  (let [cat (read-json root "providers" (str provider ".json"))
        psubj (ns-subject ver "provider" provider)
        prov (get cat "provenance")]
    (s1! port psubj "kind" "provider_catalog")
    (s1! port psubj "as_of" (get prov "asOf"))
    (s1! port psubj "review_after" (get prov "reviewAfter"))
    (smulti! port psubj "transport" (get cat "transports"))
    (smulti! port psubj "provenance_source"
             (map json/generate-string (get prov "sources")))
    ;; alias index inverted onto each model
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
          (smulti! port msubj "calibrated_route"
                   (for [[tier ls] routes l ls] (str tier "/" l)))
          (s1! port msubj "context_window_tokens" (get cw "tokens"))
          (s1! port msubj "context_window_from" (get cw "effectiveFrom"))
          (s1! port msubj "delta_kind" (get delta "kind"))
          (when (= "none" (get delta "kind"))
            (s1! port msubj "delta_reason" (get delta "reason")))
          (when (= "calibrated" (get delta "kind"))
            (let [path (get delta "path")]
              (s1! port msubj "doctrine_source" path)
              (s1! port msubj "prompt_block"
                   (extract-first-fence (slurp (io/file root path)))))))))
    ;; tier rows — model id stored as a literal (value-kind compatible with the
    ;; existing single-literal `model` predicate); the row's own tier + levels.
    (doseq [[tier spec] (get cat "tiers")]
      (let [tsubj (ns-subject ver "tier-row" provider tier)]
        (s1! port tsubj "kind" "tier_row")
        (s1! port tsubj "tier" tier)
        (s1! port tsubj "model" (get spec "model"))
        (smulti! port tsubj "level" (or (get spec "efforts") (get spec "reasoning")))
        (s1! port tsubj "default_level" (or (get spec "defaultEffort") (get spec "defaultReasoning")))))))

(defn emit-selection! [port ver root]
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
    ;; digest over the canonical rule projection (design section 1.5) — the
    ;; §3.2 pin recomputes this exact value at admission.
    (s1! port policy "policy_sha256" (rules-digest rules))))

;; ---------------------------------------------------------------------------
;; Verbs.
;; ---------------------------------------------------------------------------
(defn import! [port root]
  (let [ver (inc (or (current-version port) 0))
        catalog (read-json root "staffing" "catalog.json")
        actions (atom {})]
    (binding [*publication-actions* actions]
      (emit-staffing! port ver catalog)
      (emit-axis-values! port ver root catalog)
      (emit-comms! port ver root)
      (emit-templates! port ver root catalog)
      (emit-provider! port ver root "anthropic")
      (emit-provider! port ver root "openai")
      (emit-selection! port ver root))
    (publish-actions! port (vals @actions))
    ;; ATOMIC FLIP — one serialized write; consumers never see a torn import.
    (flip! port ver)
    ver))

;; measure — R2: import ONLY the multi-KB prompt_block literals to a throwaway
;; namespace, measure coordination.log growth + query latency, then retract.
(defn log-size []
  (let [f (io/file (north.coord/expected-log))]
    (if (.isFile f) (.length f) 0)))

(defn query-latency-ms [port subj]
  (let [t0 (System/nanoTime)]
    (dotimes [_ 20] (exact-values port subj "prompt_block"))
    (/ (- (System/nanoTime) t0) 1e6 20.0)))

(defn measure! [port root]
  (let [catalog (read-json root "staffing" "catalog.json")
        blocks (concat
                (for [p (get catalog "presets")] [(get p "name") (section-fence root "roles.md" (get p "name"))])
                (for [g (get-in catalog ["vocabulary" "taskGrades"])] [g (section-fence root "task-grades.md" g)])
                (for [t (get-in catalog ["vocabulary" "topologies"])] [t (section-fence root "topologies.md" t)])
                (for [ps (get-in catalog ["vocabulary" "postures"])] [ps (section-fence root "postures.md" ps)])
                [["comms" (section-fence root "comms.md" "universal")]]
                (for [d ["gpt-5.6-luna" "gpt-5.6-terra" "gpt-5.6-sol" "opus" "sonnet"]]
                  [d (extract-first-fence (slurp (io/file root "docs" "deltas" (str d ".md"))))]))
        subs (map (fn [[k _]] (str "@throwaway:probe:" k)) blocks)
        total-bytes (reduce + (map (fn [[_ b]] (count (.getBytes ^String b "UTF-8"))) blocks))
        before (log-size)
        t0 (System/nanoTime)]
    (publish-actions!
     port
     (for [[k block] blocks]
       {:op :set :subject (str "@throwaway:probe:" k)
        :predicate "prompt_block" :values [block] :cardinality :one}))
    (let [write-ms (/ (- (System/nanoTime) t0) 1e6)
          after (log-size)
          lat (/ (reduce + (map #(query-latency-ms port %) subs)) (count subs))]
      (publish-actions!
       port
       (for [subject subs]
         {:op :set :subject subject :predicate "prompt_block" :values []
          :cardinality :one}))
      {:blocks (count blocks)
       :prompt_block_bytes total-bytes
       :log_growth_bytes (- after before)
       :write_ms (Math/round (double write-ms))
       :mean_query_ms (Double/parseDouble (format "%.3f" lat))})))

(defn retract-version! [port ver]
  ;; retract every fact whose subject is under @catalog:v<ver>: plus the pointer
  (let [prefix (str "@catalog:v" ver ":")
        rows (north.coord/query-rows
              port
              {:find "s,p,o"
               :rules [{:head {:rel "s,p,o" :args [{:var "s"} {:var "p"} {:var "o"}]}
                        :body [{:rel "triple" :args [{:var "s"} {:var "p"} {:var "o"}]}]}]})
        mine (filter (fn [[s _ _]] (str/starts-with? s prefix)) rows)]
    (publish-actions!
     port
     (concat
      (for [[subject predicate] (distinct (map (juxt first second) mine))]
        {:op :set :subject subject :predicate predicate :values []
         :cardinality :many})
      (when (= ver (current-version port))
        [{:op :set :subject POINTER :predicate "catalog_version" :values []
          :cardinality :one}])))
    (count mine)))

(defn show! [port ver]
  (let [prefix (str "@catalog:v" ver ":")
        rows (north.coord/query-rows
              port
              {:find "s" :rules [{:head {:rel "s" :args [{:var "s"}]}
                                   :body [{:rel "triple" :args [{:var "s"} "kind" {:var "k"}]}]}]})
        mine (sort (distinct (filter #(str/starts-with? % prefix) (map first rows))))]
    (println (format "pointer @catalog:current -> v%s (%d subjects)" ver (count mine)))
    (doseq [s mine] (println "  " s))))

(defn -main [& [ps verb arg]]
  (let [port (Integer/parseInt (or ps "7977"))]
    (case verb
      "import"  (let [ver (import! port (orchestration-home arg))]
                  (println (format "✓ imported catalog v%d on :%d; @catalog:current -> v%d" ver port ver)))
      "measure" (let [m (measure! port (orchestration-home arg))]
                  (println (json/generate-string m)))
      "retract" (let [ver (version-arg port arg)
                      n (retract-version! port ver)]
                  (println (format "✓ retracted %d facts under @catalog:v%d:" n ver)))
      "show"    (show! port (version-arg port arg))
      (do (println "usage: orchestration-import-cli.clj <port> {import|measure|retract <N|vN>|show [N|vN]} [orchestration-home]")
          (System/exit 2)))))

;; DUAL MODE (the projector's precedent): dormant when a sibling test load-file's
;; this as a library to exercise the pointer-cardinality helpers against stubs.
(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
