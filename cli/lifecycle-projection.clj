(ns north.lifecycle-projection
  (:require [clojure.set :as set]))

(def identity-marker-predicate "identity_manifest_sha256")

(def terminal-marker-predicate "terminal_manifest_sha256")

(def managed-agent-predicates (-> (set/union north.agent-provenance/identity-predicates (set north.agent-provenance/required-identity-predicates) north.agent-provenance/terminal-predicates (set north.terminal-projection/terminal-projection-predicates) #{identity-marker-predicate terminal-marker-predicate}) sort vec))

(def trace-session-display-predicates #{"current_thread" "active_workflow" "task" "stalled"})

(def trace-agent-predicates (-> (set/union (set managed-agent-predicates) trace-session-display-predicates) sort vec))

(def route-guard-predicates (-> (set/union (set north.terminal-projection/terminal-projection-predicates) #{identity-marker-predicate terminal-marker-predicate "live_input" "live_input_state" "live_input_epoch" "shadow_reviewer_note_capability_sha256"}) sort vec))

(def reported-run-predicates (-> (set/union (set north.terminal-projection/run-reservation-predicates) #{"run_bar_evidence"}) sort vec))

(def reported-thread-predicates ["done_when"])

(def managed-lifecycle-predicates (-> (set/union (set trace-agent-predicates) (set north.terminal-projection/run-resolution-predicates) (set reported-run-predicates) (set reported-thread-predicates)) sort vec))

(defn raw-point-facts
  "Read exactly PREDICATES for SUBJECT and retain every distinct raw value.\n  READ-MANY has shape (fn [subject predicate] values). Malformed point-read\n  responses fail closed instead of silently dropping lifecycle evidence." [read-many subject predicates]
  (reduce (fn [facts predicate] (let [values (read-many subject predicate)]
  (if (not (and (sequential? values) (every? string? values))) (do
  (throw (ex-info "managed lifecycle point read was malformed" {:subject subject :predicate predicate}))))
  (if (seq values) (assoc facts predicate (set values)) facts))) {} predicates))

(defn folded-agent-point-facts
  "Read the canonical managed-agent projection and preserve identity conflicts\n  plus raw multi-valued terminal evidence through the shared provenance fold."
  ([read-many subject]
    (folded-agent-point-facts read-many subject managed-agent-predicates))
  ([read-many subject predicates]
    (reduce-kv (fn [facts predicate values] (reduce (fn [current value] (north.agent-provenance/fold-fact current predicate value)) facts (sort values))) {} (raw-point-facts read-many subject predicates))))
