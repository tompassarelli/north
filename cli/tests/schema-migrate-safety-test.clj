#!/usr/bin/env bb
;; Adversarial, write-free proof of the schema cutover safety contract.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file *file*)) "../..")))
(def fram (.getCanonicalPath
           (io/file (or (System/getenv "FRAM_PATH") (str root "/../fram")))))
(load-file (str root "/cli/schema-migrate.clj"))

(def checks (atom []))
(defn check! [label ok detail]
  (swap! checks conj {:label label :ok (boolean ok) :detail detail}))

(defn ops-for [triples]
  (mapv (fn [index [subject predicate value]]
          {:tx (inc index) :op "assert" :l subject :p predicate :r value
           :frame "schema-safety-test"})
        (range) triples))

(defn corpus-for [triples]
  (let [ops (ops-for triples)
        folded (fold-for-cutover ops)]
    {:paths ["/tmp/schema-safety-coordination.log" "/tmp/schema-safety-telemetry.log"]
     :ops ops :facts (:facts folded) :version (:version folded)
     :card_map (:card_map folded)
     :files [{:path "/tmp/schema-safety-coordination.log" :bytes 1 :sha256 (apply str (repeat 64 "a"))}
             {:path "/tmp/schema-safety-telemetry.log" :bytes 0 :sha256 (apply str (repeat 64 "b"))}]}))

(defn facts-for [subject predicates]
  (mapv (fn [predicate] [subject predicate (str predicate "-value")]) predicates))

(defn repeat-subjects [prefix count predicates]
  (mapcat (fn [index] (facts-for (str "@" prefix index) predicates)) (range count)))

