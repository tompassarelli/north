#!/usr/bin/env bb
;; Canonical executable schema migration for North.
;;
;; A predicate has exactly one authority: the Fram predicate entity @<name>.
;; cardinality, value_kind, acyclic, doc, entity_kind, and any extension metadata
;; are ordinary facts on that entity.  The old @pred:<name> catalog is never read
;; here; pred-cli now renders the executable entities directly.
;;
;; The migration is deliberately additive for valid declarations.  Existing
;; executable metadata wins.  BOOTSTRAP VOCAB and the legacy engine/env singleton
;; sets are used only to fill an absent declaration during the one-time cutover.
;; Once every live predicate has explicit facts, engine fallback is inert.
;;
;; Run through bin/north (it supplies Fram's classpath and corpus paths):
;;   north schema-migrate plan
;;   north schema-migrate migrate --execute
;;   north schema-migrate audit --strict
;;   north schema-migrate repair-corrupt --execute --offline-confirm  # coordinator stopped
(require '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str]
         '[fram.fold :as fold]
         '[fram.rt :as rt])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def VALID-PREDICATE #"^[A-Za-z][A-Za-z0-9_-]*(?:/[A-Za-z][A-Za-z0-9_-]*)?$")
(def VALID-ENTITY-KIND #"^[a-z][a-z0-9_-]*(?:/[a-z][a-z0-9_-]*)?$")
(def META-PREDICATES #{"cardinality" "value_kind" "acyclic"})
(def ACYCLIC-PREDICATES #{"depends_on" "part_of"})

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
   "topic" "topic"})

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

(defn script-dir [] (.getParent (io/file (System/getProperty "babashka.file"))))
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
     BUILTIN-SCHEMA)))

