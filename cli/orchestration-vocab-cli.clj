;; orchestration-vocab-cli.clj — Phase 0 (inert) seed for the Orchestration -> North
;; Orchestration migration vocabulary (thread 019f8f5c-74e0-7be7-ba65-3179f1bccde1;
;; design doc: north-orchestration-vocabulary-design.md in the repo's private docs —
;; packaged code must not embed checkout paths, per the package path-hygiene lint).
;;
;; This registers DATA only: the 13 @entity-kind:* definitions owned below and
;; the five @shape:<kind> subjects design section 2.1 spells out explicitly
;; (template, model, selection_rule, task, shape-the-meta-shape).
;;
;; Every shape is seeded with enforcement "unshaped" — the inert dial (design
;; section 2.3). Nothing reads or enforces these facts yet; no interpreter,
;; write path, or spawn/dispatch code changed. New predicate registration
;; itself is `bb pred-cli.clj <port> seed` (VOCAB already carries the new rows).
;;
;; usage:
;;   bb orchestration-vocab-cli.clj <port> seed     assert kind + shape data (idempotent)
;;   bb orchestration-vocab-cli.clj <port> show     print what is on the graph
;;   bb orchestration-vocab-cli.clj <port> retract  undo seed (rollback path)
(require '[clojure.java.io :as io])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

;; This command owns the exact orchestration kind vocabulary it publishes.
(def ORCHESTRATION-ENTITY-KINDS
  (sorted-map
   "template" "Reusable behavior and routing defaults whose composition provenance is independent of role."
   "axis_value" "A first-class value of an orchestration axis (task_grade/tier/reasoning/posture/topology/capability/...)."
   "provider_catalog" "A provider's calibrated model/transport/provenance catalog vintage."
   "model" "One provider model with its calibrated routes, context window, and delta."
   "tier_row" "The canonical model + deliberation levels resolved for one provider/tier pair."
   "selection_policy" "A named, digest-pinned set of selection_rule subjects (e.g. minimum-sufficient-v1)."
   "selection_rule" "One signal -> minimum tier/reasoning floor rule under a selection_policy."
   "selection_signal" "A routing signal's name and enumerated legal values."
   "shape" "A kind-scoped default-deny predicate allowlist, itself governed by @shape:shape."
   "wire_contract" "A queryable subject documenting one coordinator wire contract (fields, example, error codes)."
   "staffing_catalog" "Catalog-level defaults for template axes (task_grade/tier/reasoning/posture/topology)."
   "doctrine_block" "A graph-resident prompt_block not attached to a template (e.g. comms doctrine)."
   "task" "A coordination subject recording proposed_by, delegate, and done_when; actor ownership moves only through Agent Machinery acknowledgement."))

(defn exact-facts [port subject]
  (->> (north.coord/query-rows
        port
        {:find "p,v" :rules [{:head {:rel "p,v" :args [{:var "p"} {:var "v"}]}
                               :body [{:rel "triple" :args [subject {:var "p"} {:var "v"}]}]}]})
       (map (fn [row] [(nth row 0) (nth row 1)]))
       (sort-by (juxt first second))))

(def ^:dynamic *publication-actions* nil)

(defn publish-actions! [port actions]
  (let [result (north.coord/publish! port (vec actions))]
    (when (:reject result)
      (throw (ex-info "Store RPC rejected orchestration vocabulary publication"
                      {:type :vocabulary-publication-rejected :result result})))
    result))

(defn queue-set! [subject predicate values cardinality]
  (when-not *publication-actions*
    (throw (ex-info "orchestration vocabulary mutation requires one publication scope"
                    {:type :missing-publication-scope})))
  (swap! *publication-actions* conj
         {:op :set :subject subject :predicate predicate
          :values (vec (map str values)) :cardinality cardinality}))

(defn collect-publication! [port operation!]
  (let [actions (atom [])
        result (binding [*publication-actions* actions] (operation!))]
    (publish-actions! port @actions)
    result))

(defn set-1! [_port subject predicate value]
  (queue-set! subject predicate [value] :one))

(defn set-multi! [_port subject predicate values]
  (queue-set! subject predicate values :many))

;; ============================================================================
;; Entity-kind definitions use the canonical entity_kind definition shape.
;; ============================================================================
(def ENTITY-KIND-DEFINITION "north/entity_kind_definition")

(defn seed-entity-kinds! [port]
  (doseq [[kind doc] ORCHESTRATION-ENTITY-KINDS]
    (let [subject (str "@entity-kind:" kind)]
      (set-1! port subject "entity_kind" ENTITY-KIND-DEFINITION)
      (set-1! port subject "entity_kind_name" kind)
      (set-1! port subject "doc" doc)))
  (count ORCHESTRATION-ENTITY-KINDS))

(defn retract-entity-kinds! [port]
  (doseq [[kind _] ORCHESTRATION-ENTITY-KINDS]
    (let [subject (str "@entity-kind:" kind)]
      (doseq [p ["entity_kind" "entity_kind_name" "doc"]]
        (queue-set! subject p [] :one))))
  (count ORCHESTRATION-ENTITY-KINDS))

;; ============================================================================
;; Shape subjects — design section 2.1, verbatim. Five shapes only: the ones
;; the design spells out explicitly. Every other new kind (axis_value,
;; provider_catalog, tier_row, selection_policy, selection_signal, wire_contract,
;; staffing_catalog, doctrine_block) stays unshaped in Phase 0 — its kind
;; definition is registered (above) but it gets no @shape:* subject yet; a later
;; phase mints one as a governed graph edit (design section 2.2), never a code
;; change.
;; ============================================================================
(def SHAPES
  {"template"
   {:required ["name" "task_grade" "topology" "tier" "reasoning" "posture" "capability" "tagline" "doc"]
    :extra-allowed ["prompt_block" "doctrine_source" "kind" "minted_by" "minted_at"]}
   "model"
   {:required ["provider" "deliberation_support" "calibrated_route"
               "context_window_tokens" "context_window_from" "delta_kind"]
    :extra-allowed ["delta_reason" "prompt_block" "doctrine_source" "alias" "kind" "minted_by" "minted_at"]}
   "selection_rule"
   {:required ["signal" "signal_value" "min_tier" "min_reasoning" "rule_code"]
    :extra-allowed ["kind" "minted_by" "minted_at"]}
   "task"
   {:required ["proposed_by" "delegate" "done_when"]
    :extra-allowed ["kind" "minted_by" "minted_at" "progress" "outcome"]
    :structural-rules ["distinct:proposed_by,delegate"]}
   "shape"
   {:required ["applies_to_kind" "required_predicate" "allowed_predicate" "enforcement"]
    :extra-allowed ["structural_rule" "kind" "minted_by" "minted_at" "doc"]}})

;; Phase 0's own enforcement dial value: inert. `unshaped` reads as "no shape
;; governs this kind yet" in design prose; seeding the shape subject itself with
;; `enforcement "unshaped"` keeps the same self-description machine-readable —
;; a later governed `tell @shape:<kind> enforcement warn` is the sole way to
;; move a kind along the dial (design section 2.3), never a code deploy.
(def PHASE-0-ENFORCEMENT "unshaped")

(defn seed-shape! [port kind {:keys [required extra-allowed structural-rules]}]
  (let [subject (str "@shape:" kind)
        allowed (vec (distinct (concat required extra-allowed)))]
    (set-1! port subject "kind" "shape")
    (set-1! port subject "applies_to_kind" kind)
    (set-multi! port subject "required_predicate" required)
    (set-multi! port subject "allowed_predicate" allowed)
    (set-multi! port subject "structural_rule" (or structural-rules []))
    (set-1! port subject "enforcement" PHASE-0-ENFORCEMENT)
    subject))

(defn seed-shapes! [port]
  (doseq [[kind spec] SHAPES] (seed-shape! port kind spec))
  (count SHAPES))

(defn retract-shape! [port kind]
  (let [subject (str "@shape:" kind)]
    (doseq [p ["kind" "applies_to_kind" "enforcement"]]
      (queue-set! subject p [] :one))
    (doseq [p ["required_predicate" "allowed_predicate" "structural_rule"]]
      (queue-set! subject p [] :many))))

(defn retract-shapes! [port]
  (doseq [[kind _] SHAPES] (retract-shape! port kind))
  (count SHAPES))

;; ============================================================================
(let [[ps verb] *command-line-args*
      port (Integer/parseInt (or ps "7977"))]
  (case verb
    "seed"
    (let [[kinds shapes]
          (collect-publication!
           port
           #(vector (seed-entity-kinds! port) (seed-shapes! port)))]
      (println (format "✓ seeded %d @entity-kind:* definitions and %d @shape:* subjects on :%d (enforcement=%s)"
                       kinds shapes port PHASE-0-ENFORCEMENT)))

    "retract"
    (let [[kinds shapes]
          (collect-publication!
           port
           #(vector (retract-entity-kinds! port) (retract-shapes! port)))]
      (println (format "✓ retracted %d @entity-kind:* definitions and %d @shape:* subjects on :%d"
                       kinds shapes port)))

    "show"
    (do
      (doseq [[kind _] ORCHESTRATION-ENTITY-KINDS]
        (let [subject (str "@entity-kind:" kind)
              facts (exact-facts port subject)]
          (println subject)
          (doseq [[p v] facts] (println (format "  %-20s %s" p v)))))
      (doseq [[kind _] SHAPES]
        (let [subject (str "@shape:" kind)
              facts (exact-facts port subject)]
          (println subject)
          (doseq [[p v] facts] (println (format "  %-20s %s" p v))))))

    (do (println "usage: orchestration-vocab-cli.clj <port> {seed | show | retract}")
        (System/exit 2))))
