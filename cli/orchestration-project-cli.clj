;; orchestration-project-cli.clj — Phase 1 catalog PROJECTOR for the Orchestration ->
;; North Orchestration migration (thread 019f8f5c-74e0-7be7-ba65-3179f1bccde1).
;;
;; Reconstructs the canonical JSON of the imported catalog subgraph (staffing
;; catalog.json / providers/{anthropic,openai}.json) from graph facts, reading
;; via the @catalog:current pointer so it always sees the atomically-flipped
;; version. This ONE projection is consumed twice: the equality gate proves it
;; is byte-equal (after normalization) to the source files, and the TS dual-read
;; path (NORTH_STAFFING_SOURCE=graph) feeds it through the existing loaders.
;;
;; usage:
;;   bb orchestration-project-cli.clj <port> staffing            catalog.json projection
;;   bb orchestration-project-cli.clj <port> provider <name>     providers/<name>.json projection
;;   bb orchestration-project-cli.clj <port> policy-pin          §3.2 three-way rule digests
;;   bb orchestration-project-cli.clj <port> catalog-pin         §3.1(6) receipt graph-digest + tx watermark
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[cheshire.core :as json])

;; Resolve siblings relative to THIS file (not the entry script) so a test can
;; load-file the projector as a library — *file* is the loading file in both
;; direct-run and load-file modes; babashka.file is only ever the entry script.
(def CLI-DIR (.getParent (io/file *file*)))
(load-file (str CLI-DIR "/coord.clj"))
(load-file (str CLI-DIR "/orchestration-selection.clj"))
(def send-op north.coord/send-op)
(def enumerate-selection-rules north.orchestration-selection/enumerate-selection-rules)
(def rule-map                  north.orchestration-selection/rule-map)
(def rules-digest              north.orchestration-selection/rules-digest)

