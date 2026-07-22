#!/usr/bin/env bb
;; Canonical executable schema migration for North.
;;
;; A predicate has exactly one authority: the Fram predicate entity @<name>.
;; cardinality, value_kind, acyclic, doc, entity_kind, and any extension metadata
;; are ordinary facts on that entity.  The old @pred:<name> catalog is never read
;; here; pred-cli now renders the executable entities directly.
;;
;; The migration is deliberately additive for valid declarations.  Existing
;; executable metadata wins. BOOTSTRAP VOCAB and the fixed legacy-kernel singleton
;; set are used only to fill an absent declaration during the one-time cutover;
;; caller environment fallback is deliberately excluded from sealed planning.
;; Once every live predicate has explicit facts, engine fallback is inert.
;;
;; Run through bin/north (it supplies Fram's classpath and corpus paths):
;;   north schema-migrate plan
;;   north schema-migrate migrate                         # plan-only compatibility
;;   north schema-migrate build-candidate --execute \
;;     --offline-confirm --manifest REVIEWED.edn          # disposable copies only
;;   north schema-migrate audit --strict
;;   north schema-migrate repair-corrupt                 # diagnostics only
(require '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str]
         '[fram.fold :as fold]
         '[fram.kernel :as kernel]
         '[fram.rt :as rt])

(def SCHEMA-MIGRATE-SOURCE
  (.getCanonicalPath (io/file (or *file* (System/getProperty "babashka.file")))))

(load-file (str (.getParent (io/file SCHEMA-MIGRATE-SOURCE)) "/coord.clj"))

(def VALID-PREDICATE #"^[A-Za-z][A-Za-z0-9_-]*(?:/[A-Za-z][A-Za-z0-9_-]*)?$")
(def VALID-ENTITY-KIND #"^[a-z][a-z0-9_-]*(?:/[a-z][a-z0-9_-]*)?$")
(def META-PREDICATES #{"cardinality" "value_kind" "acyclic"})
(def ACYCLIC-PREDICATES #{"depends_on" "part_of"})
(def REPAIR-MANIFEST-FORMAT "north-schema-repair-manifest/v1")
(def PLAN-FORMAT "north-schema-plan/v2")
(def AUDIT-FORMAT "north-schema-audit/v2")

;; Fram's transitional fallback is copied only into the migration input.  The
;; migration materializes each applicable value as @p cardinality single; no
;; North runtime decision reads this set after migration.
(def LEGACY-KERNEL-SINGLE
  #{"title" "owner" "lead" "driver" "source" "part_of" "do_on" "valid_until"
    "estimate_hours" "created_at" "updated_at" "name" "body" "created_by"
    "committed" "outcome" "abandoned" "superseded_by" "merged_into"
    "session_of" "start_time" "end_time" "clockify_id"})

(def CORE-ENTITY-KINDS
  (sorted-map
   "agent" "A human-facing or managed execution identity."
   "client_session" "One human/client billing-clock interval; never a managed run."
   "concern" "A concurrent implementation footprint and intent declaration."
   "guard_denial" "An admission-guard refusal with its diagnostic evidence."
   "message" "A durable coordination message or peer command envelope."
   "person" "A human identity with a display_name."
   "predicate" "An executable Fram predicate entity carrying its own schema facts."
   "run" "One managed task-duration and delivery telemetry record."
   "thread" "A durable unit of intended or possible work."
   "topic" "A thread-shaped relatedness anchor."))

(def LEGACY-ENTITY-KINDS
  (sorted-map
   "north/clock_audit_run" "One historical clock-coverage audit execution."
   "north/integration_link" "A deterministic external-integration identity link."
   "north/legacy_agent_session" "A pre-run-model managed-agent timing session; audit-only and never billable."
   "north/legacy_human_session" "A pre-client-session human billing interval retained for historical billing."
   "north/legacy_schema_projection" "A deprecated @pred:* catalog projection; never executable schema authority."
   "north/legacy_session" "A historical session whose actor was not recorded."
   "north/linear_bootstrap_reservation" "A deterministic Linear bootstrap reservation."
   "north/test_fixture" "A historical test or scratch entity retained outside domain authority."))

(def ENTITY-KINDS (into (sorted-map) (concat CORE-ENTITY-KINDS LEGACY-ENTITY-KINDS)))

(def ENTITY-KIND-DEFINITION "north/entity_kind_definition")

(def LEGACY-KIND->ENTITY-KIND
  {"agent" "agent" "lane" "agent" "managed" "agent" "session" "agent"
   "client_session" "client_session"
   "concern" "concern"
   "guard_denial" "guard_denial"
   "message" "message" "msg" "message" "command" "message"
   "mine" "north/mine" "snapshot" "north/snapshot"
   "person" "person"
   "predicate" "predicate"
   "run" "run"
   "thread" "thread"
   "topic" "topic"
   "clock_audit_run" "north/clock_audit_run"
   "integration_link" "north/integration_link"
   "linear_bootstrap_reservation" "north/linear_bootstrap_reservation"})

(def BUILTIN-SCHEMA
  {"acyclic" {:card "single" :kind "literal"
              :doc "Whether this reference predicate must remain cycle-free (true enables the rule)."}
   "cardinality" {:card "single" :kind "literal"
                  :doc "Executable Fram cardinality: single or multi."}
   "doc" {:card "single" :kind "literal"
          :doc "Human-readable documentation attached to a schema entity."}
   "entity_kind" {:card "single" :kind "literal"
                  :doc "Open structural entity taxonomy; core values are defined by @entity-kind:* and extensions use namespace/name."}
   "entity_kind_name" {:card "single" :kind "literal"
                       :doc "Canonical value represented by an @entity-kind:* definition entity."}
   "value_kind" {:card "single" :kind "literal"
                 :doc "Executable Fram object kind: literal or ref."}})

;; These predicates predate the connected registry but their semantics are not
;; inferable from object spelling. `notify` is prose even when it begins with an
;; @handle. `blocks` remains a reference edge; prose-valued legacy rows require
;; an exact manifest repair into a distinct literal reason/note predicate.
(def CURATED-CUTOVER-SCHEMA
  {"notify" {:card "multi" :kind "literal"
             :doc "Durable human-readable completion, stall, or coordination notification."}
   "blocks" {:card "multi" :kind "ref"
             :doc "Reference edge from a blocker to the entity it blocks; explanatory prose belongs in a reason/note predicate."}
   "depends_on" {:card "multi" :kind "ref" :acyclic "true"
                 :doc "Dependency edge from an entity to a prerequisite that must precede it."}
   "part_of" {:card "single" :kind "ref" :acyclic "true"
              :doc "Containment edge from a child entity to its single parent."}})

(def LEGACY-SESSION-SIGNATURES
  #{#{"clocked_by" "end_time" "session_of" "start_time"}
    #{"end_time" "session_of" "start_time"}
    #{"clock_orphaned" "clocked_by" "end_time" "session_of" "start_time"}
    #{"clocked_by" "session_of" "start_time"}})

(def TEST-FIXTURE-SIGNATURES
  #{#{"agg_done_batch" "agg_done_worker"}
    #{"agg_run_batch" "agg_run_tokens"}
    #{"agg_charge_tokens" "agg_charged_to"}
    #{"end_time" "start_time"}})

(def LEGACY-SCHEMA-PROJECTION-SIGNATURE
  #{"doc" "minted_at" "minted_by" "pred_cardinality" "pred_value_kind"})

(defn script-dir [] (.getParent (io/file SCHEMA-MIGRATE-SOURCE)))
(defn pred-cli-path [] (str (script-dir) "/pred-cli.clj"))

(defn read-forms [path]
  (with-open [rdr (java.io.PushbackReader. (io/reader path))]
    (let [eof (Object.)]
      (loop [acc []]
        (let [form (read {:eof eof :read-cond :allow} rdr)]
          (if (= eof form) acc (recur (conj acc form))))))))

(defn literal-def [path sym]
  (some (fn [form]
          (when (and (seq? form) (= 'def (first form)) (= sym (second form)))
            (nth form 2 nil)))
        (read-forms path)))

;; The code table is bootstrap material and offline-lint inventory, not a live
;; schema read path.  Valid graph facts always win over it.
(defn bootstrap-schema []
  (let [rows (or (literal-def (pred-cli-path) 'VOCAB) [])]
    (merge
     (into {} (keep (fn [row]
                      (when (and (vector? row) (>= (count row) 4))
                        [(nth row 0) {:card (nth row 1)
                                      :kind (nth row 2)
                                      :doc (nth row 3)}]))
                    rows))
     CURATED-CUTOVER-SCHEMA
     BUILTIN-SCHEMA)))

(defn file-sha256 [path]
  (let [md (java.security.MessageDigest/getInstance "SHA-256")]
    (with-open [in (io/input-stream path)]
      (let [buf (byte-array 65536)]
        (loop []
          (let [n (.read in buf)]
            (when (pos? n)
              (.update md buf 0 n)
              (recur))))))
    (format "%064x" (java.math.BigInteger. 1 (.digest md)))))

(defn text-sha256 [s]
  (let [md (java.security.MessageDigest/getInstance "SHA-256")]
    (.update md (.getBytes (str s) java.nio.charset.StandardCharsets/UTF_8))
    (format "%064x" (java.math.BigInteger. 1 (.digest md)))))

(defn configured-path? [path]
  (and (string? path) (not (str/blank? path))))

(defn canonical-readable-file! [role path]
  (when-not (configured-path? path)
    (throw (ex-info (str role " corpus log path is required")
                    {:type :missing-corpus-path :role role :path path})))
  (let [file (io/file path)]
    (when-not (and (.isFile file) (.canRead file))
      (throw (ex-info (str role " corpus log is missing or unreadable: " path)
                      {:type :unreadable-corpus-path :role role :path path})))
    (.getCanonicalPath file)))

(defn distinct-corpus-paths! [paths]
  (when-not (= (count paths) (count (set paths)))
    (throw (ex-info "coordination and telemetry corpus logs resolve to the same canonical path"
                    {:type :duplicate-corpus-path :paths paths})))
  paths)

(defn resolve-corpus-paths! [coordination telemetry]
  (distinct-corpus-paths!
   [(canonical-readable-file! "coordination" coordination)
    (canonical-readable-file! "telemetry" telemetry)]))

(defn op-tx [op]
  (let [tx (:tx op)] (if (integer? tx) tx 0)))

(defn effective-single?
  "Cutover planning deliberately models the selected no-env runtime: explicit
  graph cardinality wins, then Fram's transitional kernel fallback. The caller's
  FRAM_SINGLE_VALUED never changes a plan hash or hides a production conflict."
  [card-map predicate]
  (if (contains? card-map predicate)
    (true? (get card-map predicate))
    (contains? LEGACY-KERNEL-SINGLE predicate)))

(defn fold-for-cutover [ops]
  (let [valid (filterv #(and (some? (:l %)) (some? (:p %)) (some? (:r %))) ops)
        cards (fold/card-map valid)
        keyed
        (reduce (fn [latest op]
                  (let [key (if (effective-single? cards (:p op))
                              [(:l op) (:p op)]
                              [(:l op) (:p op) (:r op)])
                        prior (get latest key)]
                    (if (and prior (> (op-tx prior) (op-tx op)))
                      latest
                      (assoc latest key op))))
                {} valid)
        facts (->> (vals keyed)
                   (filter #(= "assert" (:op %)))
                   (mapv #(select-keys % [:l :p :r])))]
    {:facts facts :version (fold/max-tx ops) :card_map cards}))

(defn read-corpus [paths]
  (when (empty? paths)
    (throw (ex-info "coordination corpus log path is required"
                    {:type :missing-corpus-path :role "coordination"})))
  (let [paths (->> paths
                   (mapv #(canonical-readable-file! "configured" %))
                   distinct-corpus-paths!)
        ops (vec (mapcat rt/read-log paths))
        folded (fold-for-cutover ops)]
    {:paths paths
     :ops ops
     :facts (:facts folded)
     :version (:version folded)
     :card_map (:card_map folded)
     :files (mapv (fn [path]
                    {:path (.getCanonicalPath (io/file path))
                     :bytes (.length (io/file path))
                     :sha256 (file-sha256 path)})
                  paths)}))

(defn source-seal [corpus]
  {:fold_version (:version corpus)
   :corpus (mapv (fn [role file]
                   {:role role :bytes (:bytes file) :sha256 (:sha256 file)})
                 ["coordination" "telemetry"] (:files corpus))})

(defn nonblank-string? [value]
  (and (string? value) (not (str/blank? value))))

(defn reviewed-string? [value]
  (and (nonblank-string? value)
       (not (re-find #"[A-Z][A-Z0-9_]*_REQUIRED" value))))

(defn manifest-predicate-semantics [manifest]
  (or (:predicate_semantics manifest) {}))

(defn manifest-other-entries [manifest]
  (or (get-in manifest [:other_allowlist :entries]) {}))

(defn normalize-manifest-schema-row [row]
  {:card (:cardinality row)
   :kind (:value_kind row)
   :doc (:doc row)
   :acyclic (:acyclic row)})

(declare registrable-predicate?)

(defn validate-manifest-structure! [manifest]
  (when-not (= REPAIR-MANIFEST-FORMAT (:format manifest))
    (throw (ex-info "unsupported or missing schema repair manifest format"
                    {:type :invalid-repair-manifest-format
                     :expected REPAIR-MANIFEST-FORMAT :actual (:format manifest)})))
  (when-not (and (map? (:source manifest))
                 (integer? (get-in manifest [:source :fold_version]))
                 (= ["coordination" "telemetry"]
                    (mapv :role (get-in manifest [:source :corpus]))))
    (throw (ex-info "repair manifest source seal is malformed"
                    {:type :invalid-repair-manifest-source})))
  (when-not (every? reviewed-string?
                    [(get-in manifest [:review :by])
                     (get-in manifest [:review :at])
                     (get-in manifest [:review :basis])])
    (throw (ex-info "repair manifest requires nonblank review by/at/basis"
                    {:type :unreviewed-repair-manifest})))
  (doseq [[predicate row] (manifest-predicate-semantics manifest)]
    (when-not (and (registrable-predicate? predicate)
                   (map? row)
                   (contains? #{"single" "multi"} (:cardinality row))
                   (contains? #{"literal" "ref"} (:value_kind row))
                   (reviewed-string? (:doc row))
                   (or (nil? (:acyclic row))
                       (contains? #{"true" "false"} (:acyclic row)))
                   (reviewed-string? (:rationale row)))
      (throw (ex-info (str "invalid reviewed predicate semantics for " (pr-str predicate))
                      {:type :invalid-predicate-semantics
                       :predicate predicate :row row}))))
  (when-let [allowlist (:other_allowlist manifest)]
    (when-not (and (reviewed-string? (:name allowlist))
                   (map? (:entries allowlist)))
      (throw (ex-info "other allowlist requires a name and subject entry map"
                      {:type :invalid-other-allowlist})))
    (doseq [[subject row] (:entries allowlist)]
      (when-not (and (nonblank-string? subject)
                     (map? row)
                     (nonblank-string? (:entity_kind row))
                     (re-matches VALID-ENTITY-KIND (:entity_kind row))
                     (reviewed-string? (:rationale row)))
        (throw (ex-info (str "invalid reviewed other classification for " (pr-str subject))
                        {:type :invalid-other-classification
                         :subject subject :row row})))))
  (doseq [repair (:cardinality_repairs manifest)]
    (when-not (and (map? repair)
                   (nonblank-string? (:subject repair))
                   (registrable-predicate? (:predicate repair))
                   (vector? (:retract repair))
                   (reviewed-string? (:policy repair))
                   (reviewed-string? (:rationale repair)))
      (throw (ex-info "cardinality repair entries require exact identity, retract vector, policy, and rationale"
                      {:type :invalid-cardinality-repair :repair repair}))))
  (doseq [repair (:fact_repairs manifest)]
    (when-not (and (map? repair)
                   (contains? #{"assert" "retract"} (:action repair))
                   (nonblank-string? (:subject repair))
                   (nonblank-string? (:predicate repair))
                   (string? (:value repair))
                   (reviewed-string? (:policy repair))
                   (reviewed-string? (:rationale repair)))
      (throw (ex-info "fact repair entries require an exact append action, triple, policy, and rationale"
                      {:type :invalid-fact-repair :repair repair}))))
  manifest)

(defn read-repair-manifest! [path corpus]
  (when-not (configured-path? path)
    (throw (ex-info "offline candidate construction requires --manifest PATH"
                    {:type :missing-repair-manifest})))
  (let [canonical (canonical-readable-file! "repair manifest" path)
        manifest (-> canonical slurp edn/read-string validate-manifest-structure!)]
    (when-not (= (source-seal corpus) (:source manifest))
      (throw (ex-info "repair manifest source seal does not match the exact input corpus"
                      {:type :repair-manifest-source-mismatch
                       :expected (source-seal corpus) :actual (:source manifest)})))
    (assoc manifest :_path canonical :_sha256 (file-sha256 canonical))))

(defn append-boundary-ready? [path]
  (let [file (io/file path)
        length (.length file)]
    (or (zero? length)
        (with-open [raf (java.io.RandomAccessFile. file "r")]
          (.seek raf (dec length))
          (= 10 (.read raf))))))

(defn require-append-boundaries! [paths]
  (doseq [path paths]
    (when-not (append-boundary-ready? path)
      (throw (ex-info
              (str "corpus log is not newline-terminated; coordinator append would merge two EDN records: " path)
              {:type :unsafe-corpus-append-boundary
               :path path}))))
  paths)

(defn registrable-predicate? [p]
  (and (string? p) (boolean (re-matches VALID-PREDICATE p))))

(defn facts-by-lp [facts]
  (reduce (fn [m fact]
            (update m [(:l fact) (:p fact)] (fnil conj #{}) (:r fact)))
          {} facts))

(defn values-at [by-lp subject predicate]
  (get by-lp [subject predicate] #{}))

(defn valid-singleton [values allowed]
  (when (and (= 1 (count values)) (contains? allowed (first values)))
    (first values)))

(defn live-predicate-values [facts]
  (reduce (fn [m fact]
            (if (registrable-predicate? (:p fact))
              (update m (:p fact) (fnil conj #{}) (:r fact))
              m))
          (sorted-map) facts))

(defn declared-predicate-names [facts]
  (->> facts
       (filter #(contains? META-PREDICATES (:p %)))
       (keep (fn [fact]
               (let [subject (:l fact)]
                 (when (and (string? subject) (str/starts-with? subject "@"))
                   (let [predicate (subs subject 1)]
                     (when (registrable-predicate? predicate) predicate))))))
       set))

(defn corrupt-facts [facts]
  (->> facts
       (remove #(registrable-predicate? (:p %)))
       (sort-by (juxt :l :p :r))
       vec))

(defn collisions [facts predicates]
  (let [by-lp (facts-by-lp facts)]
    (->> predicates
         (filter #(seq (values-at by-lp (str "@" %) "title")))
         sort vec)))

(defn preferred-doc [current policy]
  (or (when (= 1 (count current))
        (let [v (first current)] (when-not (str/blank? (str v)) (str v))))
      (let [v (:doc policy)]
        (when-not (str/blank? (str v)) (str v)))))

(defn desired-schema
  ([facts] (desired-schema facts nil))
  ([facts manifest]
   (let [by-lp (facts-by-lp facts)
         observed (live-predicate-values facts)
         bootstrap (bootstrap-schema)
         reviewed (into {} (map (fn [[predicate row]]
                                  [predicate (normalize-manifest-schema-row row)]))
                        (manifest-predicate-semantics manifest))
         predicates (sort (set/union (set (keys observed))
                                     (declared-predicate-names facts)
                                     (set (keys bootstrap))
                                     (set (keys reviewed))
                                     META-PREDICATES
                                     (set (keys BUILTIN-SCHEMA))))]
     (into (sorted-map)
           (map (fn [predicate]
                  (let [entity (str "@" predicate)
                        card-values (values-at by-lp entity "cardinality")
                        kind-values (values-at by-lp entity "value_kind")
                        doc-values (values-at by-lp entity "doc")
                        acyclic-values (values-at by-lp entity "acyclic")
                        existing-card (valid-singleton card-values #{"single" "multi"})
                        existing-kind (valid-singleton kind-values #{"literal" "ref"})
                        existing-acyclic (valid-singleton acyclic-values #{"true" "false"})
                        policy (merge (get bootstrap predicate) (get reviewed predicate))
                        cardinality (or existing-card
                                        (:card policy)
                                        (when (contains? LEGACY-KERNEL-SINGLE predicate) "single"))
                        value-kind (or existing-kind (:kind policy))
                        doc (preferred-doc doc-values policy)
                        unresolved (cond-> []
                                     (nil? cardinality) (conj "cardinality")
                                     (nil? value-kind) (conj "value_kind")
                                     (nil? doc) (conj "doc"))]
                    [predicate
                     {:cardinality cardinality
                      :value_kind value-kind
                      :doc doc
                      :unresolved_fields unresolved
                      :policy_source (cond
                                       (or existing-card existing-kind (seq doc-values)) "graph"
                                       (contains? reviewed predicate) "reviewed-manifest"
                                       (contains? bootstrap predicate) "curated-bootstrap"
                                       :else "unresolved")
                      ;; A valid explicit false is policy, not absence. Bootstrap
                      ;; only when no declaration exists at all; malformed or
                      ;; multiple values remain untouched for strict audit.
                      :acyclic (cond
                                 existing-acyclic existing-acyclic
                                 (empty? acyclic-values)
                                 (or (:acyclic policy)
                                     (when (contains? ACYCLIC-PREDICATES predicate) "true"))
                                 :else nil)}]))
                predicates)))))

(defn set-action [by-lp subject predicate value]
  (let [current (values-at by-lp subject predicate)]
    (when (and (some? value) (not= current #{value}))
      {:action "set" :subject subject :predicate predicate
       :value value :before (vec (sort current))})))

(defn schema-actions [facts schema]
  (let [by-lp (facts-by-lp facts)]
    (->> schema
         (mapcat (fn [[predicate row]]
                   (let [subject (str "@" predicate)]
                     (remove nil?
                             [(set-action by-lp subject "cardinality" (:cardinality row))
                              (set-action by-lp subject "value_kind" (:value_kind row))
                              (set-action by-lp subject "doc" (:doc row))
                              (set-action by-lp subject "entity_kind" "predicate")
                              (when-let [acyclic (:acyclic row)]
                                (set-action by-lp subject "acyclic" acyclic))]))))
         (sort-by (juxt :subject :predicate))
         vec)))

(defn entity-kind-definition-actions [facts]
  (let [by-lp (facts-by-lp facts)]
    (->> ENTITY-KINDS
         (mapcat (fn [[kind doc]]
                   (let [subject (str "@entity-kind:" kind)]
                     (remove nil?
                             [(set-action by-lp subject "entity_kind" ENTITY-KIND-DEFINITION)
                              (set-action by-lp subject "entity_kind_name" kind)
                              (set-action by-lp subject "doc" doc)]))))
         (sort-by (juxt :subject :predicate))
         vec)))

(defn subject-bare [subject]
  (let [s (str subject)] (if (str/starts-with? s "@") (subs s 1) s)))

(defn one-valid-entity-kind [values]
  (when (and (= 1 (count values))
             (string? (first values))
             (re-matches VALID-ENTITY-KIND (first values)))
    (first values)))

(defn deterministic-legacy-kind [by-lp subject predicates]
  (let [clocked-by (values-at by-lp subject "clocked_by")
        bare (subject-bare subject)]
    (cond
      (contains? LEGACY-SESSION-SIGNATURES predicates)
      (cond
        (empty? clocked-by) "north/legacy_session"
        (= #{"user"} clocked-by) "north/legacy_human_session"
        (contains? clocked-by "user") nil
        :else "north/legacy_agent_session")

      (and (or (str/starts-with? bare "aggtest:")
               (str/starts-with? bare "aggtest-"))
           (contains? TEST-FIXTURE-SIGNATURES predicates))
      "north/test_fixture"

      (and (or (str/starts-with? bare "scratch-sess-")
               (str/starts-with? bare "scratch2-sess-"))
           (contains? TEST-FIXTURE-SIGNATURES predicates))
      "north/test_fixture"

      (and (str/starts-with? (str subject) "@pred:")
           (= LEGACY-SCHEMA-PROJECTION-SIGNATURE predicates))
      "north/legacy_schema_projection"

      :else nil)))

(defn infer-entity-kind [by-lp predicate-names subject predicates other-entries]
  ;; Explicit valid values are open and authoritative, including namespace/name
  ;; extensions. Otherwise infer only when one structural signal is unambiguous.
  (let [explicit-values (values-at by-lp subject "entity_kind")
        explicit (one-valid-entity-kind explicit-values)
        legacy-values (values-at by-lp subject "kind")
        legacy (when (= 1 (count legacy-values))
                 (get LEGACY-KIND->ENTITY-KIND (first legacy-values)))
        bare (subject-bare subject)
        predicate? (and (str/starts-with? (str subject) "@")
                        (contains? predicate-names bare))
        prefixed (cond
                   (str/starts-with? bare "topic-") "topic"
                   (str/starts-with? bare "concern-") "concern"
                   (or (str/starts-with? bare "msg:")
                       (str/starts-with? bare "cmd:")) "message"
                   (str/starts-with? bare "agent:") "agent"
                   (or (str/starts-with? bare "run-")
                       (str/starts-with? bare "run:")) "run"
                   (or (str/starts-with? bare "session:")
                       (str/starts-with? bare "sess-")
                       (str/starts-with? bare "cc-")) "agent"
                   (str/starts-with? bare "denial:") "guard_denial"
                   (str/starts-with? bare "mine:") "north/mine"
                   (str/starts-with? bare "snapshot:") "north/snapshot"
                   (str/starts-with? bare "arena-") "north/arena_run"
                   :else nil)
        shaped (cond
                 (seq (values-at by-lp subject "title")) "thread"
                 (seq (values-at by-lp subject "display_name")) "person"
                 :else nil)
        deterministic (deterministic-legacy-kind by-lp subject predicates)
        reviewed-other (get-in other-entries [subject :entity_kind])]
    (cond
      ;; Predicate identity is the executable schema boundary. A stale or
      ;; extension-valued entity_kind on @p cannot turn the schema entity into a
      ;; domain entity; the migration normalizes it to predicate.
      predicate? {:kind "predicate" :source "schema-entity" :values explicit-values}
      explicit {:kind explicit :source "explicit" :values explicit-values}
      legacy {:kind legacy :source "legacy-kind" :values explicit-values}
      deterministic {:kind deterministic :source "deterministic-legacy-signature" :values explicit-values}
      prefixed {:kind prefixed :source "namespace" :values explicit-values}
      shaped {:kind shaped :source "shape" :values explicit-values}
      reviewed-other {:kind reviewed-other :source "named-reviewed-other-allowlist" :values explicit-values}
      :else {:kind "other" :source "ambiguous" :values explicit-values})))

(defn entity-classifications
  ([facts schema] (entity-classifications facts schema nil))
  ([facts schema manifest]
  (let [by-lp (facts-by-lp facts)
        predicate-names (set (keys schema))
        subject-predicates (reduce (fn [m fact]
                                     (update m (:l fact) (fnil conj #{}) (:p fact)))
                                   {} facts)
        subjects (sort-by str (keys subject-predicates))
        other-entries (manifest-other-entries manifest)]
    (into (sorted-map)
          (map (fn [subject]
                 [subject (infer-entity-kind by-lp predicate-names subject
                                             (get subject-predicates subject #{})
                                             other-entries)]))
          subjects))))

(defn entity-assignment-actions
  ([facts schema] (entity-assignment-actions facts schema nil))
  ([facts schema manifest]
  (let [by-lp (facts-by-lp facts)]
    (->> (entity-classifications facts schema manifest)
         (keep (fn [[subject classification]]
                 (when-not (= "other" (:kind classification))
                   (set-action by-lp subject "entity_kind" (:kind classification)))))
         (sort-by :subject)
         vec))))

(defn dedupe-actions [actions]
  (->> actions
       (reduce (fn [m action]
                 (assoc m [(:subject action) (:predicate action)] action)) {})
       vals
       (sort-by (juxt :subject :predicate))
       vec))

(defn unresolved-predicate-semantics [schema]
  (->> schema
       (keep (fn [[predicate row]]
               (when (seq (:unresolved_fields row))
                 {:predicate predicate :fields (:unresolved_fields row)})))
       vec))

(defn cardinality-conflicts [corpus schema]
  (let [facts-by-predicate (group-by :p (:facts corpus))
        cards (:card_map corpus)]
    (->> schema
         (mapcat (fn [[predicate row]]
                   (when (and (= "single" (:cardinality row))
                              (not (effective-single? cards predicate)))
                     (->> (get facts-by-predicate predicate [])
                          (group-by :l)
                          (keep (fn [[subject facts]]
                                  (let [values (vec (sort (set (map :r facts))))]
                                    (when (> (count values) 1)
                                      {:subject subject :predicate predicate
                                       :values values}))))))))
         (sort-by (juxt :predicate :subject))
         vec)))

(defn reference-shaped? [value]
  (and (string? value)
       (boolean (re-matches #"^@[^@\s][^\s]*$" value))))

(defn reference-shape-defects [facts schema]
  (->> facts
       (keep (fn [fact]
               (when (and (= "ref" (get-in schema [(:p fact) :value_kind]))
                          (not (reference-shaped? (:r fact))))
                 (select-keys fact [:l :p :r]))))
       (sort-by (juxt :p :l :r))
       vec))

(defn dangling-reference-defects [facts schema]
  (let [entities (set (map :l facts))]
    (->> facts
         (keep (fn [fact]
                 (when (and (= "ref" (get-in schema [(:p fact) :value_kind]))
                            (reference-shaped? (:r fact))
                            (not (contains? entities (:r fact))))
                   (select-keys fact [:l :p :r]))))
         (sort-by (juxt :p :l :r))
         vec)))

(defn acyclic-cycle-defects [facts schema]
  (let [index (kernel/build-index facts)
        predicates (->> schema
                        (keep (fn [[predicate row]]
                                (when (= "true" (:acyclic row)) predicate)))
                        set)]
    (->> facts
         (keep (fn [fact]
                 (when (and (contains? predicates (:p fact))
                            (kernel/cycle-i? index (:p fact) (:l fact)))
                   {:l (:l fact) :p (:p fact)})))
         distinct
         (sort-by (juxt :p :l))
         vec)))

(defn repair-identity [repair]
  [(:subject repair) (:predicate repair)])

(defn manifest-cardinality-defects [conflicts manifest]
  (let [repairs (vec (:cardinality_repairs manifest))
        grouped (group-by repair-identity repairs)
        expected (set (map (juxt :subject :predicate) conflicts))
        duplicate-keys (->> grouped (keep (fn [[key rows]] (when (> (count rows) 1) key))) set)
        actual (set (keys grouped))]
    (vec
     (concat
      (map (fn [key] {:type "duplicate-cardinality-repair" :identity key})
           (sort duplicate-keys))
      (map (fn [key] {:type "unexpected-cardinality-repair" :identity key})
           (sort (set/difference actual expected)))
      (mapcat
       (fn [{:keys [subject predicate values]}]
         (let [key [subject predicate]
               repair (first (get grouped key))
               value-set (set values)
               retain-present? (and repair (contains? repair :retain))
               retain (:retain repair)
               retain-set (if (some? retain) #{retain} #{})
               retract-set (set (:retract repair))]
           (cond
             (nil? repair)
             [{:type "missing-cardinality-repair" :identity key :values values}]

             (not retain-present?)
             [{:type "cardinality-repair-missing-retain-decision" :identity key}]

             (not (set/subset? retain-set value-set))
             [{:type "cardinality-repair-retains-unknown-value" :identity key
               :retain retain :values values}]

             (not= retract-set (set/difference value-set retain-set))
             [{:type "cardinality-repair-not-exact" :identity key
               :expected_retract (vec (sort (set/difference value-set retain-set)))
               :actual_retract (vec (sort retract-set))}]

             (not= (count retract-set) (count (:retract repair)))
             [{:type "cardinality-repair-duplicates-retraction" :identity key}]

             :else [])))
       conflicts)))))

(defn manifest-fact-repair-defects [facts manifest]
  (let [live (set (map (juxt :l :p :r) facts))
        repairs (vec (:fact_repairs manifest))
        identities (mapv (juxt :action :subject :predicate :value) repairs)]
    (vec
     (concat
      (when-not (= (count identities) (count (set identities)))
        [{:type "duplicate-fact-repair"}])
      (keep (fn [repair]
              (let [triple [(:subject repair) (:predicate repair) (:value repair)]
                    present? (contains? live triple)]
                (cond
                  (and (= "retract" (:action repair)) (not present?))
                  {:type "fact-repair-retracts-nonlive-triple" :triple triple}

                  (and (= "assert" (:action repair)) present?)
                  {:type "fact-repair-asserts-live-triple" :triple triple}

                  :else nil)))
            repairs)))))

(defn manifest-predicate-conflicts [facts manifest]
  (let [by-lp (facts-by-lp facts)]
    (->> (manifest-predicate-semantics manifest)
         (mapcat
          (fn [[predicate reviewed]]
            (let [subject (str "@" predicate)
                  checks [["cardinality" (:cardinality reviewed) #{"single" "multi"}]
                          ["value_kind" (:value_kind reviewed) #{"literal" "ref"}]
                          ["doc" (:doc reviewed) nil]
                          ["acyclic" (:acyclic reviewed) #{"true" "false"}]]]
              (keep (fn [[field wanted allowed]]
                      (when (some? wanted)
                        (let [actual (values-at by-lp subject field)
                              valid (if allowed (valid-singleton actual allowed)
                                        (when (= 1 (count actual)) (first actual)))]
                          (when (and valid (not= wanted valid))
                            {:type "reviewed-semantics-conflict-with-graph"
                             :predicate predicate :field field
                             :graph valid :reviewed wanted}))))
                    checks))))
         vec)))

(defn plan-for
  ([corpus] (plan-for corpus nil))
  ([corpus manifest]
  (let [facts (:facts corpus)
        schema (desired-schema facts manifest)
        corrupt (corrupt-facts facts)
        colliding (collisions facts (keys schema))
        base-classifications (entity-classifications facts schema)
        base-ambiguous (->> base-classifications
                            (keep (fn [[subject classification]]
                                    (when (= "other" (:kind classification)) subject)))
                            set)
        allowlist-subjects (set (keys (manifest-other-entries manifest)))
        allowlist-defects (->> (set/difference allowlist-subjects base-ambiguous)
                               sort
                               (mapv (fn [subject]
                                       {:type "other-allowlist-subject-is-not-unresolved"
                                        :subject subject})))
        classifications (entity-classifications facts schema manifest)
        ambiguous (->> classifications
                       (keep (fn [[subject classification]]
                               (when (= "other" (:kind classification)) subject)))
                       vec)
        unresolved-semantics (unresolved-predicate-semantics schema)
        conflicts (cardinality-conflicts corpus schema)
        ref-defects (reference-shape-defects facts schema)
        dangling-refs (dangling-reference-defects facts schema)
        cycles (acyclic-cycle-defects facts schema)
        manifest-defects (vec (concat allowlist-defects
                                      (manifest-cardinality-defects conflicts manifest)
                                      (manifest-fact-repair-defects facts manifest)
                                      (manifest-predicate-conflicts facts manifest)))
        actions (dedupe-actions
                 (concat (schema-actions facts schema)
                         (entity-kind-definition-actions facts)
                         (entity-assignment-actions facts schema manifest)))
        receipt {:format PLAN-FORMAT
                 :corpus (:files corpus)
                 :source_seal (source-seal corpus)
                 :fold_version (:version corpus)
                 :predicate_count (count schema)
                 :entity_kinds (vec (keys ENTITY-KINDS))
                 :manifest_sha256 (:_sha256 manifest)
                 :manifest_review (:review manifest)
                 :unresolved_predicate_semantics unresolved-semantics
                 :cardinality_conflicts conflicts
                 :reference_shape_defects ref-defects
                 :dangling_reference_defects dangling-refs
                 :acyclic_cycle_defects cycles
                 :manifest_defects manifest-defects
                 :ambiguous_subjects ambiguous
                 :collisions colliding
                 :corrupt (mapv #(select-keys % [:l :p :r]) corrupt)
                 :actions actions}]
    {:schema schema :corrupt corrupt :collisions colliding :actions actions
     :ambiguous ambiguous :classifications classifications
     :unresolved-semantics unresolved-semantics
     :cardinality-conflicts conflicts :reference-shape-defects ref-defects
     :dangling-reference-defects dangling-refs
     :acyclic-cycle-defects cycles
     :manifest-defects manifest-defects
     :receipt receipt :sha256 (text-sha256 (pr-str receipt))})))

(defn malformed-schema [facts schema]
  (let [by-lp (facts-by-lp facts)]
    (->> schema
         (mapcat (fn [[predicate row]]
                   (let [subject (str "@" predicate)
                         card (values-at by-lp subject "cardinality")
                         kind (values-at by-lp subject "value_kind")
                         acyclic (values-at by-lp subject "acyclic")
                         docs (values-at by-lp subject "doc")
                         ek (values-at by-lp subject "entity_kind")]
                     (remove nil?
                             [(when-not (and (= 1 (count card))
                                             (contains? #{"single" "multi"} (first card)))
                                {:predicate predicate :field "cardinality" :values (vec (sort card))})
                              (when-not (and (= 1 (count kind))
                                             (contains? #{"literal" "ref"} (first kind)))
                                {:predicate predicate :field "value_kind" :values (vec (sort kind))})
                              (when (or (seq acyclic) (some? (:acyclic row)))
                                (when-not (and (= 1 (count acyclic))
                                               (contains? #{"true" "false"} (first acyclic))
                                               (= (:acyclic row) (first acyclic)))
                                  {:predicate predicate :field "acyclic" :values (vec (sort acyclic))}))
                              (when-not (and (= 1 (count docs)) (not (str/blank? (str (first docs)))))
                                {:predicate predicate :field "doc" :values (vec (sort docs))})
                              (when-not (= #{"predicate"} ek)
                                {:predicate predicate :field "entity_kind" :values (vec (sort ek))})]))))
         (sort-by (juxt :predicate :field))
         vec)))

(defn malformed-entity-kind-definitions [facts]
  (let [by-lp (facts-by-lp facts)]
    (->> ENTITY-KINDS
         (keep (fn [[kind doc]]
                 (let [subject (str "@entity-kind:" kind)
                       actual {:entity_kind (values-at by-lp subject "entity_kind")
                               :entity_kind_name (values-at by-lp subject "entity_kind_name")
                               :doc (values-at by-lp subject "doc")}
                       expected {:entity_kind #{ENTITY-KIND-DEFINITION}
                                 :entity_kind_name #{kind}
                                 :doc #{doc}}]
                   (when-not (= expected actual)
                     {:kind kind :actual actual}))))
         vec)))

(defn invalid-entity-kind-values [facts]
  (->> facts
       (filter #(= "entity_kind" (:p %)))
       (group-by :l)
       (keep (fn [[subject subject-facts]]
               (let [values (set (map :r subject-facts))]
                 (when-not (and (= 1 (count values))
                                (string? (first values))
                                (re-matches VALID-ENTITY-KIND (first values)))
                   {:l subject :values (vec (sort-by str values))}))))
       (sort-by (comp str :l))
       vec))

(defn entity-assignment-defects [facts schema]
  (let [by-lp (facts-by-lp facts)]
    (->> (entity-classifications facts schema)
         (keep (fn [[subject classification]]
                 (let [kind (:kind classification)
                       actual (values-at by-lp subject "entity_kind")]
                   (when (and (not= "other" kind) (not= #{kind} actual))
                     {:subject subject :expected kind :actual (vec (sort-by str actual))
                      :source (:source classification)}))))
         vec)))

(defn audit-report [corpus]
  (let [facts (:facts corpus)
        schema (desired-schema facts)
        unresolved-semantics (unresolved-predicate-semantics schema)
        malformed (malformed-schema facts schema)
        kind-defects (malformed-entity-kind-definitions facts)
        assignment-defects (entity-assignment-defects facts schema)
        ambiguous (->> (entity-classifications facts schema)
                       (keep (fn [[subject classification]]
                               (when (= "other" (:kind classification)) subject)))
                       vec)
        corrupt (corrupt-facts facts)
        colliding (collisions facts (keys schema))
        invalid-kinds (invalid-entity-kind-values facts)
        ref-defects (reference-shape-defects facts schema)
        dangling-refs (dangling-reference-defects facts schema)
        cycles (acyclic-cycle-defects facts schema)]
    {:format AUDIT-FORMAT
     :corpus (:files corpus)
     :fold_version (:version corpus)
     :predicate_count (count schema)
     :corrupt (mapv #(select-keys % [:l :p :r]) corrupt)
     :collisions colliding
     :schema_defects malformed
     :unresolved_predicate_semantics unresolved-semantics
     :reference_shape_defects ref-defects
     :dangling_reference_defects dangling-refs
     :acyclic_cycle_defects cycles
     :entity_kind_definition_defects kind-defects
     :entity_assignment_defects assignment-defects
     :ambiguous_subjects ambiguous
     :invalid_entity_kind_values invalid-kinds
     :ok (and (empty? corrupt) (empty? colliding) (empty? malformed)
              (empty? unresolved-semantics) (empty? ref-defects)
              (empty? dangling-refs)
              (empty? cycles)
              (empty? kind-defects) (empty? assignment-defects)
              (empty? invalid-kinds) (empty? ambiguous))}))

(defn cardinality-retraction-actions [manifest]
  (->> (:cardinality_repairs manifest)
       (mapcat (fn [repair]
                 (map (fn [value]
                        {:action "retract" :subject (:subject repair)
                         :predicate (:predicate repair) :value value
                         :policy (:policy repair) :rationale (:rationale repair)
                         :source "reviewed-cardinality-repair"})
                      (:retract repair))))
       (sort-by (juxt :predicate :subject :value))
       vec))

(defn schema-wire-actions [actions]
  (->> actions
       (mapcat (fn [{:keys [subject predicate value before]}]
                 (concat
                  (map (fn [old]
                         {:action "retract" :subject subject :predicate predicate
                          :value old :source "schema-set"})
                       before)
                  [{:action "assert" :subject subject :predicate predicate
                    :value value :source "schema-set"}])))
       vec))

(defn candidate-wire-actions [plan manifest]
  (vec (concat (:fact_repairs manifest)
               (cardinality-retraction-actions manifest)
               (schema-wire-actions (:actions plan)))))

(defn simulate-wire-actions [corpus wire-actions]
  (let [base (long (:version corpus))
        appended (mapv (fn [index action]
                         {:tx (+ base index 1)
                          :op (:action action)
                          :l (:subject action)
                          :p (:predicate action)
                          :r (:value action)
                          :frame "schema-candidate-preflight"})
                       (range) wire-actions)
        ops (into (vec (:ops corpus)) appended)
        folded (fold-for-cutover ops)]
    {:paths (:paths corpus)
     :ops ops
     :facts (:facts folded)
     :version (:version folded)
     :card_map (:card_map folded)
     :files (:files corpus)}))

(defn initial-preflight-defects [plan]
  (vec
   (concat
    (when (seq (:corrupt plan))
      [{:type "corrupt-facts-present" :count (count (:corrupt plan))}])
    (when (seq (:collisions plan))
      [{:type "predicate-thread-collisions" :subjects (:collisions plan)}])
    (map #(assoc % :type "unresolved-predicate-semantics")
         (:unresolved-semantics plan))
    (map (fn [subject] {:type "unresolved-other-subject" :subject subject})
         (:ambiguous plan))
    (:manifest-defects plan))))

(defn candidate-preflight [corpus manifest]
  (let [plan (plan-for corpus manifest)
        initial-defects (initial-preflight-defects plan)
        wire-actions (candidate-wire-actions plan manifest)
        simulated (when (empty? initial-defects)
                    (simulate-wire-actions corpus wire-actions))
        post-plan (when simulated (plan-for simulated))
        post-audit (when simulated (audit-report simulated))
        defects (vec
                 (concat
                  initial-defects
                  (when (and post-plan (seq (:actions post-plan)))
                    [{:type "candidate-does-not-converge"
                      :remaining_action_identities
                      (mapv #(select-keys % [:subject :predicate :value])
                            (:actions post-plan))}])
                  (when (and post-plan (seq (:cardinality-conflicts post-plan)))
                    [{:type "candidate-retains-cardinality-conflicts"
                      :conflicts (:cardinality-conflicts post-plan)}])
                  (when (and post-audit (not (:ok post-audit)))
                    [{:type "candidate-strict-audit-fails"
                      :audit post-audit}])))]
    {:ok (empty? defects)
     :plan plan
     :wire_actions wire-actions
     :simulated_corpus simulated
     :post_plan post-plan
     :post_audit post-audit
     :defects defects}))

(defn apply-wire-action! [port {:keys [action subject predicate value] :as wire-action}]
  (let [result (case action
                 "assert" (north.coord/put! port subject predicate value)
                 "retract" (north.coord/retract! port subject predicate value))]
    (when-not (:ok result)
      (throw (ex-info "offline candidate append action failed"
                      {:type :candidate-action-failed
                       :action (select-keys wire-action [:action :subject :predicate :value])
                       :result result})))
    true))

(defn apply-wire-actions! [port actions]
  (doseq [action actions] (apply-wire-action! port action))
  (count actions))

(defn possible-live-corpus-paths []
  (let [home (System/getProperty "user.home")]
    [(str home "/.local/state/north/coordination.log")
     (str home "/.local/state/north/telemetry.log")
     (str home "/.local/state/north/facts.log")
     (str home "/code/north-data/coordination.log")
     (str home "/code/north-data/telemetry.log")
     (str home "/code/north-data/facts.log")]))

(defn same-existing-file? [a b]
  (let [left (io/file a) right (io/file b)]
    (and (.exists left) (.exists right)
         (try (java.nio.file.Files/isSameFile (.toPath left) (.toPath right))
              (catch Exception _ false)))))

(defn live-corpus-aliases [paths]
  (->> paths
       (mapcat (fn [path]
                 (keep (fn [live]
                         (when (same-existing-file? path live)
                           {:candidate path :live (.getCanonicalPath (io/file live))}))
                       (possible-live-corpus-paths))))
       vec))

(defn assert-offline-candidate! [paths]
  (let [aliases (live-corpus-aliases paths)]
    (when (seq aliases)
      (throw (ex-info "candidate builder refuses canonical/live North corpus aliases before the first write"
                      {:type :live-corpus-candidate-refused :aliases aliases}))))
  (when (nonblank-string? (System/getenv "FRAM_SINGLE_VALUED"))
    (throw (ex-info "candidate builder requires FRAM_SINGLE_VALUED unset so graph declarations are the only post-cutover authority"
                    {:type :candidate-fallback-environment-refused})))
  true)

(defn action-identities [actions]
  (mapv #(select-keys % [:subject :predicate :value]) actions))

(defn write-receipt! [dir receipt]
  (let [sha (text-sha256 (pr-str receipt))
        directory (io/file dir)
        target (io/file directory (str "schema-" sha ".edn"))
        tmp (io/file directory (str ".schema-" sha ".tmp"))]
    (.mkdirs directory)
    (spit tmp (str (pr-str receipt) "\n"))
    (java.nio.file.Files/move (.toPath tmp) (.toPath target)
                              (into-array java.nio.file.CopyOption
                                          [java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
    (.getCanonicalPath target)))

(defn option-value! [option remaining]
  (let [value (first remaining)]
    (when (or (nil? value) (str/starts-with? value "--"))
      (throw (ex-info (str option " requires a value")
                      {:type :missing-option-value :option option})))
    value))

(defn parse-opts [args]
  (loop [remaining args opts {:execute false :strict false :repair-corrupt false
                              :offline-confirm false :verbose false}]
    (if (empty? remaining)
      opts
      (let [[arg & more] remaining]
        (case arg
          "--execute" (recur more (assoc opts :execute true))
          "--strict" (recur more (assoc opts :strict true))
          "--verbose" (recur more (assoc opts :verbose true))
          "--repair-corrupt" (recur more (assoc opts :repair-corrupt true))
          "--offline-confirm" (recur more (assoc opts :offline-confirm true))
          "--log" (recur (rest more) (assoc opts :log (option-value! arg more)))
          "--telemetry" (recur (rest more) (assoc opts :telemetry (option-value! arg more)))
          "--manifest" (recur (rest more) (assoc opts :manifest (option-value! arg more)))
          "--receipt-dir" (recur (rest more) (assoc opts :receipt-dir (option-value! arg more)))
          ;; Compatibility with the abandoned lane's positional log argument.
          (if (:log opts)
            (throw (ex-info (str "unknown argument: " arg) {}))
            (recur more (assoc opts :log arg))))))))

(defn default-receipt-dir []
  (str (System/getProperty "user.home") "/.local/state/north/schema-receipts"))

(defn manifest-template [corpus plan]
  {:format REPAIR-MANIFEST-FORMAT
   :source (source-seal corpus)
   :review {:by "REVIEWER_REQUIRED"
            :at "REVIEW_INSTANT_REQUIRED"
            :basis "REVIEW_BASIS_REQUIRED"}
   :predicate_semantics
   (into (sorted-map)
         (map (fn [{:keys [predicate]}]
                [predicate {:cardinality nil :value_kind nil :doc nil
                            :rationale "SEMANTIC_REVIEW_REQUIRED"}]))
         (:unresolved-semantics plan))
   :cardinality_repairs
   (mapv (fn [{:keys [subject predicate values]}]
           {:subject subject :predicate predicate :retain nil :retract values
            :policy "SEMANTIC_POLICY_REQUIRED"
            :rationale "REVIEW_REQUIRED; do not accept the template's retain/retract placeholders"})
         (:cardinality-conflicts plan))
   :fact_repairs []
   :other_allowlist
   {:name "NAMED_REVIEWED_ALLOWLIST_REQUIRED"
    :entries (into (sorted-map)
                   (map (fn [subject]
                          [subject {:entity_kind nil
                                    :rationale "QUARANTINE_OR_EXTENSION_REVIEW_REQUIRED"}]))
                   (:ambiguous plan))}})

(defn print-omitted [label total shown]
  (when (> total shown)
    (println (str "  … " (- total shown) " more " label " omitted"))))

(defn print-plan [plan verbose]
  (println (format "schema plan — %d predicate(s), %d action(s), %d corrupt fact(s), %d collision(s), %d unresolved predicate(s), %d cardinality conflict group(s), %d ref-shape defect(s), %d dangling ref(s), %d cycle node(s), %d ambiguous other"
                   (count (:schema plan)) (count (:actions plan))
                   (count (:corrupt plan)) (count (:collisions plan))
                   (count (:unresolved-semantics plan))
                   (count (:cardinality-conflicts plan))
                   (count (:reference-shape-defects plan))
                   (count (:dangling-reference-defects plan))
                   (count (:acyclic-cycle-defects plan))
                   (count (:ambiguous plan))))
  (println (str "  plan_sha256 " (:sha256 plan)))
  (doseq [collision (:collisions plan)]
    (println (str "  ✗ predicate/thread subject collision: @" collision)))
  (doseq [fact (:corrupt plan)]
    (println (str "  ✗ non-registrable predicate: " (pr-str (:p fact))
                  " on " (:l fact) " -> " (pr-str (:r fact)))))
  (doseq [unresolved (take 50 (:unresolved-semantics plan))]
    (println (str "  ✗ predicate semantics unresolved: " (:predicate unresolved)
                  " needs " (str/join "," (:fields unresolved)))))
  (print-omitted "unresolved predicate(s)"
                 (count (:unresolved-semantics plan)) 50)
  (doseq [conflict (take 50 (:cardinality-conflicts plan))]
    (println (str "  ✗ reviewed cardinality repair required: "
                  (:subject conflict) " " (:predicate conflict) " "
                  (pr-str {:value_count (count (:values conflict))
                           :first_values (vec (take 5 (:values conflict)))}))))
  (print-omitted "cardinality conflict group(s)"
                 (count (:cardinality-conflicts plan)) 50)
  (doseq [defect (take 50 (:reference-shape-defects plan))]
    (println (str "  ✗ ref predicate carries non-reference value: "
                  (:l defect) " " (:p defect) " " (pr-str (:r defect)))))
  (doseq [defect (take 50 (:dangling-reference-defects plan))]
    (println (str "  ✗ ref predicate references missing entity: "
                  (:l defect) " " (:p defect) " " (pr-str (:r defect)))))
  (doseq [defect (take 50 (:acyclic-cycle-defects plan))]
    (println (str "  ✗ acyclic predicate participates in a cycle: "
                  (:l defect) " " (:p defect))))
  (let [manifest-defects (remove #(= "missing-cardinality-repair" (:type %))
                                 (:manifest-defects plan))]
    (doseq [defect (take 50 manifest-defects)]
      (println (str "  ✗ repair manifest: " (pr-str defect))))
    (print-omitted "repair-manifest defect(s)" (count manifest-defects) 50))
  (when (seq (:ambiguous plan))
    (println (str "  ✗ unresolved subjects remain `other`; name a reviewed quarantine/extension allowlist; first 20: "
                  (str/join ", " (take 20 (:ambiguous plan))))))
  (when verbose
    (doseq [{:keys [subject predicate value before]} (:actions plan)]
      (println (format "  set %-38s %-22s %-10s  was %s"
                       subject predicate value (pr-str before))))))

(defn print-audit [report]
  (println (format "schema audit — %d predicate(s), %d schema defect(s), %d unresolved predicate(s), %d corrupt fact(s), %d ref-shape defect(s), %d dangling ref(s), %d cycle node(s), %d entity-kind defect(s), %d unresolved other"
                   (:predicate_count report) (count (:schema_defects report))
                   (count (:unresolved_predicate_semantics report))
                   (count (:corrupt report))
                   (count (:reference_shape_defects report))
                   (count (:dangling_reference_defects report))
                   (count (:acyclic_cycle_defects report))
                   (+ (count (:entity_kind_definition_defects report))
                      (count (:entity_assignment_defects report))
                      (count (:invalid_entity_kind_values report)))
                   (count (:ambiguous_subjects report))))
  (doseq [fact (:corrupt report)]
    (println (str "  ✗ corrupt predicate " (pr-str (:p fact)) " on " (:l fact))))
  (doseq [collision (:collisions report)]
    (println (str "  ✗ predicate/thread collision @" collision)))
  (doseq [defect (:schema_defects report)]
    (println (str "  ✗ @" (:predicate defect) " " (:field defect)
                  " = " (pr-str (:values defect)))))
  (doseq [defect (:unresolved_predicate_semantics report)]
    (println (str "  ✗ predicate semantics unresolved: " (:predicate defect)
                  " needs " (str/join "," (:fields defect)))))
  (doseq [defect (take 50 (:reference_shape_defects report))]
    (println (str "  ✗ ref predicate carries non-reference value: "
                  (:l defect) " " (:p defect) " " (pr-str (:r defect)))))
  (doseq [defect (take 50 (:dangling_reference_defects report))]
    (println (str "  ✗ ref predicate references missing entity: "
                  (:l defect) " " (:p defect) " " (pr-str (:r defect)))))
  (doseq [defect (take 50 (:acyclic_cycle_defects report))]
    (println (str "  ✗ acyclic predicate participates in a cycle: "
                  (:l defect) " " (:p defect))))
  (doseq [defect (:entity_kind_definition_defects report)]
    (println (str "  ✗ entity kind definition " (:kind defect) " is incomplete")))
  (doseq [defect (take 50 (:entity_assignment_defects report))]
    (println (str "  ✗ " (:subject defect) " entity_kind=" (pr-str (:actual defect))
                  ", expected " (:expected defect) " from " (:source defect))))
  (doseq [defect (:invalid_entity_kind_values report)]
    (println (str "  ✗ invalid/ambiguous entity_kind " (pr-str (:values defect)) " on " (:l defect))))
  (when (seq (:ambiguous_subjects report))
    (println (str "  ✗ unresolved subjects remain `other`; reviewed quarantine/extension classification required; first 20: "
                  (str/join ", " (take 20 (:ambiguous_subjects report))))))
  (println (if (:ok report)
             "  ✓ executable predicate entities are authoritative; entity kinds are governed; unresolved other is empty"
             "  -> if corruption is listed, use `repair-corrupt` for exact diagnostics; mutation requires the corpus transaction surface")))

(defn usage! []
  (binding [*out* *err*]
    (println "usage: schema-migrate.clj <port> {plan|manifest-template|migrate|build-candidate|audit|repair-corrupt} [--execute] [--strict] [--verbose] [--manifest PATH] [--log PATH] [--telemetry PATH] [--receipt-dir PATH]")
    (println "       coordination and telemetry logs are mandatory; canonical mutation routes through an offline candidate plus corpus-transaction"))
  (System/exit 2))

(defn print-preflight [preflight]
  (if (:ok preflight)
    (println (str "  ✓ complete preflight simulated "
                  (count (:wire_actions preflight))
                  " append action(s); post-plan 0 and strict audit green"))
    (doseq [defect (:defects preflight)]
      (println (str "  ✗ candidate preflight: " (pr-str defect))))))

(defn require-offline-daemon! [port log version]
  (let [status (north.coord/strict-coordinator-status port log)]
    (when-not (and (:ready status) (= version (:version status)))
      (throw (ex-info "offline candidate coordinator is not strict-ready on the exact sealed corpus version"
                      {:type :offline-candidate-daemon-mismatch
                       :expected_version version :status status})))
    status))

(defn execute-offline-candidate! [port paths corpus manifest opts]
  (when-not (:offline-confirm opts)
    (throw (ex-info "build-candidate --execute requires --offline-confirm"
                    {:type :offline-confirm-required})))
  (assert-offline-candidate! paths)
  (require-append-boundaries! paths)
  (let [preflight (candidate-preflight corpus manifest)]
    (print-plan (:plan preflight) (:verbose opts))
    (print-preflight preflight)
    (when-not (:ok preflight)
      (throw (ex-info "complete candidate preflight failed; zero coordinator writes attempted"
                      {:type :candidate-preflight-failed
                       :defects (:defects preflight)})))
    (let [daemon-status (require-offline-daemon! port (first paths) (:version corpus))
          acknowledged (apply-wire-actions! port (:wire_actions preflight))
          post (read-corpus paths)
          post-plan (plan-for post)
          report (audit-report post)
          expected-post (get preflight :simulated_corpus)
          expected-version (+ (long (:version corpus))
                              (count (:wire_actions preflight)))
          exact-simulation? (and (= expected-version (:version post))
                                 (= (set (:facts expected-post))
                                    (set (:facts post))))
          converged? (and exact-simulation?
                          (empty? (:actions post-plan))
                          (empty? (:cardinality-conflicts post-plan))
                          (:ok report))
          receipt {:format "north-schema-candidate-build/v1"
                   :source (source-seal corpus)
                   :manifest_sha256 (:_sha256 manifest)
                   :plan_sha256 (get-in preflight [:plan :sha256])
                   :simulated_post_plan_sha256 (get-in preflight [:post_plan :sha256])
                   :daemon daemon-status
                   :requested_action_identities
                   (mapv #(select-keys % [:action :subject :predicate :value])
                         (:wire_actions preflight))
                   :actions_acknowledged acknowledged
                   :expected_post_version expected-version
                   :actual_post_version (:version post)
                   :post_matches_simulation exact-simulation?
                   :post_plan_sha256 (:sha256 post-plan)
                   :remaining_action_identities (action-identities (:actions post-plan))
                   :converged converged?
                   :post_audit report
                   :candidate_corpus (:files post)}
          receipt-path (write-receipt! (or (:receipt-dir opts) (default-receipt-dir)) receipt)]
      (print-audit report)
      (println (str "  receipt " receipt-path))
      (when-not converged?
        (throw (ex-info "offline candidate diverged from its complete simulation"
                        {:type :candidate-postcondition-failed
                         :post_matches_simulation exact-simulation?
                         :expected_version expected-version
                         :actual_version (:version post)
                         :remaining (count (:actions post-plan))
                         :audit_ok (:ok report)})))
      receipt)))

(defn main! [args]
  (let [[port-arg verb & raw-args] args]
    (when-not (and port-arg verb) (usage!))
    (let [port (Integer/parseInt port-arg)
          opts (parse-opts raw-args)
          log (or (:log opts) (System/getenv "FRAM_LOG") (north.coord/expected-log))
          telemetry (or (:telemetry opts) (System/getenv "FRAM_TELEMETRY_LOG"))
          paths (resolve-corpus-paths! log telemetry)
          corpus (read-corpus paths)
          manifest (when (:manifest opts)
                     (read-repair-manifest! (:manifest opts) corpus))]
      (case verb
        "plan"
        (print-plan (plan-for corpus manifest) (:verbose opts))

        "manifest-template"
        (let [plan (plan-for corpus)]
          (println (pr-str (manifest-template corpus plan))))

        "audit"
        (let [report (audit-report corpus)]
          (print-audit report)
          (when (and (:strict opts) (not (:ok report))) (System/exit 1)))

        "migrate"
        (let [plan (plan-for corpus manifest)]
          (print-plan plan (:verbose opts))
          (if (:execute opts)
            (throw (ex-info "direct migrate --execute is disabled before the first write; build a reviewed offline candidate, then install it with north corpus-transaction"
                            {:type :direct-schema-migration-disabled
                             :route "build-candidate + corpus-transaction"}))
            (println "  plan-only compatibility verb; use manifest-template, then build-candidate on disposable copies")))

        "build-candidate"
        (if (:execute opts)
          (do
            (when-not manifest
              (throw (ex-info "build-candidate --execute requires --manifest PATH"
                              {:type :missing-repair-manifest})))
            (execute-offline-candidate! port paths corpus manifest opts))
          (let [plan (plan-for corpus manifest)
                preflight (when manifest (candidate-preflight corpus manifest))]
            (print-plan plan (:verbose opts))
            (if preflight
              (print-preflight preflight)
              (println "  dry-run only; supply a reviewed manifest to evaluate complete preflight"))))

        "repair-corrupt"
        (let [bad (corrupt-facts (:facts corpus))]
          (println (str "repair-corrupt — " (count bad) " live non-registrable predicate fact(s)"))
          (doseq [fact bad]
            (println (str "  would retract exact triple "
                          (pr-str (select-keys fact [:l :p :r])))))
          (if (:execute opts)
            (throw (ex-info "repair-corrupt execute unavailable: corpus transaction required; no bytes written"
                            {:type :corpus-transaction-required
                             :count (count bad)}))
            (println "  diagnostic dry-run only; no bytes written")))

        (usage!)))))

(defn invoked-as-script? []
  (when-let [main-source (System/getProperty "babashka.file")]
    (= SCHEMA-MIGRATE-SOURCE (.getCanonicalPath (io/file main-source)))))

(when (invoked-as-script?)
  (main! *command-line-args*))