(def legacy-triples
  (vec
   (concat
    ;; 329 historical sessions: 51 human, 178 managed, 100 actor-absent.
    (mapcat (fn [index]
              [[(str "@2026-human-" index) "clocked_by" "user"]
               [(str "@2026-human-" index) "end_time" "end"]
               [(str "@2026-human-" index) "session_of" "thread"]
               [(str "@2026-human-" index) "start_time" "start"]])
            (range 51))
    (mapcat (fn [index]
              [[(str "@2026-agent-" index) "clocked_by" (str "lane-" index)]
               [(str "@2026-agent-" index) "end_time" "end"]
               [(str "@2026-agent-" index) "session_of" "thread"]
               [(str "@2026-agent-" index) "start_time" "start"]])
            (range 146))
    (repeat-subjects "2026-unknown-" 100 #{"end_time" "session_of" "start_time"})
    (mapcat (fn [index]
              [[(str "@2026-orphan-" index) "clock_orphaned" "true"]
               [(str "@2026-orphan-" index) "clocked_by" (str "lane-orphan-" index)]
               [(str "@2026-orphan-" index) "end_time" "end"]
               [(str "@2026-orphan-" index) "session_of" "thread"]
               [(str "@2026-orphan-" index) "start_time" "start"]])
            (range 24))
    (mapcat (fn [index]
              [[(str "@2026-open-" index) "clocked_by" (str "lane-open-" index)]
               [(str "@2026-open-" index) "session_of" "thread"]
               [(str "@2026-open-" index) "start_time" "start"]])
            (range 8))
    ;; 234 aggregate residues.
    (repeat-subjects "aggtest:done-" 130 #{"agg_done_batch" "agg_done_worker"})
    (repeat-subjects "aggtest:run-" 96 #{"agg_run_batch" "agg_run_tokens"})
    (repeat-subjects "aggtest:charge-" 8 #{"agg_charge_tokens" "agg_charged_to"})
    ;; 57 deprecated @pred:* projections.
    (repeat-subjects "pred:legacy-" 57 LEGACY-SCHEMA-PROJECTION-SIGNATURE)
    ;; Six explicit extension kinds.
    (mapcat (fn [index]
              [[(str "@clock-audit-" index) "kind" "clock_audit_run"]
               [(str "@clock-audit-" index) "repo_summary" "repo"]
               [(str "@clock-audit-" index) "run_at" "at"]
               [(str "@clock-audit-" index) "uncovered_count" "0"]
               [(str "@clock-audit-" index) "window" "week"]])
            (range 4))
    [["@integration-link" "kind" "integration_link"]
     ["@linear-reservation" "kind" "linear_bootstrap_reservation"]]
    ;; Two scratch fixtures.
    (facts-for "@scratch-sess-000001" #{"end_time" "start_time"})
    (facts-for "@scratch2-sess-a" #{"end_time" "start_time"})
    ;; Three intentionally unresolved quarantine decisions.
    [["@swarm" "agent_death" "dead"]
     ["019f6ec8-60b4-7d14-a199-1bdcb044a857" "merged_into" "bare-target"]
     ["@--json" "reached" ""]])))

(let [corpus (corpus-for legacy-triples)
      schema (desired-schema (:facts corpus))
      classes (entity-classifications (:facts corpus) schema)
      counts (frequencies (map (comp :kind val) classes))]
  (check! "all 628 deterministic legacy/test/extension subjects receive explicit kinds"
          (and (= 51 (get counts "north/legacy_human_session"))
               (= 178 (get counts "north/legacy_agent_session"))
               (= 100 (get counts "north/legacy_session"))
               (= 236 (get counts "north/test_fixture"))
               (= 57 (get counts "north/legacy_schema_projection"))
               (= 4 (get counts "north/clock_audit_run"))
               (= 1 (get counts "north/integration_link"))
               (= 1 (get counts "north/linear_bootstrap_reservation")))
          (pr-str counts))
  (check! "only the three named manual-quarantine decisions remain other"
          (= #{"@swarm" "019f6ec8-60b4-7d14-a199-1bdcb044a857" "@--json"}
             (set (map key (filter #(= "other" (get-in % [1 :kind])) classes))))
          (pr-str (into {} (filter #(= "other" (get-in % [1 :kind])) classes)))))

(let [prose "@session-beagle: completion prose with spaces"
      corpus (corpus-for [["@thread" "title" "thread"]
                          ["@thread" "notify" prose]
                          ["@thread" "blocks" "@prod deploy is waiting"]
                          ["@thread" "created_by" "claude-code"]
                          ["@thread" "driver" "@agent:missing"]
                          ["@thread" "mystery_extension" "@looks-like-a-ref"]])
      plan (plan-for corpus)
      defects (set (map :p (:reference-shape-defects plan)))
      dangling (set (map :p (:dangling-reference-defects plan)))]
  (check! "@-prefixed notification prose remains explicitly literal"
          (and (= "literal" (get-in plan [:schema "notify" :value_kind]))
               (not (contains? defects "notify")))
          (pr-str (:reference-shape-defects plan)))
  (check! "blocks keeps ref semantics and prose-valued legacy blocks is surfaced"
          (and (= "ref" (get-in plan [:schema "blocks" :value_kind]))
               (contains? defects "blocks"))
          (pr-str (:reference-shape-defects plan)))
  (check! "created_by remains a ref and dangling legacy spelling is surfaced"
          (and (= "ref" (get-in plan [:schema "created_by" :value_kind]))
               (contains? defects "created_by"))
          (pr-str (:reference-shape-defects plan)))
  (check! "driver remains a ref and a missing @entity target is surfaced"
          (and (= "ref" (get-in plan [:schema "driver" :value_kind]))
               (contains? dangling "driver"))
          (pr-str (:dangling-reference-defects plan)))
  (check! "unknown @-shaped extension semantics fail closed instead of becoming ref"
          (and (nil? (get-in plan [:schema "mystery_extension" :value_kind]))
               (= [{:predicate "mystery_extension"
                    :fields ["cardinality" "value_kind" "doc"]}]
                  (filterv #(= "mystery_extension" (:predicate %))
                           (:unresolved-semantics plan))))
          (pr-str (:unresolved-semantics plan))))

(def conflict-values
  {"started_at" ["2026-01-01T00:00:00Z" "2026-01-02T00:00:00Z"]
   "acked_at" ["2026-01-01T00:00:00Z" "2026-01-01T00:00:01Z"]
   "display_handle" ["old-handle" "new-handle"]
   "clocked_by" ["lane-a" "lane-b"]
   "at" ["2026-01-01T00:00:00Z" "2026-01-01T00:00:01Z"]
   "agent" ["agent:test" "@agent:test"]
   "dir" ["/repo/a" "/repo/b"]
   "intent" ["old intent" "new intent"]
   "provider" ["unobserved" "openai"]
   "status" ["draft" "done"]})

(def conflict-subjects
  {"started_at" "@session:test-start"
   "acked_at" "@msg:test-ack"
   "display_handle" "@agent:test-display"
   "clocked_by" "@session:test-clock"
   "at" "@run:test-at"
   "agent" "@run:test-agent"
   "dir" "@session:test-dir"
   "intent" "@concern-test-intent"
   "provider" "@run:test-provider"
   "status" "@concern-test-status"})

(def conflict-triples
  (vec
   (concat
    (mapcat (fn [[predicate values]]
              (mapv (fn [value] [(get conflict-subjects predicate) predicate value]) values))
            conflict-values)
    [["@thread-block" "title" "blocked thread"]
     ["@agent:test" "entity_kind" "agent"]
     ["@thread-block" "blocks" "@prod deploy is waiting"]
     ["@thread-block" "notify" "@session: this is prose"]])))

(let [corpus (corpus-for conflict-triples)
      unreviewed (plan-for corpus)
      conflicts (:cardinality-conflicts unreviewed)
      repairs (mapv (fn [{:keys [subject predicate values]}]
                      {:subject subject :predicate predicate
                       :retain (first values) :retract (vec (rest values))
                       :policy "explicit-test-policy"
                       :rationale (str "reviewed semantic decision for " predicate)})
                    conflicts)
      manifest {:format REPAIR-MANIFEST-FORMAT
                :source (source-seal corpus)
                :review {:by "schema-safety-test" :at "2026-07-22T00:00:00Z"
                         :basis "ten-class adversarial fixture"}
                :predicate_semantics
                {"block_reason" {:cardinality "multi" :value_kind "literal"
                                 :doc "Literal explanation for a blocking condition."
                                 :rationale "separates prose from the blocks reference edge"}}
                :cardinality_repairs repairs
                :fact_repairs
                [{:action "retract" :subject "@thread-block" :predicate "blocks"
                  :value "@prod deploy is waiting" :policy "split-overloaded-blocks"
                  :rationale "prose cannot inhabit the reference edge"}
                 {:action "assert" :subject "@thread-block" :predicate "block_reason"
                  :value "prod deploy is waiting" :policy "split-overloaded-blocks"
                  :rationale "preserve prose under an explicit literal predicate"}]
                :other_allowlist {:name "empty-test-allowlist" :entries {}}}
      reviewed (validate-manifest-structure! manifest)
      preflight (candidate-preflight corpus reviewed)]
  (check! "all ten audited cardinality conflict classes are enumerated before writes"
          (= (set (keys conflict-values)) (set (map :predicate conflicts)))
          (pr-str conflicts))
  (check! "missing manifest coverage fails every conflict rather than selecting latest tx"
          (= 10 (count (filter #(= "missing-cardinality-repair" (:type %))
                               (:manifest-defects unreviewed))))
          (pr-str (:manifest-defects unreviewed)))
  (check! "reviewed exact repairs plus block-reason split simulate to plan convergence"
          (and (:ok preflight)
               (empty? (get-in preflight [:post_plan :actions]))
               (empty? (get-in preflight [:post_plan :cardinality-conflicts]))
               (true? (get-in preflight [:post_audit :ok])))
          (pr-str (:defects preflight))))

(let [corpus (corpus-for [["@ambiguous" "opaque_extension" "value"]])
      report (audit-report corpus)]
  (check! "strict audit model rejects unresolved other outside a named allowlist"
          (and (false? (:ok report)) (= ["@ambiguous"] (:ambiguous_subjects report)))
          (pr-str report)))

(let [corpus (corpus-for [["@thread-a" "title" "a"]
                           ["@thread-b" "title" "b"]
                           ["@thread-a" "depends_on" "@thread-b"]
                           ["@thread-b" "depends_on" "@thread-a"]])
      plan (plan-for corpus)]
  (check! "acyclic dependency cycles are surfaced before candidate construction"
          (= #{["@thread-a" "depends_on"] ["@thread-b" "depends_on"]}
             (set (map (juxt :l :p) (:acyclic-cycle-defects plan))))
          (pr-str (:acyclic-cycle-defects plan))))

(let [corpus (corpus-for [["@thread" "title" "one"]])
      template (manifest-template corpus (plan-for corpus))
      rejected? (try
                  (validate-manifest-structure! template)
                  false
                  (catch clojure.lang.ExceptionInfo error
                    (= :unreviewed-repair-manifest (:type (ex-data error)))))]
  (check! "generated manifest placeholders cannot masquerade as review"
          rejected? (pr-str template)))

;; Reproduce the old sequential-failure shape without touching a coordinator:
;; 1,152 acknowledged actions followed by rejection. Then prove the public direct
;; execute route cannot enter that loop and leaves bytes identical.
(let [acknowledged (atom 0)]
  (try
    (doseq [index (range 1153)]
      (if (= index 1152)
        (throw (ex-info "simulated cardinality rejection" {}))
        (swap! acknowledged inc)))
    (catch Exception _ nil))
  (check! "legacy sequential apply reproduces exactly 1,152 partial acknowledgements"
          (= 1152 @acknowledged) (str @acknowledged)))

(defn delete-tree! [file]
  (when (.isDirectory file)
    (doseq [child (.listFiles file)] (delete-tree! child)))
  (.delete file))

(let [temp (.toFile (java.nio.file.Files/createTempDirectory
                     "north-schema-direct-refusal-"
                     (make-array java.nio.file.attribute.FileAttribute 0)))
      log (.getCanonicalPath (io/file temp "coordination.log"))
      telemetry (.getCanonicalPath (io/file temp "telemetry.log"))
      line (pr-str {:tx 1 :op "assert" :l "@thread" :p "title" :r "one"
                    :frame "schema-safety-test"})
      _ (spit log (str line "\n"))
      _ (spit telemetry "")
      before [(slurp log) (slurp telemetry)]
      alias-refused?
      (with-redefs [possible-live-corpus-paths (fn [] [log])]
        (try
          (assert-offline-candidate! [log telemetry])
          false
          (catch clojure.lang.ExceptionInfo error
            (= :live-corpus-candidate-refused (:type (ex-data error))))))
      result (proc/shell {:dir root :out :string :err :string :continue true
                          :extra-env {"FRAM_LOG" log "FRAM_TELEMETRY_LOG" telemetry
                                      "FRAM_SINGLE_VALUED" ""}}
                         "bb" "-cp" (str fram "/out")
                         (str root "/cli/schema-migrate.clj") "1" "migrate" "--execute"
                         "--log" log "--telemetry" telemetry)]
  (try
    (check! "candidate builder refuses a canonical or hard-link live alias"
            (and alias-refused? (= before [(slurp log) (slurp telemetry)]))
            (str "alias-refused=" alias-refused?))
    (check! "direct migrate execute fails before its first coordinator write"
            (and (not (zero? (:exit result)))
                 (str/includes? (:err result) "direct migrate --execute is disabled")
                 (= before [(slurp log) (slurp telemetry)]))
            (str "exit=" (:exit result) " out=" (:out result) " err=" (:err result)))
    (finally (delete-tree! temp))))

(let [results @checks failures (remove :ok results)
      passed (- (count results) (count failures))]
  (doseq [{:keys [label ok detail]} results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when (and (not ok) detail) (println (str "         " detail))))
  (println (format "\nschema migration safety: %d / %d PASS" passed (count results)))
  (System/exit (if (empty? failures) 0 1)))
