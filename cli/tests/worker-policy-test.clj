#!/usr/bin/env bb
(require '[clojure.java.io :as io])

(def test-file (io/file (System/getProperty "babashka.file")))
(def root (-> test-file .getParentFile .getParentFile .getParentFile .getCanonicalPath))
(load-file (str root "/out/north/worker_policy.clj"))

(def checks (atom []))
(defn check [label value & [detail]]
  (swap! checks conj [label (boolean value) detail]))

(check "bounded doubling stops at the declared maximum"
       (= 30000 (north.worker-policy/bounded-double 20000 30000)))

(check "empty concern spool sleeps at the idle cadence"
       (= {:action :sleep :sleep-ms 1000 :next-backoff-ms 1000}
          (into {}
                (north.worker-policy/concern-reconciliation-decision
                 0 0 8000))))

(check "successful concern pass immediately continues while work remains"
       (= {:action :run :sleep-ms 0 :next-backoff-ms 1000}
          (into {}
                (north.worker-policy/concern-reconciliation-decision
                 4 0 8000))))

(check "already-owned concern work yields at the idle cadence"
       (= {:action :sleep :sleep-ms 1000 :next-backoff-ms 1000}
          (into {}
                (north.worker-policy/concern-reconciliation-decision
                 4 3 8000))))

(check "failed concern pass applies bounded exponential backoff"
       (= {:action :sleep :sleep-ms 20000 :next-backoff-ms 30000}
          (into {}
                (north.worker-policy/concern-reconciliation-decision
                 4 1 20000))))

(check "successful attention pass returns to its normal cadence"
       (= {:action :sleep :sleep-ms 30000 :next-backoff-ms 30000}
          (into {}
                (north.worker-policy/attention-reconciliation-decision
                 0 120000 false))))

(check "attention backlog continues immediately in bounded passes"
       (= {:action :run :sleep-ms 0 :next-backoff-ms 30000}
          (into {}
                (north.worker-policy/attention-reconciliation-decision
                 0 120000 true))))

(check "failed attention pass backs off without exceeding five minutes"
       (= {:action :sleep :sleep-ms 200000 :next-backoff-ms 300000}
          (into {}
                (north.worker-policy/attention-reconciliation-decision
                 1 200000 true))))

(let [tasks [:stale-concerns :stale-lanes :worktrees :agent-logs :spend-guard]]
  (check "only the five independently scheduled tasks are admitted"
         (and (every? north.worker-policy/scheduled-task? tasks)
              (not (north.worker-policy/scheduled-task? :everything))))
  (check "every admitted task has a positive cadence and timeout"
         (every?
          (fn [task]
            (and (pos? (north.worker-policy/task-cadence-ms task))
                 (pos? (north.worker-policy/task-timeout-ms task))))
          tasks)))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok detail] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
    (when (and (not ok) detail) (println (str "        " detail))))
  (println (format "\nworker policy: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
