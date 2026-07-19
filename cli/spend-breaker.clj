;; spend-breaker.clj — the spend-guard CIRCUIT BREAKER + reactor burn/kill/reap
;; logic (build-order step 3). Loaded (not required) the same way north-reactor.clj
;; loads coord.clj/reap.clj: the CALLER loads coord.clj FIRST, then this file, so
;; `north.coord/*` resolves. Two callers load it — north-reactor.clj (the sweep
;; that WRITES trips + kills breached lanes + settles dead lanes) and spend-cli.clj
;; (the reserve precondition + the human `reset-breaker` ceremony). north-reactor.clj
;; runs `-main` on load, so it can NEVER be load-file'd by a test; every daemon-
;; touching sweep primitive therefore lives HERE, in a load-safe lib, and the
;; reactor is a thin caller. spend-breaker-test.clj drives these directly.
;;
;; DESIGN (spend-guard-design-2026-07-19.md §4): ONE global breaker
;; `@spend-breaker:global`. Trip = assert `tripped <ts>` + `trip_reason "..."`.
;; Tripped ⇒ every API-billed reservation refuses (~1ms coord read, checked in
;; spend-cli reserve!). Human-only reset. Trip conditions computed here:
;;   1. trailing-window ledger accrual > burn_limit × window (the burn-rate sweep)
;;   2. ledger fold failure (a corrupt counter) — fail-closed
;;   3. reconciliation divergence — STUB SEAM for step 5 (named, not built)
;;
;; WINDOW RESOLUTION (documented honestly): the accrual metric is the cumulative
;; (reserved+settled) micro-USD charge on the target's CURRENT-month period. The
;; reactor samples it every sweep (5-min cadence) into a small on-disk ring
;; (~/.cache/north, the reactor's existing state home — no new fact-log churn, no
;; new daemon), keyed by port + target. Burn = Δ(cumulative) over the trailing
;; WINDOW-MS; rate = Δ · hour / elapsed. Trip when rate > burn_limit_per_hour once
;; at least MIN-ELAPSED-MS of history spans the window (below that: insufficient
;; data, no trip — a ≤15-min blind spot backstopped by admission-per-spawn + the
;; per-turn parent check of step 4). Two safe-direction approximations, both
;; UNDER-stating burn (never a false spend-under-cap): (a) reservations settle
;; DOWN, so a settled run shrinks the cumulative and a quiet window reads
;; negative; (b) at a UTC month boundary the period counter resets and cumulative
;; drops. Both fail toward NOT tripping on stale history, never toward masking a
;; live runaway (a runaway makes cumulative climb fast regardless).
(ns north.spend-breaker
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

;; Sibling-relative paths captured at LOAD time (*file* = this file while loading,
;; the coord.clj precedent). Used to shell `spend-cli.clj settle` for the FULL
;; reservation settlement of killed/dead lanes — the design's "settle via spend-cli
;; settle" wording, and the only way the reactor (which never load-file's spend-cli)
;; reaches the settlement primitive.
(def ^:private here (.getParent (io/file *file*)))
(def ^:private spend-cli-path (str here "/spend-cli.clj"))

(def BREAKER "spend-breaker:global")
(def WINDOW-MS      (* 60 60 1000))      ; trailing burn window = 1h (burn_limit is per-hour)
(def MIN-ELAPSED-MS (* 15 60 1000))      ; need ≥15min of spanning history before a rate is meaningful
(def RING-KEEP-MS   (* 2 WINDOW-MS))     ; retain ~2 windows of samples, drop older

;; --- money counters (mirror spend-cli's fail-closed reads; the reactor does not
;; --- load spend-cli, so the 3-line period math is duplicated deliberately) -----
(defn utc-month [] (.format (java.time.format.DateTimeFormatter/ofPattern "yyyy-MM")
                            (java.time.ZonedDateTime/now java.time.ZoneOffset/UTC)))
(defn period-id [target month] (str "spend-period:" target ":" month))

(defn counter
  "Live micro-USD counter value, 0 when absent. A non-integer live value is a
   CORRUPT ledger and throws — the sweep turns that into a breaker trip."
  [port subject pred]
  (let [v (north.coord/resolved port (str "@" subject) pred)]
    (cond (nil? v) 0
          (re-matches #"\d+" (str v)) (parse-long (str v))
          :else (throw (ex-info (str "corrupt counter " subject "/" pred " = " (pr-str v))
                                {:type :ledger-corruption :subject subject :pred pred})))))

(defn cumulative-micro
  "Cumulative (reserved+settled) charge on TARGET's current-month period."
  [port target]
  (let [period (period-id target (utc-month))]
    (+ (counter port period "reserved_microusd")
       (counter port period "settled_microusd"))))

(defn budget-targets
  "Every configured spend-budget target (kind=spend-budget)."
  [port]
  (->> (:ok (north.coord/send-op
             port {:op :query
                   :query {:find "b"
                           :rules [{:head {:rel "b" :args [{:var "b"}]}
                                    :body [{:rel "triple" :args [{:var "b"} "kind" "spend-budget"]}]}]}}))
       (map first)
       (map #(str/replace (str %) #"^@?spend-budget:" ""))
       distinct sort))

(defn burn-limit-micro [port target]
  (let [v (north.coord/resolved port (str "@spend-budget:" target) "burn_limit_microusd_per_hour")]
    (when (and v (re-matches #"\d+" (str v))) (parse-long (str v)))))

;; --- PURE burn verdict (unit-testable off in-memory samples) -----------------
;; SAMPLES: seq of {:ts <epoch-ms> :total <micro>}. NOW: epoch-ms. Returns
;; {:breached? bool :rate <micro/hr or nil> :delta :elapsed-ms :baseline-ts
;;  :reason (when breached)}. Baseline = the EARLIEST sample still inside the
;; trailing window (largest honest Δ); insufficient span => not breached.
(defn burn-verdict [samples now current-total burn-limit-per-hr]
  (let [in-window (->> samples (filter #(>= (:ts %) (- now WINDOW-MS))) (sort-by :ts))
        baseline  (first in-window)]
    (if (or (nil? baseline) (nil? burn-limit-per-hr))
      {:breached? false :reason nil}
      (let [elapsed (- now (:ts baseline))
            delta   (- current-total (:total baseline))]
        (if (< elapsed MIN-ELAPSED-MS)
          {:breached? false :elapsed-ms elapsed :delta delta :baseline-ts (:ts baseline)}
          (let [rate (long (/ (* delta 3600000.0) (max 1 elapsed)))]
            {:breached? (> rate burn-limit-per-hr)
             :rate rate :delta delta :elapsed-ms elapsed :baseline-ts (:ts baseline)
             :reason (when (> rate burn-limit-per-hr)
                       (format "burn-rate breach: %d micro-USD/hr over trailing %dmin > limit %d micro-USD/hr"
                               rate (long (/ elapsed 60000)) burn-limit-per-hr))}))))))

;; --- burn sample ring (on-disk, reactor's ~/.cache/north state home) ---------
(defn ring-file [port]
  (if-let [o (System/getenv "NORTH_SPEND_BURN_STATE")]
    (io/file o)
    (io/file (System/getenv "HOME") ".cache" "north" (str "spend-burn-" port ".json"))))

(defn read-ring
  "target -> [{:ts :total} ...]. Absent/torn/malformed => {} (best-effort measurement)."
  [port]
  (try
    (let [f (ring-file port)]
      (if (.isFile f)
        (reduce-kv (fn [m k v]
                     (assoc m (name k)
                            (keep (fn [s] (when (and (:ts s) (:total s))
                                            {:ts (long (:ts s)) :total (long (:total s))})) v)))
                   {} (json/parse-string (slurp f) true))
        {}))
    (catch Throwable _ {})))

(defn write-ring! [port ring]
  (try
    (let [f (ring-file port) dir (.getParentFile f)]
      (when dir (.mkdirs dir))
      (let [tmp (io/file (str (.getPath f) ".tmp"))]
        (spit tmp (json/generate-string ring))
        (java.nio.file.Files/move
         (.toPath tmp) (.toPath f)
         (into-array java.nio.file.CopyOption
                     [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                      java.nio.file.StandardCopyOption/REPLACE_EXISTING]))))
    (catch Throwable t
      (println (str "[sweep] burn-ring write failed: " (.getMessage t))))))

(defn sample-target!
  "Append a fresh cumulative sample for TARGET, prune to RING-KEEP-MS, persist,
   and return the updated in-window sample seq for this target. Corruption throws."
  [port target now]
  (let [total (cumulative-micro port target)
        ring  (read-ring port)
        kept  (->> (conj (vec (get ring target)) {:ts now :total total})
                   (filter #(>= (:ts %) (- now RING-KEEP-MS)))
                   (sort-by :ts) vec)]
    (write-ring! port (assoc ring target kept))
    kept))

;; --- breaker state (facts on @spend-breaker:global) --------------------------
(defn tripped? [port] (boolean (seq (str (north.coord/resolved port (str "@" BREAKER) "tripped")))))
(defn trip-reason [port] (str (north.coord/resolved port (str "@" BREAKER) "trip_reason")))

(defn trip!
  "Trip the global breaker (idempotent-ish: refreshes reason). Human-only reset."
  [port reason]
  (let [now (str (java.time.Instant/now))]
    (north.coord/put! port (str "@" BREAKER) "kind" "spend-breaker")
    (north.coord/put! port (str "@" BREAKER) "tripped" now)
    (north.coord/put! port (str "@" BREAKER) "trip_reason" (str reason))
    now))

(defn retract-tripped!
  "Clear the live trip facts (the reset ceremony's terminal write). Retracts the
   exact live values so a concurrent re-trip between read and retract is untouched."
  [port]
  (doseq [p ["tripped" "trip_reason"]]
    (when-let [v (north.coord/resolved port (str "@" BREAKER) p)]
      (when (seq (str v)) (north.coord/retract! port (str "@" BREAKER) p (str v))))))

;; --- live trip-condition re-evaluation (the reset ceremony's refusal gate) ----
;; Returns nil when NO trip condition currently holds, else a machine reason.
;; Corruption is always evaluable; burn needs ring history (absent => can't prove
;; it true => not a refusal, human judgment — documented in the ceremony).
(defn live-trip-condition [port now]
  (let [ring (read-ring port)]
    (some (fn [t]
            (try
              (let [v (burn-verdict (get ring t) now (cumulative-micro port t) (burn-limit-micro port t))]
                (when (:breached? v) (str t ": " (:reason v))))
              (catch clojure.lang.ExceptionInfo e
                (when (= :ledger-corruption (:type (ex-data e)))
                  (str t ": ledger corruption — " (.getMessage e))))))
          (budget-targets port))))

;; --- RECONCILIATION DIVERGENCE — STEP-5 SEAM (named, deliberately not built) --
;; Step 5 lands the SpendReconciliationAdapter (design §5): provider balance/usage
;; vs ledger, hourly reactor piggyback. When it detects a dangerous-direction
;; divergence it calls `(trip! port <reason>)` from that hourly gate. Nothing here
;; fetches provider state; this fn is the only hook and always returns nil now.
(defn reconciliation-divergence-reason
  "STUB (step 5): always nil. Reserved so the trip surface is single-sourced."
  [_port _now] nil)

;; ============================================================================
;; REACTOR SWEEP PRIMITIVES (called by north-reactor.clj sweep!, driven directly
;; by spend-breaker-test.clj). Each returns a small map/count and prints its act.
;; ============================================================================

(defn sweep-burn!
  "Sample every budget target's cumulative accrual, evaluate the burn verdict, and
   TRIP the global breaker on the first breach or ledger corruption. No-op writes
   under dry?. Already-tripped => still samples (keeps the ring warm) but does not
   re-trip. Returns {:tripped bool :reason <str|nil>}."
  [port dry?]
  (let [now (System/currentTimeMillis)
        already (tripped? port)]
    (loop [targets (budget-targets port)]
      (if-let [t (first targets)]
        (let [verdict (try
                        (let [samples (if dry? (get (read-ring port) t) (sample-target! port t now))]
                          (burn-verdict samples now (cumulative-micro port t) (burn-limit-micro port t)))
                        (catch clojure.lang.ExceptionInfo e
                          (if (= :ledger-corruption (:type (ex-data e)))
                            {:breached? true :reason (str "ledger corruption — " (.getMessage e))}
                            (throw e))))]
          (if (:breached? verdict)
            (let [reason (str t ": " (:reason verdict))]
              (when-not (or already dry?) (trip! port reason))
              (println (str "[sweep] " (if dry? "WOULD trip" (if already "breach (already tripped)" "TRIPPED"))
                            " breaker — " reason))
              {:tripped true :reason reason})
            (recur (rest targets))))
        {:tripped false :reason nil}))))

;; --- open spend-lane reservations (the sweep-kill + reaper linkage seam) ------
;; @spend-lane:<id> {kind spend-lane, lane <handle>, pid <int>, target <t>,
;; period <p>, reserved_microusd <micro>}. OPEN = kind=spend-lane with NO
;; settled_at (settled_at is the idempotency guard; kill also stamps killed_at).
;; STEP-4 SEAM: the per-turn parent adapter WRITES these facts at spawn (it owns
;; the child pid + the reservation it made). Step 3 provides the schema + both
;; consumers (sweep-kill, reaper settle) + the test writer.
(defn open-spend-lanes [port]
  (->> (:ok (north.coord/send-op
             port {:op :query
                   :query {:find "row"
                           :strata [[{:head {:rel "settled" :args [{:var "e"}]}
                                      :body [{:rel "triple" :args [{:var "e"} "settled_at" {:var "s"}]}]}]
                                    [{:head {:rel "row" :args [{:var "e"} {:var "pid"} {:var "tgt"} {:var "per"} {:var "res"}]}
                                      :body [{:rel "triple" :args [{:var "e"} "kind" "spend-lane"]}
                                             {:rel "triple" :args [{:var "e"} "pid" {:var "pid"}]}
                                             {:rel "triple" :args [{:var "e"} "target" {:var "tgt"}]}
                                             {:rel "triple" :args [{:var "e"} "period" {:var "per"}]}
                                             {:rel "triple" :args [{:var "e"} "reserved_microusd" {:var "res"}]}
                                             {:rel "settled" :args [{:var "e"}] :neg true}]}]]}}))
       (map (fn [[e pid tgt per res]]
              {:id (str e) :pid (parse-long (str pid)) :target (str tgt)
               :period (str per) :reserved (parse-long (str res))
               :lane (north.coord/resolved port (str e) "lane")}))))

(defn pid-alive? [pid]
  (boolean (and pid (some-> (java.lang.ProcessHandle/of (long pid)) (.orElse nil) (.isAlive)))))

(defn active-envelope-pids
  "Live pids from the resource-envelope accounting active leases — the INDEPENDENT
   proof (alongside the @spend-lane fact) that a pid is a currently-managed North
   lane. Never kill a pid absent from BOTH surfaces. Best-effort read; a
   missing/torn accounting file yields the empty set (kill nothing)."
  []
  (try
    (let [path (or (System/getenv "NORTH_ENVELOPE_ACCOUNTING")
                   (str (System/getenv "HOME") "/.local/state/north/resource-envelope-accounting.json"))
          f (io/file path)]
      (if (.isFile f)
        (->> (:scopes (json/parse-string (slurp f) true))
             vals
             (mapcat (fn [scope] (vals (:active scope))))
             (keep :pid) (map long) set)
        #{}))
    (catch Throwable _ #{})))

;; babashka.process is required by the reactor + test entry scripts; reference it
;; through requiring-resolve so this lib needs no top-level require of it.
(defn proc-shell-settle [port target period reserved]
  ((requiring-resolve 'babashka.process/shell)
   {:out :string :err :string :continue true
    :extra-env {"NORTH_PORT" (str port) "FRAM_LOG" (north.coord/expected-log)}}
   "bb" spend-cli-path "settle" target
   "--period" period "--reserved-microusd" (str reserved) "--status" "unknown"))

(defn settle-lane-full!
  "Settle one open spend-lane at its FULL reservation (status unknown) by shelling
   `spend-cli.clj settle`, then stamp settled_at. Idempotent: skips if already
   stamped. WHY: the exact micro-USD settle math + CAS lives in spend-cli; the
   reactor never load-file's it, so the settlement primitive is reached by shell —
   the design's 'settle via spend-cli settle'."
  [port {:keys [id target period reserved]} dry? note]
  (if (north.coord/resolved port id "settled_at")
    :already-settled
    (do
      (when-not dry?
        (let [r (try
                  (proc-shell-settle port target period reserved)
                  (catch Throwable t {:exit 1 :err (.getMessage t)}))]
          (when-not (zero? (:exit r 1))
            (println (str "[sweep] spend-lane settle shell exit=" (:exit r) " " (:err r))))
          (north.coord/put! port id "settled_at" (str (java.time.Instant/now)))))
      (println (str "[sweep] " (if dry? "WOULD settle" "settled") " open reservation " id
                    " target " target " $" (format "%.6f" (/ reserved 1e6)) " full (" note ")"))
      :settled)))

(defn sigkill-group!
  "SIGKILL the process GROUP of PID (negative-pid kill), then force the process
   itself. Best-effort; both paths are guarded. Kills the group so a wedged lane's
   provider-query children die with it."
  [pid]
  (try ((requiring-resolve 'babashka.process/shell)
        {:out :string :err :string :continue true} "kill" "-KILL" (str "-" pid))
       (catch Throwable _ nil))
  (try (some-> (java.lang.ProcessHandle/of (long pid)) (.orElse nil) (.destroyForcibly))
       (catch Throwable _ nil)))

(defn sweep-kill!
  "When the global breaker is TRIPPED, SIGKILL every open spend-lane whose pid is a
   VERIFIED live North-managed lane (pid alive AND present in the envelope-accounting
   active leases), settle its reservation at full, stamp killed_at, and append the
   killed lane to the trip_reason. Never kills a pid absent from the envelope leases
   (recycled-pid safety, the liveness-reaper precedent). Returns count killed."
  [port dry?]
  (if-not (tripped? port)
    0
    (let [env-pids (active-envelope-pids)
          lanes (open-spend-lanes port)
          killed (atom 0)]
      (doseq [{:keys [id pid target lane] :as l} lanes]
        (cond
          (not (pid-alive? pid))
          nil                                                    ; dead already — reaper settles it
          (not (contains? env-pids pid))
          (println (str "[sweep] REFUSE kill " id " pid " pid
                        " — not a verified envelope-lease lane (recycled-pid safety)"))
          :else
          (do
            (when-not dry?
              (sigkill-group! pid)
              (settle-lane-full! port l false (str "sweep-kill lane " lane))
              (north.coord/put! port id "killed_at" (str (java.time.Instant/now)))
              ;; note the lane on a DISTINCT multi predicate — never mutate the
              ;; single trip_reason the reset ceremony must quote back verbatim.
              (north.coord/append! port (str "@" BREAKER) "trip_note"
                                   (str "sweep-killed lane " lane " (pid " pid ", target " target ")")))
            (swap! killed inc)
            (println (str "[sweep] " (if dry? "WOULD SIGKILL" "SIGKILLED") " breached lane " id
                          " pid " pid " (lane " lane ", target " target ") + settled full")))))
      @killed)))

(defn reap-settle-lane-reservations!
  "Reaper hook: when a lane HANDLE is judged terminal-dead (died-unreported), settle
   its open spend reservations at FULL (status unknown — design §1: a dead lane's
   reservation only ever stands, never settles cheaper). Idempotent via settled_at,
   so a re-sweep never double-settles. Returns count settled."
  [port handle dry?]
  (let [lanes (->> (open-spend-lanes port) (filter #(= (str "@" handle) (str (:lane %)))))]
    (doseq [l lanes] (settle-lane-full! port l dry? (str "reaper dead lane " handle)))
    (count lanes)))
