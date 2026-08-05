;; acquire-cli.clj <port> {claim|verify|acquire|release|status} <thread> [holder]
;; Atomic work acquisition without a lease. Driving a thread is graph-internal:
;; the `driver` assertion commits against the exact version that observed no
;; driver. Two racers can both read empty, but only one expected-version
;; transaction commits; the other replans and sees the winner.
(require '[clojure.java.io :as io] '[clojure.string :as str])

;; Shared coordination substrate: typed FRAMRPC reads and transactions live once.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(defn- driver-of [port thread]
  (north.coord/resolved port thread "driver"))

(defn- thread-exists? [port thread]
  (some? (north.coord/resolved port thread "title")))

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
    (let [base (north.coord/cur-ver port)]
      (when-not (integer? base)
        (throw (ex-info "Fram version unavailable" {})))
      (let [cur (driver-of port thread)]
        (if (not= cur me)
          {:state :noop :driver cur}
          (let [result (north.coord/transact!
                        port [{:op :retract :subject thread
                               :predicate "driver" :value me}]
                        {:expected-version base})]
            (cond
              (nil? (:reject result)) {:state :released}
              (and (= :conflict (:reject result)) (> remaining 1))
              (recur (dec remaining))
              :else {:state :failed :reject (:reject result)})))))))

(defn- claim-driver! [port thread me require-thread?]
  (try
    (let [result
          (north.coord/assert-after-read!
           port thread "driver" me
           (fn []
             (when (and require-thread? (not (thread-exists? port thread)))
               (throw (ex-info "thread does not exist" {:driver-state :missing})))
             (when-let [current (driver-of port thread)]
               (throw (ex-info "thread already has a driver"
                               {:driver-state (if (= current me) :already-mine :held)
                                :driver current})))))]
      (if (:reject result)
        {:state :failed :reject (:reject result)}
        {:state :acquired}))
    (catch clojure.lang.ExceptionInfo error
      (if-let [state (:driver-state (ex-data error))]
        {:state state :driver (:driver (ex-data error))}
        (throw error)))))

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
        (let [{:keys [state]} (claim-driver! port thread me true)]
          (if (= :acquired state)
            (println (format "CLAIMED %s by %s" thread holder))
            (do (println (format "DENIED %s — lost the race" thread))
                (System/exit 3))))))

    "verify"                             ; <thread> <holder> — MCP pre-claim handoff
    (let [[thread holder] args
          me  (str "@" holder)
          cur (driver-of port thread)]
      (cond
        (= me cur)
        (println (format "VERIFIED %s by %s" thread holder))

        (nil? cur)
        (do
          (println (format "DENIED %s — preclaimed driver is absent" thread))
          (System/exit 6))

        :else
        (do
          (println (format "DENIED %s — preclaimed driver is %s, expected %s"
                           thread cur me))
          (System/exit 7))))

    "acquire"                            ; <thread> <holder> — declared-single driver fact
    (let [[thread holder] args
          me  (str "@" holder)
          cur (driver-of port thread)]
      (cond
        (= cur me)                       ; already mine — idempotent re-drive, no write
        (println (format "ACQUIRED %s by %s (already held)" thread holder))

        (some? cur)                      ; driven by someone else — read-check denial
        (do (println (format "DENIED %s — driven by %s" thread cur)) (System/exit 1))

        :else
        (let [{:keys [state driver]} (claim-driver! port thread me false)]
          (case state
            :acquired (println (format "ACQUIRED %s by %s" thread holder))
            :already-mine (println (format "ACQUIRED %s by %s (already held)" thread holder))
            (do (println (format "DENIED %s — lost the race (driver=%s)"
                                 thread (or driver (driver-of port thread))))
                (System/exit 1))))))

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
