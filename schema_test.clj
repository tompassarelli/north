;; schema_test.clj — the vocabulary census (`north schema`): the kind classifier
;; and the census roll-up.
;;   (1) kind-of: explicit `entity_kind` authority > legacy `kind` compatibility
;;       > namespace/shape/schema heuristics > other. Namespaced extensions are
;;       preserved exactly instead of collapsing into presentation buckets.
;;   (2) census: per-kind subject + fact counts, sorted by fact count desc.
;;   (3) predicate metadata (cardinality/value_kind) is surfaced from the graph.
;;   bb -cp out:../fram/out schema_test.clj      (run from the repo root)
(require '[fram.kernel :as k] '[north.main :as m])

;; one subject per kind: authoritative, legacy-compatible, and inferred rows.
(def facts
  [(k/->Fact "@t1" "kind" "thread")   (k/->Fact "@t1" "title" "Kinded thread")
   (k/->Fact "@2026-05-01-000000" "title" "Legacy thread (no kind)")
   (k/->Fact "concern-a" "kind" "concern")  (k/->Fact "concern-a" "title" "Kinded concern")
   (k/->Fact "@concern-b" "title" "Prefix concern (no kind)")
   (k/->Fact "@agent:x" "display_name" "Agent X")
   (k/->Fact "@msg:m1" "body" "hello")
   (k/->Fact "@topic-perf" "note" "a topic")
   (k/->Fact "@mine:1" "kind" "mine")   (k/->Fact "@mine:1" "note" "personal")
   ;; Explicit structure wins even when stale legacy classification disagrees.
   (k/->Fact "@run-9" "entity_kind" "run") (k/->Fact "@run-9" "kind" "session")
   (k/->Fact "@run-9" "started_at" "t")
   (k/->Fact "@client-clock" "kind" "client_session") (k/->Fact "@client-clock" "owner" "acme")
   (k/->Fact "@session:s1" "started_at" "t")   (k/->Fact "@session:s1" "agent" "cc")
   (k/->Fact "@denial:g1" "reason" "guarded")
   (k/->Fact "@person:p1" "display_name" "Person P")
   (k/->Fact "@vendor:x" "entity_kind" "vendor/widget") (k/->Fact "@vendor:x" "note" "open extension")
   (k/->Fact "@depends_on" "cardinality" "single")  (k/->Fact "@depends_on" "acyclic" "true")
   (k/->Fact "@rate" "value_kind" "literal")
   (k/->Fact "@weird" "foo" "bar")
   ;; A historical malformed write must remain inspectable through the no-arg
   ;; census instead of taking the entire schema command down.
   (k/->Fact nil "reached" "")
   ;; a synthetic legacy `gadget` kind for the per-kind field spec (required vs optional):
   ;; `name` on 3/3 subjects (100% => REQUIRED), `color` on 1/3 (33% => OPTIONAL),
   ;; `tag` asserted twice on ONE subject (coverage must dedup to 1 subject, not 2).
   (k/->Fact "@g1" "kind" "gadget")  (k/->Fact "@g1" "name" "a")
   (k/->Fact "@g1" "color" "red")    (k/->Fact "@g1" "tag" "x")  (k/->Fact "@g1" "tag" "y")
   (k/->Fact "@g2" "kind" "gadget")  (k/->Fact "@g2" "name" "b")
   (k/->Fact "@g3" "kind" "gadget")  (k/->Fact "@g3" "name" "c")])
(def idx (k/build-index facts))
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
  (filter (fn [s] (or (some? (k/one-i idx s "cardinality")) (some? (k/one-i idx s "value_kind"))))
          (:subjects idx)))

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
   ["legacy client_session maps to core kind"        (= "client_session" (kof "@client-clock"))]
   ["prefix session: maps to agent"                  (= "agent" (kof "@session:s1"))]
   ["prefix denial: maps to guard_denial"            (= "guard_denial" (kof "@denial:g1"))]
   ["display_name shape maps to person"              (= "person" (kof "@person:p1"))]
   ["explicit namespaced extension is preserved"    (= "vendor/widget" (kof "@vendor:x"))]
   ["schema-as-facts subject => predicate"          (= "predicate" (kof "@depends_on"))]
   ["unclassifiable => other"                       (= "other" (kof "@weird"))]
   ["malformed nil subject => other"                (= "other" (kof nil))]
   ["no-arg schema tolerates malformed subject"     (.contains no-arg-schema-output "SCHEMA —")]
   ["census: 2 thread subjects"                     (= 2 (subj-of "thread"))]
   ["census: 2 concern subjects"                    (= 2 (subj-of "concern"))]
   ["census: run remains its own core kind"          (= 1 (subj-of "run"))]
   ["census: client_session remains core kind"       (= 1 (subj-of "client_session"))]
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
   ["schema-seed compatibility path is non-writing" (let [result (atom nil)
                                                            output (with-out-str
                                                                     (reset! result (m/run-status ["schema-seed" "--execute"] "" "")))]
                                                        (and (= 2 @result)
                                                             (.contains output "RETIRED")
                                                             (.contains output "no facts were written")))]
   ["writers map: uncurated kind => not curated"    (.contains (#'m/kind-writer "zzz") "not curated")]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (if (empty? fails)
    (println "\nschema:" (count checks) "/" (count checks) "PASS")
    (do (println "\nschema:" (count fails) "FAILED") (System/exit 1))))
