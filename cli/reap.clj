(ns north.reap
  (:require [clojure.java.io :as io]
            [clojure.string :as str]))

(load-file (str (.getParent (io/file *file*)) "/terminal-projection.clj"))

(def LANE-STALE-MS (* 30 60 1000))

(def CONCERN-STALE-MS (* 24 60 60 1000))

(def SDK-AGENT-ID-EPOCH-FLOOR-MS (.toEpochMilli (java.time.Instant/parse "2026-07-01T00:00:00Z")))

(def uuid-suffix #"-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")

(defn lane-resolved?
  "RESOLVED (never reap) from committed execution state: the lane's valid\n  terminal projection or the lane's latest valid committed run. A death report\n  is only a notification receipt; it cannot substitute for materializing a\n  committed terminal. This boolean is true only for resolved truth; destructive\n  callers use lane-resolution directly so indeterminate evidence blocks reap." [h lane-facts run-entries]
  (north.terminal-projection/lane-resolved? h lane-facts run-entries))

(defn ^Boolean death-reported?
  "Whether @swarm already received the lane's exact death notification. This\n  suppresses duplicate pings only; it deliberately does not resolve liveness." [^String h deaths]
  (boolean (some (fn [__north_anon_1] (str/starts-with? (str __north_anon_1) (str h " | "))) deaths)))

(defn lane-lapse-ms
  "Ms the lane has been SILENT, or nil if live / too-new-to-judge. Expired lease -> the\n   EXACT lapse; live (unexpired) lease -> nil (alive); NO lease at all -> spawned_at age\n   (leases are GC'd, so their absence must not make a dead lane invisible — cross-ref the\n   @agent identity's spawned_at instead). lease-exp / spawned-ms are epoch-ms or nil." [now lease-exp spawned-ms]
  (cond
  (and lease-exp (> lease-exp now)) nil
  lease-exp (- now lease-exp)
  spawned-ms (- now spawned-ms)
  :else nil))

(defn ^Boolean reap-lane?
  "Terminal verdict. Reap iff no committed lane/run terminal resolves the lane\n  and its silence lapse has reached LANE-STALE-MS." [now ^Boolean resolved? lease-exp spawned-ms]
  (and (not resolved?) (let [lp (lane-lapse-ms now lease-exp spawned-ms)]
  (boolean (and lp (>= lp LANE-STALE-MS))))))

(defn orphaned-driver-subjects
  "Thread subjects whose driver ref names the reaped lane handle. Input rows are\n   [thread driver-ref]; keeping this pure makes the exact-ref cleanup testable." [handle driver-pairs]
  (->> driver-pairs (keep (fn [[thread driver]] (if (= driver (str "@" handle)) (do
  thread)))) distinct vec))

(defn sdk-agent-mint-ms
  "Mint time encoded by current `sdk-<fragment>-<base36-ms>-<uuid>` IDs, or nil.\n   Legacy short IDs and implausibly old/malformed values deliberately fail safe." [handle]
  (if (and (string? handle) (str/starts-with? handle "sdk-") (re-find uuid-suffix handle)) (do
  (let [^String without-uuid (str/replace handle uuid-suffix "")
   separator (.lastIndexOf without-uuid "-")]
  (if (pos? separator) (do
  (try
  (let [mint (Long/parseLong (subs without-uuid (inc separator)) 36)]
  (if (>= mint SDK-AGENT-ID-EPOCH-FLOOR-MS) (do
  mint)))
  (catch Throwable _
    nil))))))))

(defn orphaned-unpublished-driver-pairs
  "Exact [thread driver-ref] pairs for timestamped SDK claims that never\n   published a kind=lane identity and have aged to the normal lane reap bar.\n   Exact pairs let the coordinator retract only the observed owner if state\n   changes between the scan and write." [now known-lane-handles driver-pairs]
  (let [known (set known-lane-handles)]
  (->> driver-pairs (keep (fn [[thread driver-ref :as pair]] (if (and (string? driver-ref) (str/starts-with? driver-ref "@")) (do
  (let [handle (subs driver-ref 1)
   mint (sdk-agent-mint-ms handle)]
  (if (and mint (not (contains? known handle)) (>= (- now mint) LANE-STALE-MS)) (do
  pair))))))) distinct vec)))
