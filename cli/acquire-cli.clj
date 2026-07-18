;; acquire-cli.clj <port> {claim|verify|acquire|release|status} <thread> [holder]
;; Atomic work acquisition WITHOUT a lease (thread 019f100f-eefe). The @lease:<thread>
;; work-acquire lease is DELETED: driving a thread is GRAPH-INTERNAL, so it collapses onto
;; DECLARED-SINGLE — `driver` is a single-valued cardinality fact, and the engine's own
;; per-(subject,predicate) base-version reject IS the mutual exclusion. Two agents racing
;; to drive the SAME thread both pass the empty-group base (0); the writer serialized first
;; wins, the second's now-stale base is rejected (:conflict). No @lease:, no epoch/ttl — a
;; stuck driver is force-released explicitly or retracted by the lane-liveness reaper.
;; acquire-lease!/lease-cli survive ONLY for EXTERNAL resources (build dir / external API),
;; never a graph-internal subject like @lease:<thread>.
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str])

;; shared coord substrate (Foundation Part B): send-op lives once in cli/coord.clj.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(def send-op north.coord/send-op)

(defn- driver-of [port thread]
  (:value (send-op port {:op :resolved :te thread :p "driver"})))

(defn- thread-exists? [port thread]
  (some? (:value (send-op port {:op :resolved :te thread :p "title"}))))

(defn- thread-subject [thread]
  (let [value (when (string? thread) thread)
        bare (some-> value (str/replace-first #"^@" ""))]
    (when (and value
               (= value (str/trim value))
               (<= (count bare) 512)
               (re-matches #"[A-Za-z0-9][A-Za-z0-9._:-]*" bare))
      (str "@" bare))))

(defn- release-driver [port thread me]
  ;; Capture the global version BEFORE checking ownership, then retract only
  ;; against that exact snapshot. If any writer moves the graph between those
  ;; reads and the retract, retry the whole observation. This prevents a stale
  ;; releaser from clearing a successor installed during the read/retract gap.
  (loop [remaining 8]
    (let [base (:version (send-op port {:op :version}))]
      (when-not (integer? base)
        (throw (ex-info "coordinator version unavailable" {})))
      (let [cur (driver-of port thread)]
        (if (not= cur me)
          {:state :noop :driver cur}
          (let [result (send-op port {:op :retract
                                      :te thread :p "driver" :r me :base base})]
            (cond
              (nil? (:reject result)) {:state :released}
              (and (= :conflict (:reject result)) (> remaining 1))
              (recur (dec remaining))
              :else {:state :failed :reject (:reject result)})))))))

(let [[ps verb & args] *command-line-args*
      port (Integer/parseInt ps)
      raw-thread (first args)
      canonical-thread (thread-subject raw-thread)
      args (if canonical-thread (cons canonical-thread (rest args)) args)
      _ (when (and (#{"claim" "verify" "acquire" "release" "status"} verb)
                   (nil? canonical-thread))
          (binding [*out* *err*]
            (println "invalid thread id: expected a bare or single-@ ASCII identifier"))
          (System/exit 2))]
  (case verb
    "claim"                              ; <thread> <holder> — fail if ANY driver exists
    (let [[thread holder] args
          me  (str "@" holder)
          cur (driver-of port thread)]
      (cond
        (not (thread-exists? port thread))
        (do (println (format "DENIED %s — thread does not exist" thread)) (System/exit 4))

        (some? cur)
        (do (println (format "DENIED %s — already driven" thread)) (System/exit 3))

        :else
        (let [r (send-op port {:op :assert :te thread :p "driver" :r me :base 0})]
          (if (:reject r)
            (do (println (format "DENIED %s — lost the race" thread)) (System/exit 3))
            (println (format "CLAIMED %s by %s" thread holder))))))

    "verify"                             ; <thread> <holder> — MCP pre-claim handoff
    (let [[thread holder] args
          me  (str "@" holder)]
      (if (= me (driver-of port thread))
        (println (format "VERIFIED %s by %s" thread holder))
        (do (println (format "DENIED %s — driver handoff mismatch" thread)) (System/exit 3))))

    "acquire"                            ; <thread> <holder> — declared-single driver fact
    (let [[thread holder] args
          me  (str "@" holder)
          cur (driver-of port thread)]
      (cond
        (= cur me)                       ; already mine — idempotent re-drive, no write
        (println (format "ACQUIRED %s by %s (already held)" thread holder))

        (some? cur)                      ; driven by someone else — read-check denial
        (do (println (format "DENIED %s — driven by %s" thread cur)) (System/exit 1))

        :else                            ; undriven: assert with the empty-group base (0).
        ;; Concurrent racers both pass base 0; the engine commits the first and rejects the
        ;; second (bv > 0). The OCC reject IS the lock — no lease.
        (let [r (send-op port {:op :assert :te thread :p "driver" :r me :base 0})]
          (if (:reject r)
            (do (println (format "DENIED %s — lost the race (driver=%s)" thread (driver-of port thread)))
                (System/exit 1))
            (println (format "ACQUIRED %s by %s" thread holder))))))

    "release"                            ; <thread> <holder> — only the live driver may release
    (let [[thread holder] args
          me  (str "@" holder)]
      (try
        (let [{:keys [state driver]} (release-driver port thread me)]
          (case state
            :released (println (format "released %s by %s" thread holder))
            :noop (println (format "noop %s — not driven by %s (driver=%s)"
                                   thread holder (or driver "(none)")))
            (do
              (binding [*out* *err*]
                (println (format "DENIED %s — safe release could not commit" thread)))
              (System/exit 5))))
        (catch Exception _
          (binding [*out* *err*]
            (println (format "DENIED %s — safe release unavailable" thread)))
          (System/exit 5))))

    "status"                             ; <thread> — who drives it (coexist-elected single driver)
    (let [[thread] args]
      (println (format "%s driver=%s" thread (or (driver-of port thread) "(none)"))))

    (do (println "usage: acquire-cli.clj <port> {claim|verify|acquire|release|status} <thread> [holder]") (System/exit 2))))