(def POINTER "@catalog:current")
(def REASONING-RANK ["low" "medium" "high" "xhigh" "max"])
(defn by-reasoning [xs] (sort-by #(.indexOf REASONING-RANK %) xs))

;; STRICT query envelope. A rules query that times out (query-time-limit) or
;; errors comes back WITHOUT an :ok vector; the old `(:ok resp)` silently became
;; nil -> an empty result set -> a downstream `(parse-long nil)` NPE that named
;; neither the failure nor the model. Fail loud instead, carrying the original
;; coordinator error and the subject/model context so the packaged-JSON fallback
;; upstream can log exactly what the graph could not answer.
(defn query-rows!
  [port query context]
  (let [resp (send-op port {:op :query :query query})]
    (if (vector? (:ok resp))
      (:ok resp)
      (throw (ex-info (str "catalog projection query failed for " context)
                      {:type :catalog-projection-query-failed
                       :context context
                       :error (:error resp)
                       :code (:code resp)})))))

(defn current-version [port]
  (let [resp (send-op port {:op :resolved :te POINTER :p "catalog_version"})]
    (when (or (contains? resp :error) (contains? resp :code))
      (throw (ex-info "catalog projection failed resolving @catalog:current"
                      {:type :catalog-projection-query-failed
                       :context "@catalog:current version"
                       :error (:error resp)
                       :code (:code resp)})))
    ;; :value is the coexist-elect winner (earliest fact), so an appended pointer
    ;; would project a STALE version silently — refuse instead of electing one.
    (when (:ambiguous? resp)
      (throw (ex-info "@catalog:current holds multiple catalog_version values — pointer flip did not supersede"
                      {:type :catalog-pointer-ambiguous :values (:values resp)})))
    (or (some-> (:value resp) parse-long)
        (throw (ex-info "no @catalog:current pointer — import first"
                        {:type :catalog-pointer-missing})))))

(defn facts
  "All (p o) facts for one subject through the coordinator's indexed show op.
   A Datalog query for this exact shape still pays the per-version query-engine
   warmup; under concurrent admission that warmup exhausted :query-time-limit
   before the already-ground subject lookup ran."
  [port subj]
  (let [resp (send-op port {:op :show :te subj})]
    (if (vector? (:rows resp))
      (reduce (fn [m [p o]] (update m p (fnil conj []) o)) {} (:rows resp))
      (throw (ex-info (str "catalog projection query failed for facts of " subj)
                      {:type :catalog-projection-query-failed
                       :context (str "facts of " subj)
                       :error (:error resp)
                       :code (:code resp)})))))

(defn one [f p] (first (get f p)))
(defn many [f p] (vec (get f p)))
;; Required scalar field: a graph row missing it is a data defect, not an empty
;; projection. Surface WHICH subject/field is malformed (the bar's "named-model
;; error, never a crash") rather than feeding nil into parse-long.
(defn one! [f p subj]
  (or (one f p)
      (throw (ex-info (str "catalog projection: " subj " is missing required field " p)
                      {:type :catalog-projection-missing-field :subject subj :field p}))))
(defn long! [f p subj] (parse-long (one! f p subj)))
;; `name` is an engine-reserved predicate (unwritable), so a subject's display
;; name is derived from its id's last colon-segment, not stored.
(defn id-name [subj] (last (str/split subj #":")))

(defn subjects-of-kind
  "Version-scoped subject ids carrying kind=k."
  [port ver k]
  (let [prefix (str "@catalog:v" ver ":")]
    (->> (query-rows! port
                      {:find "s" :rules [{:head {:rel "s" :args [{:var "s"}]}
                                          :body [{:rel "triple" :args [{:var "s"} "kind" k]}]}]}
                      (str "subjects of kind " k))
         (map first)
         (filter #(str/starts-with? % prefix))
         sort)))

;; ---------------------------------------------------------------------------
;; Staffing projection.
;; ---------------------------------------------------------------------------
(def AXIS-KEY {"task_grade" "taskGrades" "tier" "semanticTiers" "reasoning" "deliberations"
               "topology" "topologies" "posture" "postures" "capability" "capabilities"})

(defn project-staffing [port]
  (let [ver (current-version port)
        st (facts port (str "@catalog:v" ver ":staffing"))
        axis-values (subjects-of-kind port ver "axis_value")
        by-axis (reduce (fn [m s]
                          (let [f (facts port s)]
                            (update m (one! f "axis" s) (fnil conj [])
                                    [(long! f "rank" s) (id-name s)])))
                        {} axis-values)
        vocab (reduce (fn [m [axis vk]]
                        (assoc m vk (mapv second (sort-by first (get by-axis axis)))))
                      {} AXIS-KEY)
        ;; preset capability arrays are listed in vocabulary (capability-axis
        ;; rank) order in the source, so reproduce that order — not lexical.
        cap-rank (into {} (map (fn [[r n]] [n r]) (get by-axis "capability")))
        presets (for [s (subjects-of-kind port ver "template")]
                  (let [f (facts port s)]
                    {"name" (id-name s)
                     "taskGrade" (one f "task_grade")
                     "tier" (one f "tier")
                     "deliberation" (one f "reasoning")
                     "topology" (one f "topology")
                     "posture" (one f "posture")
                     "capabilities" (vec (sort-by cap-rank (many f "capability")))
                     "tagline" (one f "tagline")
                     "description" (one f "doc")}))]
    {"$schema" "./catalog.schema.json"
     "version" (long! st "catalog_version" "@catalog:staffing")
     "vocabulary" vocab
     "defaults" {"taskGrade" (one st "default_task_grade")
                 "tier" (one st "default_tier")
                 "deliberation" (one st "default_reasoning")
                 "topology" (one st "default_topology")
                 "posture" (one st "default_posture")}
     "presets" (vec presets)
     "aliases" []}))

;; ---------------------------------------------------------------------------
;; Provider projection. Levels unify on the "reasoning" knob (the graph's
;; canonical deliberation vocabulary); normalization renames the file's
;; "efforts"/"defaultEffort" to match — the design's byte-equal-after-
;; normalization contract.
;; ---------------------------------------------------------------------------
(defn project-provider [port provider]
  (let [ver (current-version port)
        prefix (str "@catalog:v" ver ":")
        p (facts port (str prefix "provider:" provider))
        model-subjs (filter #(str/starts-with? % (str prefix "model:" provider ":"))
                            (subjects-of-kind port ver "model"))
        tier-subjs (filter #(str/starts-with? % (str prefix "tier-row:" provider ":"))
                          (subjects-of-kind port ver "tier_row"))
        model-facts (into {} (map (fn [s] [(id-name s) (facts port s)]) model-subjs))
        aliases (into {} (for [[m f] model-facts a (many f "alias")] [a m]))
        models (into {} (for [[m f] model-facts]
                          (let [routes (reduce (fn [acc r]
                                                 (let [[tier lvl] (str/split r #"/")]
                                                   (update acc tier (fnil conj []) lvl)))
                                               {} (many f "calibrated_route"))]
                            [m (cond-> {"reasoning" (by-reasoning (many f "deliberation_support"))
                                        "contextWindow" {"tokens" (long! f "context_window_tokens" (str provider ":" m))
                                                         "effectiveFrom" (one! f "context_window_from" (str provider ":" m))}}
                                 (seq routes)
                                 (assoc "routes" (into {} (map (fn [[t ls]] [t (by-reasoning ls)]) routes))))])))
        deltas (into {} (for [[m f] model-facts]
                          [m (if (= "calibrated" (one f "delta_kind"))
                               {"kind" "calibrated" "path" (one f "doctrine_source")}
                               {"kind" (one f "delta_kind") "reason" (one f "delta_reason")})]))
        tiers (into {} (for [s tier-subjs]
                         (let [f (facts port s)]
                           [(one f "tier") {"model" (one f "model")
                                            "reasoning" (by-reasoning (many f "level"))
                                            "defaultReasoning" (one f "default_level")}])))]
    {"$schema" "./catalog.schema.json"
     "provider" provider
     "provenance" {"asOf" (one p "as_of")
                   "reviewAfter" (one p "review_after")
                   "sources" (mapv json/parse-string (many p "provenance_source"))}
     "transports" (many p "transport")
     "modelAliases" aliases
     "models" models
     "modelDeltas" deltas
     "tiers" tiers}))

;; ---------------------------------------------------------------------------
;; §3.2 digest pin. Three digests over the canonical selection-rule table that
;; MUST be equal for admission to proceed (the TS consumer refuses otherwise):
;;   storedSha256     — the policy_sha256 fact the importer wrote.
;;   projectionSha256 — recomputed here from the live rule subjects (a bare
;;                      graph write to a floor changes THIS but not the stored
;;                      fact, so the pin catches it).
;;   validatorSha256  — enumerated from the canonical validator's baked table
;;                      (changing a floor without a validator/policy version
;;                      bump changes stored+projection but not THIS).
;; A floor therefore moves only by a policy version bump, never a bare write.
;; ---------------------------------------------------------------------------
;; Derive the repo root from THIS file's own location (CLI-DIR is <root>/cli,
;; resolved above from *file* — direct-run and load-file modes alike), never
;; user.dir: a `bb` subprocess launched by execFileSync (e.g. from
;; sdk/src/orchestration-policy-pin.ts) inherits the CALLER's cwd, not the
;; north checkout, so a bare user.dir fallback silently walks to
;; <caller-cwd>/orchestration and misses the real one.
(def ^:private this-root (.getParent (io/file CLI-DIR)))

(defn orchestration-root []
  (or (System/getenv "NORTH_ORCHESTRATION_HOME")
      (str (or (System/getenv "NORTH_HOME") this-root (System/getProperty "user.dir")) "/orchestration")))

(def MAX-POLICY-RULES 128)
(def MAX-POLICY-RULE-FACTS 4096)
(def POLICY-SCOPED-PROJECTION-DEADLINE-MS 5000)
;; Malformed scoped success may be tampering; only transport/timeouts may fall back.
(def ^:private scoped-fallback-types
  #{:coordinator-operation-timeout :coordinator-response-timeout
    :coordinator-response-closed})

(defn- fold-rule-rows [rule-subjs rows]
  (let [allowed (set rule-subjs)
        returned (set (map first rows))]
    (when-not (= allowed returned)
      (throw (ex-info "policy rule projection did not return every linked subject"
                      {:type :catalog-projection-query-failed
                       :context "selection policy rule subjects"
                       :missing (vec (sort (remove returned allowed)))})))
    (reduce (fn [out [subject predicate value]]
              (update-in out [subject predicate] (fnil conj []) value))
            {}
            rows)))

(defn- scoped-rule-facts [port rule-subjs]
  (let [allowed (set rule-subjs)
        response
        (try
          (binding [north.coord/*request-deadline-ns*
                    (north.coord/request-deadline-ns
                     POLICY-SCOPED-PROJECTION-DEADLINE-MS)]
            (send-op port {:op :facts-for-subjects :subjects rule-subjs}))
          (catch clojure.lang.ExceptionInfo error
            (if (scoped-fallback-types (:type (ex-data error)))
              (throw (ex-info "scoped policy rule projection is unavailable"
                              {:type :policy-scoped-projection-unavailable}
                              error))
              (throw error))))]
    (when (and (map? response)
               (#{:request-timeout :query-time-limit} (:code response)))
      (throw (ex-info "scoped policy rule projection timed out"
                      {:type :policy-scoped-projection-unavailable
                       :code (:code response)})))
    (when-not (and (map? response)
                   (= #{:version :log :facts} (set (keys response)))
                   (integer? (:version response))
                   (not (neg? (:version response)))
                   (string? (:log response))
                   (vector? (:facts response))
                   (<= (count (:facts response)) MAX-POLICY-RULE-FACTS)
                   (every? (fn [row]
                             (and (vector? row)
                                  (= 3 (count row))
                                  (every? string? row)
                                  (contains? allowed (first row))))
                           (:facts response)))
      (throw (ex-info "scoped policy rule projection was malformed"
                      {:type :catalog-projection-query-failed
                       :context "selection policy rule subjects"})))
    (fold-rule-rows rule-subjs (:facts response))))

(defn- parallel-rule-facts [port rule-subjs]
  (let [reads (mapv (fn [subject] [subject (future (facts port subject))]) rule-subjs)
        results (mapv (fn [[subject read]]
                        (try [subject @read nil]
                             (catch Throwable error [subject nil error])))
                      reads)]
    (when-let [error (some #(nth % 2) results)] (throw error))
    (let [projected (into {} (map (fn [[subject rule-facts _]]
                                    [subject rule-facts])
                                  results))]
      (when-not (= (set rule-subjs)
                   (set (keep (fn [[subject rule-facts]]
                                (when (seq rule-facts) subject))
                              projected)))
        (throw (ex-info "parallel policy rule projection did not return every linked subject"
                        {:type :catalog-projection-query-failed
                         :context "selection policy rule subjects"})))
      projected)))

(defn- project-rule-facts [port rule-subjs]
  (when (> (count rule-subjs) MAX-POLICY-RULES)
    (throw (ex-info "selection policy links too many rules"
                    {:type :catalog-projection-query-failed
                     :context "selection policy rule subjects"
                     :count (count rule-subjs)})))
  (if (empty? rule-subjs)
    {}
    (try
      (scoped-rule-facts port rule-subjs)
      (catch clojure.lang.ExceptionInfo error
        (if (= :policy-scoped-projection-unavailable (:type (ex-data error)))
          (parallel-rule-facts port rule-subjs)
          (throw error))))))

(defn project-policy-pin [port]
  (let [ver (current-version port)
        policy (str "@catalog:v" ver ":selection-policy:minimum-sufficient-v1")
        pf (facts port policy)
        stored (one pf "policy_sha256")
        rule-subjs (vec (distinct (many pf "rule")))
        rule-facts (project-rule-facts port rule-subjs)
        graph-rules (for [s rule-subjs]
                      (let [f (get rule-facts s)]
                        (rule-map (one f "signal") (one f "signal_value")
                                  (one f "rule_code") (one f "min_tier") (one f "min_reasoning"))))
        validator-rules (enumerate-selection-rules (orchestration-root))]
    {"policyVersion" "minimum-sufficient-v1"
     "catalogVersion" ver
     "storedSha256" stored
     "projectionSha256" (rules-digest graph-rules)
     "validatorSha256" (rules-digest validator-rules)}))

;; ---------------------------------------------------------------------------
;; §3.1 point 6 — receipt catalog pin. The admission receipt's catalog-FILE
;; sha256s (staffingCatalogSha256/providerCatalogsSha256 in routing-economics.ts,
;; computed over Orchestration JSON on disk) are replaced, under NORTH_STAFFING_SOURCE=
;; graph, by (a) the digest of the canonical JSON projection of the catalog
;; subgraph and (b) two version watermarks — so the receipt names the EXACT graph
;; state admission accepted rather than a file the graph may no longer mirror:
;;   catalogVersion      — the @catalog:current pointer version (which versioned
;;                         subgraph was projected).
;;   coordinatorVersion  — the daemon's global tx watermark at projection time
;;                         (design §3.1's "tell-ack version", e.g. v322995).
;;   catalogDigestSha256 — sha256 over canonical JSON of {staffing, providers}.
;; The digest is computed here on a cold/version-changed projection, then read
;; from the same validated durable projection record as the loaders. Imported
;; version namespaces are immutable after the atomic pointer flip; therefore a
;; catalog change advances catalogVersion and forces recomputation, while
;; unrelated coordination writes only refresh the returned tx watermark.
;; ---------------------------------------------------------------------------
(defn- canon
  "Recursively sort map keys so the JSON serialization is order-independent."
  [x]
  (cond
    (map? x)        (into (sorted-map) (map (fn [[k v]] [k (canon v)]) x))
    (sequential? x) (mapv canon x)
    :else           x))

(defn- sha256-hex [^String s]
  (let [md (java.security.MessageDigest/getInstance "SHA-256")
        bs (.digest md (.getBytes s java.nio.charset.StandardCharsets/UTF_8))]
    (str/join (map #(format "%02x" (bit-and % 0xff)) bs))))

(def MAX-CATALOG-CACHE-BYTES (* 4 1024 1024))

(defn catalog-projection-cache-path []
  (or (some-> (System/getenv "NORTH_ORCHESTRATION_CATALOG_CACHE") str/trim not-empty)
      (str (or (some-> (System/getenv "XDG_STATE_HOME") str/trim not-empty)
               (str (or (System/getenv "HOME") (System/getProperty "user.home"))
                    "/.local/state"))
           "/north/orchestration-catalog-projection-cache.json")))

(defn cached-catalog-pin
  "Validate the durable projection record without touching the graph's costly
   kind scans. Imported @catalog:vN namespaces are write-once drafts followed
   by one atomic @catalog:current flip, so catalogVersion is the invalidation
   boundary; ordinary coordination writes only advance coordinatorVersion."
  [ver coord-ver]
  (try
    (let [f (io/file (catalog-projection-cache-path))]
      (when (and (.isFile f) (pos? (.length f)) (<= (.length f) MAX-CATALOG-CACHE-BYTES)
                 (not (java.nio.file.Files/isSymbolicLink (.toPath f))))
        (let [record (json/parse-string (slurp f))
              bundle (get record "bundle")
              subgraph {"staffing" (get bundle "staffing")
                        "providers" (get bundle "providers")}
              digest (sha256-hex (json/generate-string (canon subgraph)))
              recorded (get record "catalogDigestSha256")]
          (when (and (= 1 (get record "version"))
                     (map? bundle)
                     (map? (get bundle "staffing"))
                     (map? (get bundle "providers"))
                     (= ver (get record "catalogVersion"))
                     (= ver (get bundle "catalogVersion"))
                     (integer? (get record "coordinatorVersion"))
                     (string? recorded)
                     (re-matches #"[0-9a-f]{64}" recorded)
                     (= digest recorded))
            {"catalogVersion" ver
             "coordinatorVersion" coord-ver
             "catalogDigestSha256" recorded}))))
    (catch Exception _ nil)))

(defn project-catalog-pin [port]
  (let [ver (current-version port)
        coord-ver (:version (send-op port {:op :version}))]
    (or (cached-catalog-pin ver coord-ver)
        (let [subgraph {"staffing"  (project-staffing port)
                        "providers" {"anthropic" (project-provider port "anthropic")
                                     "openai"    (project-provider port "openai")}}]
          {"catalogVersion"      ver
           "coordinatorVersion"  coord-ver
           "catalogDigestSha256" (sha256-hex (json/generate-string (canon subgraph)))}))))

;; ---------------------------------------------------------------------------
;; Whole-catalog BUNDLE — one process, one @catalog:current version, both
;; consumers. The TS admission path used to shell a fresh `bb` per
;; resolve/support/context/delta call (an N+1 across the spawn hot path); each
;; cold projection re-pays the coordinator's per-version scan warmup. Projecting
;; staffing + every provider ONCE, pinned to a single version read, lets the SDK
;; cache one bundle per admission process and collapse that N+1 to a single
;; subprocess.
;; ---------------------------------------------------------------------------
(defn project-bundle [port]
  (let [ver (current-version port)]
    {"catalogVersion" ver
     "staffing"  (project-staffing port)
     "providers" {"anthropic" (project-provider port "anthropic")
                  "openai"    (project-provider port "openai")}}))

(defn -main [& [ps verb arg]]
  (let [port (Integer/parseInt (or ps "7977"))]
    (case verb
      "staffing"    (println (json/generate-string (project-staffing port)))
      "provider"    (println (json/generate-string (project-provider port arg)))
      "bundle"      (println (json/generate-string (project-bundle port)))
      "policy-pin"  (println (json/generate-string (project-policy-pin port)))
      "catalog-pin" (println (json/generate-string (project-catalog-pin port)))
      (do (println "usage: orchestration-project-cli.clj <port> {staffing | provider <name> | bundle | policy-pin | catalog-pin}")
          (System/exit 2)))))

;; DUAL MODE (the coord.clj precedent): the main-guard keeps the projector
;; dormant when a sibling test load-file's it as a library to exercise the
;; strict-envelope + named-field helpers against a stubbed send-op.
(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
