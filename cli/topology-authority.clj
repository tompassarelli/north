(ns north.topology-authority
  "Direct coordination authority boundary shared by North's Clojure adapters.

  Tool visibility is not authorization. A managed worker must be rejected at
  the first executable adapter boundary even when it invokes that adapter
  directly. An absent topology remains valid for a top-level interactive
  session; an explicitly declared topology must be `orchestrator`."
  (:require [clojure.string :as str]))

;; Injectable without mutating process environment in executable-boundary tests.
(def ^:dynamic *topology* ::ambient)

(defn current-topology []
  (let [value (if (= *topology* ::ambient)
                (System/getenv "AGENT_TOPOLOGY")
                *topology*)]
    (some-> value str str/trim not-empty)))

(defn authority-problem [operation]
  (let [topology (current-topology)]
    (when (and topology (not= topology "orchestrator"))
      (str "coordination authority denied: " operation
           " requires orchestrator topology; current topology is " topology))))

(defn managed-child-topology-problem
  "A top-level caller may create the one orchestrator tier. A managed
  orchestrator may create terminal workers only; allowing another orchestrator
  would silently grow a third tier. Worker callers are rejected separately by
  require-coordination!/authority-problem before routing is composed."
  [operation effective-child-topology]
  (when (and (= "orchestrator" (current-topology))
             (= "orchestrator" effective-child-topology))
    (str "coordination depth denied: " operation
         " from an orchestrator may create worker topology only; "
         "managed depth stops at orchestrator -> worker")))

(defn require-coordination! [operation]
  (when-let [problem (authority-problem operation)]
    (throw (ex-info problem
                    {::denied true
                     :north/error :topology-authority-denied
                     :operation operation
                     :topology (current-topology)
                     :pre-side-effect true}))))

(defn require-managed-child-topology! [operation effective-child-topology]
  (when-let [problem
             (managed-child-topology-problem operation effective-child-topology)]
    (throw (ex-info problem
                    {::denied true
                     :north/error :topology-depth-denied
                     :operation operation
                     :caller-topology (current-topology)
                     :child-topology effective-child-topology
                     :pre-side-effect true}))))

(defn require-self-agent!
  "Allow a managed worker to mutate its own agent record, but require
  coordination authority for another agent. Top-level/unclassified sessions
  retain the same ambient authority as `require-coordination!`."
  [operation agent-id]
  (let [topology (current-topology)
        target (some-> agent-id str (str/replace-first #"^@?(?:agent:)?" ""))
        self (some-> (System/getenv "AGENT_ID") str str/trim not-empty)]
    (when (and topology (not= topology "orchestrator") (not= target self))
      (require-coordination! operation))))

(defn denial? [error]
  (true? (::denied (ex-data error))))
