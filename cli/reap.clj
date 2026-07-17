;; reap.clj — PURE liveness-reap decisions, split from north-reactor.clj's I/O so the
;; verdict is unit-testable off in-memory facts (../reap_test.clj) with no live daemon.
;; The reactor GATHERS facts through the coordinator, then calls these; the test feeds
;; the same shapes directly. No coordinator, no clock, no atoms here — inputs in, verdict
;; out. Loaded (not required) the same way north-reactor.clj loads coord.clj.
(ns north.reap
  (:require [clojure.string :as str]))

(def LANE-STALE-MS    (* 30 60 1000))      ; 30min silent + no outcome -> dead lane
(def CONCERN-STALE-MS (* 24 60 60 1000))   ; 24h owner-lapsed -> abandoned-stale concern
(def SDK-AGENT-ID-EPOCH-FLOOR-MS
  ;; The timestamp+full-UUID format was introduced in July 2026. Refuse to
  ;; interpret older/legacy strings as clocks: a false negative needs manual
  ;; cleanup; a false positive could steal a live driver's thread.
  (.toEpochMilli (java.time.Instant/parse "2026-07-01T00:00:00Z")))
(def ^:private uuid-suffix
  #"-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")

(defn lane-resolved?
  "RESOLVED (never reap) iff a terminal fact exists for lane <h> — even though it may
   live on a DIFFERENT subject than @agent:<h>. The PRIMARY signal is the lane's own
   outcome: the spawn/dispatch finalize writes `outcome` on @agent:<h> SYNCHRONOUSLY
   (sdk/src/identity.ts writeAgentOutcome) at every terminal path, so reap-lane?'s
   empty-outcome guard already excludes a reported terminal. This join is the SECONDARY
   net: recordRun ALSO lands the outcome on @run-<h>-<ts> carrying `agent`=<h> (async,
   best-effort), so even if the lane write were lost the run trail resolves it. Join
   through agent=<h>: `tagged-outcomes` = the outcome-seq of EACH subject carrying
   agent=<h> (runs + session); `deaths` = @swarm agent_death lines (`<id> | reason | ts`)."
  [h tagged-outcomes deaths]
  (boolean (or (some seq tagged-outcomes)
               (some #(str/starts-with? (str %) (str h " | ")) deaths))))

(defn lane-lapse-ms
  "Ms the lane has been SILENT, or nil if live / too-new-to-judge. Expired lease -> the
   EXACT lapse; live (unexpired) lease -> nil (alive); NO lease at all -> spawned_at age
   (leases are GC'd, so their absence must not make a dead lane invisible — cross-ref the
   @agent identity's spawned_at instead). lease-exp / spawned-ms are epoch-ms or nil."
  [now lease-exp spawned-ms]
  (cond
    (and lease-exp (> lease-exp now)) nil
    lease-exp                         (- now lease-exp)
    spawned-ms                        (- now spawned-ms)
    :else                             nil))

(defn reap-lane?
  "Terminal verdict. Reap iff the lane's OWN outcome is empty (not already terminal), it
   is NOT resolved elsewhere (@run outcome / agent_death), and its silence lapse has
   reached LANE-STALE-MS."
  [now lane-outcome resolved? lease-exp spawned-ms]
  (and (empty? lane-outcome)
       (not resolved?)
       (let [lp (lane-lapse-ms now lease-exp spawned-ms)]
         (boolean (and lp (>= lp LANE-STALE-MS))))))

(defn orphaned-driver-subjects
  "Thread subjects whose driver ref names the reaped lane handle. Input rows are
   [thread driver-ref]; keeping this pure makes the exact-ref cleanup testable."
  [handle driver-pairs]
  (->> driver-pairs
       (keep (fn [[thread driver]] (when (= driver (str "@" handle)) thread)))
       distinct
       vec))

(defn sdk-agent-mint-ms
  "Mint time encoded by current `sdk-<fragment>-<base36-ms>-<uuid>` IDs, or nil.
   Legacy short IDs and implausibly old/malformed values deliberately fail safe."
  [handle]
  (when (and (string? handle) (str/starts-with? handle "sdk-")
             (re-find uuid-suffix handle))
    (let [without-uuid (str/replace handle uuid-suffix "")
          separator (.lastIndexOf without-uuid "-")]
      (when (pos? separator)
        (try
          (let [mint (Long/parseLong (subs without-uuid (inc separator)) 36)]
            (when (>= mint SDK-AGENT-ID-EPOCH-FLOOR-MS) mint))
          (catch Throwable _ nil))))))

(defn orphaned-unpublished-driver-pairs
  "Exact [thread driver-ref] pairs for timestamped SDK claims that never
   published a kind=lane identity and have aged to the normal lane reap bar.
   Exact pairs let the coordinator retract only the observed owner if state
   changes between the scan and write."
  [now known-lane-handles driver-pairs]
  (let [known (set known-lane-handles)]
    (->> driver-pairs
         (keep (fn [[thread driver-ref :as pair]]
                 (when (and (string? driver-ref) (str/starts-with? driver-ref "@"))
                   (let [handle (subs driver-ref 1)
                         mint (sdk-agent-mint-ms handle)]
                     (when (and mint
                                (not (contains? known handle))
                                (>= (- now mint) LANE-STALE-MS))
                       pair)))))
         distinct
         vec)))