(defn split-ws [s]
  (set (remove str/blank? (str/split (or s "") #"\s+"))))

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

(defn existing-paths [paths]
  (vec (filter #(.isFile (io/file %)) (remove str/blank? paths))))

(defn read-corpus [paths]
  (let [paths (existing-paths paths)
        ops (vec (mapcat rt/read-log paths))
        folded (fold/fold ops)]
    {:paths paths
     :ops ops
     :facts (:facts folded)
     :version (:version folded)
     :files (mapv (fn [path]
                    {:path (.getCanonicalPath (io/file path))
                     :bytes (.length (io/file path))
                     :sha256 (file-sha256 path)})
                  paths)}))

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

(defn preferred-doc [current bootstrap predicate]
  (or (when (= 1 (count current))
        (let [v (first current)] (when-not (str/blank? (str v)) (str v))))
      (let [v (get-in bootstrap [predicate :doc])]
        (when-not (str/blank? (str v)) (str v)))
      "Observed live predicate; semantics are not yet curated."))

(defn desired-schema [facts]
  (let [by-lp (facts-by-lp facts)
        observed (live-predicate-values facts)
        bootstrap (bootstrap-schema)
        predicates (sort (set/union (set (keys observed))
                                    (set (keys bootstrap))
                                    META-PREDICATES
                                    (set (keys BUILTIN-SCHEMA))))
        legacy-single (set/union LEGACY-KERNEL-SINGLE
                                 (split-ws (System/getenv "FRAM_SINGLE_VALUED")))]
    (into (sorted-map)
          (map (fn [predicate]
                 (let [entity (str "@" predicate)
                       card-values (values-at by-lp entity "cardinality")
                       kind-values (values-at by-lp entity "value_kind")
                       doc-values (values-at by-lp entity "doc")
                       existing-card (valid-singleton card-values #{"single" "multi"})
                       existing-kind (valid-singleton kind-values #{"literal" "ref"})
                       bootstrap-row (get bootstrap predicate)
                       objects (get observed predicate #{})
                       derived-kind (if (and (seq objects)
                                             (every? #(and (string? %) (str/starts-with? % "@")) objects))
                                      "ref" "literal")]
                   [predicate
                    {:cardinality (or existing-card
                                      (:card bootstrap-row)
                                      (when (contains? legacy-single predicate) "single")
                                      "multi")
                     :value_kind (or existing-kind (:kind bootstrap-row) derived-kind)
                     :doc (preferred-doc doc-values bootstrap predicate)
                     :acyclic (or (= "true" (valid-singleton
                                             (values-at by-lp entity "acyclic")
                                             #{"true" "false"}))
                                  (contains? ACYCLIC-PREDICATES predicate))}]))
               predicates))))

(defn set-action [by-lp subject predicate value]
  (let [current (values-at by-lp subject predicate)]
    (when (not= current #{value})
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
                              (when (:acyclic row)
                                (set-action by-lp subject "acyclic" "true"))]))))
         (sort-by (juxt :subject :predicate))
         vec)))

(defn entity-kind-definition-actions [facts]
  (let [by-lp (facts-by-lp facts)]
    (->> CORE-ENTITY-KINDS
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

(defn infer-entity-kind [by-lp predicate-names subject]
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
                 :else nil)]
    (cond
      ;; Predicate identity is the executable schema boundary. A stale or
      ;; extension-valued entity_kind on @p cannot turn the schema entity into a
      ;; domain entity; the migration normalizes it to predicate.
      predicate? {:kind "predicate" :source "schema-entity" :values explicit-values}
      explicit {:kind explicit :source "explicit" :values explicit-values}
      legacy {:kind legacy :source "legacy-kind" :values explicit-values}
      prefixed {:kind prefixed :source "namespace" :values explicit-values}
      shaped {:kind shaped :source "shape" :values explicit-values}
      :else {:kind "other" :source "ambiguous" :values explicit-values})))

(defn entity-classifications [facts schema]
  (let [by-lp (facts-by-lp facts)
        predicate-names (set (keys schema))
        subjects (sort-by str (set (map :l facts)))]
    (into (sorted-map)
          (map (fn [subject]
                 [subject (infer-entity-kind by-lp predicate-names subject)]))
          subjects)))

(defn entity-assignment-actions [facts schema]
  (let [by-lp (facts-by-lp facts)]
    (->> (entity-classifications facts schema)
         (keep (fn [[subject classification]]
                 (when-not (= "other" (:kind classification))
                   (set-action by-lp subject "entity_kind" (:kind classification)))))
         (sort-by :subject)
         vec)))

(defn dedupe-actions [actions]
  (->> actions
       (reduce (fn [m action]
                 (assoc m [(:subject action) (:predicate action)] action)) {})
       vals
       (sort-by (juxt :subject :predicate))
       vec))

(defn plan-for [corpus]
  (let [facts (:facts corpus)
        schema (desired-schema facts)
        corrupt (corrupt-facts facts)
        colliding (collisions facts (keys schema))
        classifications (entity-classifications facts schema)
        ambiguous (->> classifications
                       (keep (fn [[subject classification]]
                               (when (= "other" (:kind classification)) subject)))
                       vec)
        actions (dedupe-actions
                 (concat (schema-actions facts schema)
                         (entity-kind-definition-actions facts)
                         (entity-assignment-actions facts schema)))
        receipt {:format "north-schema-plan/v1"
                 :corpus (:files corpus)
                 :fold_version (:version corpus)
                 :predicate_count (count schema)
                 :core_entity_kinds (vec (keys CORE-ENTITY-KINDS))
                 :ambiguous_subjects ambiguous
                 :collisions colliding
                 :corrupt (mapv #(select-keys % [:l :p :r]) corrupt)
                 :actions actions}]
    {:schema schema :corrupt corrupt :collisions colliding :actions actions
     :ambiguous ambiguous :classifications classifications
     :receipt receipt :sha256 (text-sha256 (pr-str receipt))}))

(defn malformed-schema [facts schema]
  (let [by-lp (facts-by-lp facts)]
    (->> schema
         (mapcat (fn [[predicate _]]
                   (let [subject (str "@" predicate)
                         card (values-at by-lp subject "cardinality")
                         kind (values-at by-lp subject "value_kind")
                         docs (values-at by-lp subject "doc")
                         ek (values-at by-lp subject "entity_kind")]
                     (remove nil?
                             [(when-not (and (= 1 (count card))
                                             (contains? #{"single" "multi"} (first card)))
                                {:predicate predicate :field "cardinality" :values (vec (sort card))})
                              (when-not (and (= 1 (count kind))
                                             (contains? #{"literal" "ref"} (first kind)))
                                {:predicate predicate :field "value_kind" :values (vec (sort kind))})
                              (when-not (and (= 1 (count docs)) (not (str/blank? (str (first docs)))))
                                {:predicate predicate :field "doc" :values (vec (sort docs))})
                              (when-not (= #{"predicate"} ek)
                                {:predicate predicate :field "entity_kind" :values (vec (sort ek))})]))))
         (sort-by (juxt :predicate :field))
         vec)))

(defn malformed-entity-kind-definitions [facts]
  (let [by-lp (facts-by-lp facts)]
    (->> CORE-ENTITY-KINDS
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
        malformed (malformed-schema facts schema)
        kind-defects (malformed-entity-kind-definitions facts)
        assignment-defects (entity-assignment-defects facts schema)
        ambiguous (->> (entity-classifications facts schema)
                       (keep (fn [[subject classification]]
                               (when (= "other" (:kind classification)) subject)))
                       vec)
        corrupt (corrupt-facts facts)
        colliding (collisions facts (keys schema))
        invalid-kinds (invalid-entity-kind-values facts)]
    {:format "north-schema-audit/v1"
     :corpus (:files corpus)
     :fold_version (:version corpus)
     :predicate_count (count schema)
     :corrupt (mapv #(select-keys % [:l :p :r]) corrupt)
     :collisions colliding
     :schema_defects malformed
     :entity_kind_definition_defects kind-defects
     :entity_assignment_defects assignment-defects
     :ambiguous_subjects ambiguous
     :invalid_entity_kind_values invalid-kinds
     :ok (and (empty? corrupt) (empty? colliding) (empty? malformed)
              (empty? kind-defects) (empty? assignment-defects)
              (empty? invalid-kinds))}))

(defn set-values! [port subject predicate value]
  ;; Explicit supersession makes this correct even before @doc/@entity_kind have
  ;; acquired their own cardinality declarations in the same migration.
  (doseq [old (north.coord/many port subject predicate)]
    (let [result (north.coord/retract! port subject predicate old)]
      (when-not (:ok result)
        (throw (ex-info "schema metadata retract failed"
                        {:subject subject :predicate predicate :value old :result result})))))
  (let [result (north.coord/put! port subject predicate value)]
    (when-not (:ok result)
      (throw (ex-info "schema metadata write failed"
                      {:subject subject :predicate predicate :value value :result result}))))
  true)

(defn apply-actions! [port actions]
  (doseq [{:keys [subject predicate value]} actions]
    (set-values! port subject predicate value))
  (count actions))

(defn coordinator-online? [port]
  (try
    (number? (:version (north.coord/send-op port {:op :version})))
    (catch Exception _ false)))

(defn repair-log-offline! [path]
  ;; The coordinator intentionally omits malformed triples from its warm store,
  ;; so an online retract is acknowledged as a no-op and cannot heal the bytes.
  ;; Repair is therefore a narrow append-only maintenance operation: daemon
  ;; proved down, OS file lock held, fresh fold inspected under that lock, exact
  ;; retract tombstones appended with strictly increasing tx, fsync before return.
  (with-open [raf (java.io.RandomAccessFile. path "rw")
              channel (.getChannel raf)]
    ;; Keep the lock strongly reachable until channel close; closing the channel
    ;; releases it (Babashka intentionally does not expose FileLock.close).
    (let [_lock (.lock channel)
          ops (rt/read-log path)
          bad (corrupt-facts (:facts (fold/fold ops)))
          first-tx (inc (fold/max-tx ops))]
      (when (seq bad)
        (let [length (.length raf)]
          (when (pos? length)
            (.seek raf (dec length))
            (when (not= 10 (.read raf))
              (.seek raf length)
              (.write raf (.getBytes "\n" java.nio.charset.StandardCharsets/UTF_8))))
          (.seek raf (.length raf)))
        (doseq [[offset fact] (map-indexed vector bad)]
          (let [line (str (pr-str {:tx (+ first-tx offset)
                                   :op "retract"
                                   :l (:l fact) :p (:p fact) :r (:r fact)
                                   :frame "north-schema-corrupt-repair/v1"}) "\n")]
            (.write raf (.getBytes line java.nio.charset.StandardCharsets/UTF_8))))
        (.force channel true))
      bad)))

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
          "--log" (recur (rest more) (assoc opts :log (first more)))
          "--telemetry" (recur (rest more) (assoc opts :telemetry (first more)))
          "--receipt-dir" (recur (rest more) (assoc opts :receipt-dir (first more)))
          ;; Compatibility with the abandoned lane's positional log argument.
          (if (:log opts)
            (throw (ex-info (str "unknown argument: " arg) {}))
            (recur more (assoc opts :log arg))))))))

(defn default-receipt-dir []
  (str (System/getProperty "user.home") "/.local/state/north/schema-receipts"))

(defn print-plan [plan verbose]
  (println (format "schema plan — %d predicate(s), %d action(s), %d corrupt fact(s), %d collision(s), %d ambiguous other"
                   (count (:schema plan)) (count (:actions plan))
                   (count (:corrupt plan)) (count (:collisions plan))
                   (count (:ambiguous plan))))
  (println (str "  plan_sha256 " (:sha256 plan)))
  (doseq [collision (:collisions plan)]
    (println (str "  ✗ predicate/thread subject collision: @" collision)))
  (doseq [fact (:corrupt plan)]
    (println (str "  ✗ non-registrable predicate: " (pr-str (:p fact))
                  " on " (:l fact) " -> " (pr-str (:r fact)))))
  (when (seq (:ambiguous plan))
    (println (str "  ! ambiguous subjects remain `other` (never guessed); first 20: "
                  (str/join ", " (take 20 (:ambiguous plan))))))
  (when verbose
    (doseq [{:keys [subject predicate value before]} (:actions plan)]
      (println (format "  set %-38s %-22s %-10s  was %s"
                       subject predicate value (pr-str before))))))

(defn print-audit [report]
  (println (format "schema audit — %d predicate(s), %d schema defect(s), %d corrupt fact(s), %d entity-kind defect(s), %d ambiguous other"
                   (:predicate_count report) (count (:schema_defects report))
                   (count (:corrupt report))
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
  (doseq [defect (:entity_kind_definition_defects report)]
    (println (str "  ✗ entity kind definition " (:kind defect) " is incomplete")))
  (doseq [defect (take 50 (:entity_assignment_defects report))]
    (println (str "  ✗ " (:subject defect) " entity_kind=" (pr-str (:actual defect))
                  ", expected " (:expected defect) " from " (:source defect))))
  (doseq [defect (:invalid_entity_kind_values report)]
    (println (str "  ✗ invalid/ambiguous entity_kind " (pr-str (:values defect)) " on " (:l defect))))
  (when (seq (:ambiguous_subjects report))
    (println (str "  ! ambiguous subjects remain `other` (never guessed); first 20: "
                  (str/join ", " (take 20 (:ambiguous_subjects report))))))
  (println (if (:ok report)
             "  ✓ executable predicate entities are authoritative; core entity kinds are governed"
             "  -> if corruption is listed, run offline `repair-corrupt`; then run `migrate --execute` and audit again")))

(defn usage! []
  (binding [*out* *err*]
    (println "usage: schema-migrate.clj <port> {plan|migrate|audit|repair-corrupt} [--execute] [--strict] [--verbose] [--log PATH] [--telemetry PATH] [--receipt-dir PATH]")
    (println "       repair-corrupt additionally requires --offline-confirm and a stopped coordinator"))
  (System/exit 2))

(let [[port-arg verb & raw-args] *command-line-args*]
  (when-not (and port-arg verb) (usage!))
  (let [port (Integer/parseInt port-arg)
        opts (parse-opts raw-args)
        log (or (:log opts) (System/getenv "FRAM_LOG") (north.coord/expected-log))
        telemetry (or (:telemetry opts) (System/getenv "FRAM_TELEMETRY_LOG"))
        paths (existing-paths [log telemetry])
        _ (when (empty? paths)
            (throw (ex-info "no readable North corpus log" {:log log :telemetry telemetry})))
        corpus (read-corpus paths)]
    (case verb
      "plan"
      (print-plan (plan-for corpus) (:verbose opts))

      "audit"
      (let [report (audit-report corpus)]
        (print-audit report)
        (when (and (:strict opts) (not (:ok report))) (System/exit 1)))

      "migrate"
      (let [plan (plan-for corpus)]
        (print-plan plan (:verbose opts))
        (when-not (:execute opts)
          (println "  dry-run only; pass --execute to commit through the coordinator"))
        (when (and (:execute opts) (seq (:collisions plan)))
          (throw (ex-info "predicate/thread collisions make migration unsafe" {:collisions (:collisions plan)})))
        (when (and (:execute opts) (seq (:corrupt plan)))
          (throw (ex-info "corrupt predicates present; stop the coordinator and run repair-corrupt --execute --offline-confirm first"
                          {:count (count (:corrupt plan))})))
        (when (:execute opts)
          (apply-actions! port (:actions plan))
          (let [post (read-corpus paths)
                report (audit-report post)
                receipt {:format "north-schema-migration/v1"
                         :plan_sha256 (:sha256 plan)
                         :actions_committed (count (:actions plan))
                         :corrupt_retracted 0
                         :post_audit report}
                receipt-path (write-receipt! (or (:receipt-dir opts) (default-receipt-dir)) receipt)]
            (print-audit report)
            (println (str "  receipt " receipt-path))
            (when-not (:ok report) (System/exit 1)))))

      ("repair-corrupt" "reject-empty")
      (let [bad (corrupt-facts (:facts corpus))]
        (println (str "repair-corrupt — " (count bad) " live non-registrable predicate fact(s)"))
        (if-not (:execute opts)
          (println "  dry-run only; stop the coordinator, then pass --execute --offline-confirm")
          (do
            (when-not (:offline-confirm opts)
              (throw (ex-info "offline corruption repair requires --offline-confirm" {})))
            (when (coordinator-online? port)
              (throw (ex-info "offline corruption repair refused while coordinator is reachable" {:port port})))
            (let [repaired (vec (mapcat repair-log-offline! paths))
                  post (read-corpus paths)
                  receipt {:format "north-schema-corrupt-repair/v1"
                           :pre_corpus (:files corpus)
                           :retracted (mapv #(select-keys % [:l :p :r]) repaired)
                           :post_corpus (:files post)}
                  receipt-path (write-receipt! (or (:receipt-dir opts) (default-receipt-dir)) receipt)]
              (println (str "  appended " (count repaired) " exact retract tombstone(s) under file lock + fsync"))
              (println (str "  receipt " receipt-path))
              (when (seq (corrupt-facts (:facts post)))
                (throw (ex-info "corruption remains after offline repair" {})))))))

      (usage!))))
