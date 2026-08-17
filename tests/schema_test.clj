;; schema_test.clj — the vocabulary census (`north schema`): the kind classifier
;; and the census roll-up.
;;   (1) kind-of: explicit `entity_kind` authority > legacy `kind` compatibility
;;       > namespace/shape/schema heuristics > other. Namespaced extensions are
;;       preserved exactly instead of collapsing into presentation buckets.
;;   (2) census: per-kind subject + fact counts, sorted by fact count desc.
;;   (3) predicate metadata (cardinality/value_kind) is surfaced from the graph.
;;   bb -cp out:/home/tom/code/beagle/main/branch-core/out tests/schema_test.clj
(require '[fram.types :as t] '[north.projections :as proj]
         '[north.main :as m])

;; one subject per kind: authoritative, legacy-compatible, and inferred rows.
(def facts
  [(t/triple "@t1" "kind" "thread")   (t/triple "@t1" "title" "Kinded thread")
   (t/triple "@2026-05-01-000000" "title" "Legacy thread (no kind)")
   (t/triple "concern-a" "kind" "concern")  (t/triple "concern-a" "title" "Kinded concern")
   (t/triple "@concern-b" "title" "Prefix concern (no kind)")
   (t/triple "@agent:x" "display_name" "Agent X")
   (t/triple "@msg:m1" "body" "hello")
   (t/triple "@topic-perf" "note" "a topic")
   (t/triple "@mine:1" "kind" "mine")   (t/triple "@mine:1" "note" "personal")
   ;; Explicit structure wins even when stale legacy classification disagrees.
   (t/triple "@run-9" "entity_kind" "run") (t/triple "@run-9" "kind" "session")
   (t/triple "@run-9" "started_at" "t")
   (t/triple "@session:s1" "started_at" "t")   (t/triple "@session:s1" "agent" "cc")
   (t/triple "@denial:g1" "reason" "guarded")
   (t/triple "@person:p1" "display_name" "Person P")
   (t/triple "@vendor:x" "entity_kind" "vendor/widget") (t/triple "@vendor:x" "note" "open extension")
   (t/triple "@depends_on" "cardinality" "single")  (t/triple "@depends_on" "acyclic" "true")
   (t/triple "@rate" "value_kind" "literal")
   (t/triple "@weird" "foo" "bar")
   (t/triple "@other2" "foo" "baz")
   ;; a synthetic legacy `gadget` kind for the per-kind field spec (required vs optional):
   ;; `name` on 3/3 subjects (100% => REQUIRED), `color` on 1/3 (33% => OPTIONAL),
   ;; `tag` asserted twice on ONE subject (coverage must dedup to 1 subject, not 2).
   (t/triple "@g1" "kind" "gadget")  (t/triple "@g1" "name" "a")
   (t/triple "@g1" "color" "red")    (t/triple "@g1" "tag" "x")  (t/triple "@g1" "tag" "y")
   (t/triple "@g2" "kind" "gadget")  (t/triple "@g2" "name" "b")
   (t/triple "@g3" "kind" "gadget")  (t/triple "@g3" "name" "c")])
(def idx (proj/index-triples facts))
(defn kof [te] (#'m/kind-of idx te))

(def stats (#'m/census idx facts))
(defn stat-for [kd] (first (filter #(= (:kind %) kd) stats)))

;; per-kind field spec (required/optional + coverage %) — the schema-fields fold.
(defn fields-for [kd] (#'m/schema-fields idx facts kd))
(defn field [kd p] (first (filter #(= (:pred %) p) (fields-for kd))))
(defn subj-of [kd] (let [s (stat-for kd)] (if s (:subjects s) 0)))

;; census sorted by fact count descending?
(def facts-desc?
  (apply >= (cons Long/MAX_VALUE (mapv :facts stats))))

;; predicate-metadata subjects the schema view surfaces (cardinality|value_kind)
(def pred-subs
  (filter (fn [s] (or (some? (proj/string-value-at idx s "cardinality"))
                      (some? (proj/string-value-at idx s "value_kind"))))
          (proj/all-subjects idx)))

(def no-arg-schema-output
  (with-redefs-fn {#'m/live-facts (fn [_] facts)}
    #(with-out-str (m/cmd-schema "ignored" ""))))

(def checks
  [["legacy kind fallback: @t1 => thread"          (= "thread" (kof "@t1"))]
   ["title (no kind) => thread"                    (= "thread" (kof "@2026-05-01-000000"))]
   ["kind fact: concern-a => concern"              (= "concern" (kof "concern-a"))]
   ["prefix (bare/@): @concern-b => concern"       (= "concern" (kof "@concern-b"))]
   ["prefix agent:  => agent"                      (= "agent" (kof "@agent:x"))]
   ["prefix msg:    => message"                     (= "message" (kof "@msg:m1"))]
   ["prefix topic-  => topic"                       (= "topic" (kof "@topic-perf"))]
   ["legacy mine becomes namespaced extension"      (= "north/mine" (kof "@mine:1"))]
   ["explicit entity_kind wins over legacy kind"    (= "run" (kof "@run-9"))]
   ["prefix session: maps to agent"                  (= "agent" (kof "@session:s1"))]
   ["prefix denial: maps to guard_denial"            (= "guard_denial" (kof "@denial:g1"))]
   ["display_name shape maps to person"              (= "person" (kof "@person:p1"))]
   ["explicit namespaced extension is preserved"    (= "vendor/widget" (kof "@vendor:x"))]
   ["schema-as-facts subject => predicate"          (= "predicate" (kof "@depends_on"))]
   ["unclassifiable => other"                       (= "other" (kof "@weird"))]
   ["absent nil subject => other"                   (= "other" (kof nil))]
   ["no-arg schema renders the typed corpus"        (.contains no-arg-schema-output "SCHEMA —")]
   ["census: 2 thread subjects"                     (= 2 (subj-of "thread"))]
   ["census: 2 concern subjects"                    (= 2 (subj-of "concern"))]
   ["census: run remains its own core kind"          (= 1 (subj-of "run"))]
   ["census preserves namespaced extension"          (= 1 (subj-of "vendor/widget"))]
   ["census: 2 other subjects"                      (= 2 (subj-of "other"))]
   ["census sorted by fact count desc"              facts-desc?]
   ["predicate metadata surfaces depends_on"        (some #{"@depends_on"} pred-subs)]
   ["predicate metadata surfaces rate"              (some #{"@rate"} pred-subs)]
   ;; per-kind field spec: required (>=98%) vs optional, coverage %, dedup
   ["field spec: gadget/name is REQUIRED (100%)"    (:required (field "gadget" "name"))]
   ["field spec: gadget/name pct = 100"             (= 100 (:pct (field "gadget" "name")))]
   ["field spec: gadget/color is OPTIONAL"          (not (:required (field "gadget" "color")))]
   ["field spec: gadget/color pct = 33"             (= 33 (:pct (field "gadget" "color")))]
   ["coverage dedups multi-valued: tag subs = 1"    (= 1 (:subs (field "gadget" "tag")))]
   ["field spec: required sorts before optional"    (:required (first (fields-for "gadget")))]
   ["writers map: thread => capture-facts"          (.contains (#'m/kind-writer "thread") "capture-facts")]
   ["writers map: uncurated kind => not curated"    (.contains (#'m/kind-writer "zzz") "not curated")]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (if (empty? fails)
    (println "\nschema:" (count checks) "/" (count checks) "PASS")
    (do (println "\nschema:" (count fails) "FAILED") (System/exit 1))))
