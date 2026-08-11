(ns north.worker-policy)

(def concern-idle-ms 1000)

(def concern-maximum-backoff-ms 30000)

(def attention-interval-ms 30000)

(def attention-maximum-backoff-ms 300000)

(def attention-reconcile-limit 64)

(defrecord WorkerDecision [action sleep-ms next-backoff-ms])

(defn workerdecision-action [r] (:action r))

(defn workerdecision-sleep-ms [r] (:sleep-ms r))

(defn workerdecision-next-backoff-ms [r] (:next-backoff-ms r))

(defn bounded-double [value maximum]
  (let [doubled (* value 2)]
  (if (> doubled maximum) maximum doubled)))

(defn ^WorkerDecision concern-reconciliation-decision [pending-count exit-code backoff-ms]
  (cond
  (<= pending-count 0) (->WorkerDecision :sleep concern-idle-ms concern-idle-ms)
  (= exit-code 0) (->WorkerDecision :run 0 concern-idle-ms)
  (= exit-code 3) (->WorkerDecision :sleep concern-idle-ms concern-idle-ms)
  :else (->WorkerDecision :sleep backoff-ms (bounded-double backoff-ms concern-maximum-backoff-ms))))

(defn ^WorkerDecision attention-reconciliation-decision [exit-code backoff-ms ^Boolean more-work]
  (cond
  (not (= exit-code 0)) (->WorkerDecision :sleep backoff-ms (bounded-double backoff-ms attention-maximum-backoff-ms))
  more-work (->WorkerDecision :run 0 attention-interval-ms)
  :else (->WorkerDecision :sleep attention-interval-ms attention-interval-ms)))

(defn ^Boolean scheduled-task? [task]
  (or (= task :stale-concerns) (or (= task :stale-lanes) (or (= task :worktrees) (or (= task :agent-logs) (= task :spend-guard))))))

(defn task-cadence-ms [task]
  (cond
  (= task :spend-guard) 60000
  (= task :stale-lanes) 300000
  (= task :stale-concerns) 900000
  (= task :worktrees) 900000
  (= task :agent-logs) 3600000
  :else 0))

(defn task-timeout-ms [task]
  (cond
  (= task :spend-guard) 30000
  (= task :stale-lanes) 120000
  (= task :stale-concerns) 120000
  (= task :worktrees) 120000
  (= task :agent-logs) 120000
  :else 0))
