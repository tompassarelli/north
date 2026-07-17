(ns north.managed-child-env
  "One process-boundary rule for every managed North child: routing and staffing
  come from the child request, never from ambient parent AGENT_* state.")

(def routing-keys
  ["AGENT_ID" "NORTH_AGENT_ID" "AGENT_IDENTITY_ROLE" "AGENT_ROLE" "AGENT_TASK_GRADE"
   "AGENT_DOMAIN_REQUIREMENTS" "AGENT_TOPOLOGY" "AGENT_TIER"
   "AGENT_REASONING" "AGENT_EFFORT" "AGENT_POSTURE" "AGENT_COMPOSITION"
   "AGENT_MODEL" "AGENT_TARGET" "AGENT_PROVIDER" "AGENT_COORDINATOR"
   "NORTH_DISPATCH_DRIVER_PRECLAIMED"])

(defn scrub
  ([] (scrub (into {} (System/getenv))))
  ([parent-env] (apply dissoc parent-env routing-keys)))

(defn child
  "Build a clean child environment, preserving coordinator attribution only
  through the explicit argument and applying request-owned overrides last."
  [parent-env coordinator overrides]
  (merge (scrub parent-env)
         (when coordinator {"AGENT_COORDINATOR" coordinator})
         overrides))
