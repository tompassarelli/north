#!/usr/bin/env bb
;; ============================================================================
;; north-reactor.clj <port> [debounce-ms] — COORDINATOR AUTO-EXPORT.
;;
;; The threads/*.md files are a PROJECTION of the fact log, but freshness was
;; MANUAL (`north export`/`heal`) and forbidden during concurrent work — so every
;; write that didn't self-render (`fram tell`, the MCP tell tool, and the CLI
;; spokes concern/presence/msg/lease that write via the daemon socket) left the
;; file lagging the log. That lag ACCUMULATED (348 stale facts in one day) until
;; a human ran `heal`, and doctor screamed DEGRADED at every boot for the benign
;; drift. This reactor kills the class at the root: it treats the coordinator's
;; commit stream as the trigger and re-projects touched threads automatically, so
;; files NEVER lag the log and no client ever has to remember to render.
;;
;; HOW: the daemon already firehoses every commit to :subscribe subscribers
;; (coord_daemon notify-subs!). We subscribe (nil filter = firehose), coalesce
;; a burst of commits behind a short debounce, then shell the SAME `north heal` a
;; human runs — byte-identical to `north export` (both render via fram.export/
;; thread-md) and FAIL-CLOSED on genuine hand edits (a human decides those). heal
;; self-scopes: it re-renders ONLY the files that diverge from the log, so a burst
;; of edits costs one flush, and an idle stream costs nothing.
;;
;; This needs NO change to the coordinator (fram) — it rides the existing
;; :subscribe seam. It is a standalone sidecar: start it alongside the daemon.
;;   FRAM_LOG / FRAM_THREADS / FRAM_PORT select the target state (same env
;;   `north`/`fram-up` read); heal inherits them from our env.
;;
;;   bb cli/north-reactor.clj 7977            # firehose :7977, 400ms debounce
;;   bb cli/north-reactor.clj 7977 250        # tighter debounce
;;   north reactor &                          # via the bin/north wrapper (bg task)
;; ============================================================================
(require '[cheshire.core :as json]
         '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[babashka.process :as proc])

;; shared coord substrate (write verbs + renewable-lease liveness) — the sweep judges
;; owner death by the SAME lease rule presence-cli/concern-cli use, and writes its
;; verdict through the coordinator (auditable facts, never a mutated cell).
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
;; PURE reap decisions (verdict off in-memory facts) — split out so reap_test.clj can
;; drive the join/lapse/verdict logic with no live daemon. Sibling of coord.clj.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/reap.clj"))
;; Shared Git-derived worktree read model. `north worktrees` renders it; the
;; janitor's unregistered sweep decides off it. Must load before the janitor.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file")))
                "/worktree-census.clj"))
;; Side-effect-free managed-worktree janitor. It is deliberately owned by this
;; reactor: `sweep-once` and the five-minute loop execute the same function with
;; this file's canonical full terminal/run resolver.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file")))
                "/worktree-janitor.clj"))
;; DURABLE last-sweep heartbeat — the reactor's liveness trace `north doctor` reads.
;; Shared writer/reader lib (doctor loads the same file); we stamp it at each sweep.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/reactor-heartbeat.clj"))
;; Rebuild QUEUE read/write path — the reactor is the window OWNER, and it drives
;; the same verbs `north rebuild` exposes rather than writing request facts itself.
(System/setProperty "north.rebuild-request-cli.lib" "1")
(load-file (str (.getParent (io/file (System/getProperty "babashka.file")))
                "/rebuild-request-cli.clj"))
;; Spend-guard breaker + burn/kill/reaper-settle primitives (step 3). Loaded AFTER
;; coord so its north.coord/* references resolve. The reactor is the ONLY place the
;; burn-rate trip is computed + written and the only terminal for dead-lane spend.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/spend-breaker.clj"))

;; `sweep-once` verb: one-shot reap for testing. `bb cli/north-reactor.clj sweep-once
;; [--dry-run] [--repo <repo>]`. Otherwise argv = [port debounce] for the reactor loop.
(def raw-args   *command-line-args*)
(def sweep-verb? (= (first raw-args) "sweep-once"))
(def s-args     (if sweep-verb? (vec (rest raw-args)) (vec raw-args)))
(def sweep-flags (set (filter #(str/starts-with? % "--") s-args)))
(def dry-run?   (contains? sweep-flags "--dry-run"))
(def sweep-repo (when sweep-verb?
                  (let [pos (remove #(str/starts-with? % "--") s-args)
                        i (.indexOf (vec s-args) "--repo")]
                    (cond (>= i 0) (get s-args (inc i))
                          (seq pos) (first pos)
                          :else nil))))
(def port (Integer/parseInt (or (when-not sweep-verb? (first s-args))
                                 (System/getenv "FRAM_PORT") "7977")))
(def debounce-ms (Integer/parseInt (or (when-not sweep-verb? (second s-args)) "400")))

;; A bounded sweep owns every subprocess it starts. The outer deadline and the
;; phase runner synchronize on this context so cancellation wins atomically:
;; after it flips, no later phase can start and no new child can register.
(def ^:dynamic *sweep-runtime* nil)
(def CHILD-TERM-GRACE-MS 250)
(def CHILD-KILL-GRACE-MS 2000)

(defn deadline-remaining-ms [deadline-ns]
  (max 0 (long (/ (- deadline-ns (System/nanoTime)) 1000000))))

(defn child-handles [child]
  (let [root (.toHandle ^java.lang.Process (:proc child))]
    (with-open [descendants (.descendants root)]
      (vec (cons root (iterator-seq (.iterator descendants)))))))

(defn handle-alive? [^java.lang.ProcessHandle handle]
  (try (.isAlive handle) (catch Throwable _ false)))

(defn await-handles-gone! [handles timeout-ms]
  (let [deadline (+ (System/nanoTime) (* 1000000 timeout-ms))]
    (loop []
      (let [alive (vec (filter handle-alive? handles))]
        (cond
          (empty? alive) []
          (>= (System/nanoTime) deadline) alive
          :else
          (do
            (try (Thread/sleep 10) (catch InterruptedException _ nil))
            (recur)))))))

(defn terminate-child-tree!
  "TERM, then KILL, the snapshotted process tree and wait for every handle to
   disappear. A terminal sweep result must never race a surviving child."
  [child]
  (let [initial (child-handles child)]
    (doseq [^java.lang.ProcessHandle handle initial]
      (when (handle-alive? handle)
        (try (.destroy handle) (catch Throwable _ nil))))
    (let [after-term (await-handles-gone! initial CHILD-TERM-GRACE-MS)
          ;; A TERM-resistant parent may have forked after the first snapshot.
          expanded (vec (distinct
                         (concat initial
                                 (try (child-handles child)
                                      (catch Throwable _ [])))))]
      (when (seq after-term)
        (doseq [^java.lang.ProcessHandle handle expanded]
          (when (handle-alive? handle)
            (try (.destroyForcibly handle) (catch Throwable _ nil)))))
      (let [survivors (await-handles-gone! expanded CHILD-KILL-GRACE-MS)]
        (when (seq survivors)
          (throw
           (ex-info "reactor child process tree survived cancellation"
                    {:type :sweep-child-cleanup-failed
                     :pids (mapv #(.pid ^java.lang.ProcessHandle %) survivors)})))
        {:handles (count expanded) :terminated (count expanded)}))))

(defn sweep-deadline-ex [stage]
  (ex-info "aggregate reactor sweep deadline reached"
           {:type :sweep-deadline :stage stage}))

(defn run-sweep-stage! [stage f]
  (when *sweep-runtime*
    (locking *sweep-runtime*
      (when (or @(:cancelled? *sweep-runtime*)
                (zero? (deadline-remaining-ms (:deadline-ns *sweep-runtime*))))
        (throw (sweep-deadline-ex stage)))
      (reset! (:stage *sweep-runtime*) stage)))
  (f))

(defn start-sweep-child!
  "Start and register a reactor-owned child under the same lock that publishes
   aggregate cancellation. Outside sweep-once, retain the normal bounded child."
  [label options & command]
  (if-not *sweep-runtime*
    (apply proc/process options command)
    (locking *sweep-runtime*
      (when (or @(:cancelled? *sweep-runtime*)
                (zero? (deadline-remaining-ms (:deadline-ns *sweep-runtime*))))
        (throw (sweep-deadline-ex label)))
      (let [child (apply proc/process options command)]
        (swap! (:children *sweep-runtime*) assoc child {:label label})
        child))))

(defn unregister-sweep-child! [child]
  (when *sweep-runtime*
    (swap! (:children *sweep-runtime*) dissoc child)))

(defn await-sweep-child!
  "Wait for one registered child. Timeout and interruption both synchronously
   drain its process tree before returning or propagating."
  [child timeout-ms]
  (try
    (let [result (deref child timeout-ms ::timeout)]
      (if (= ::timeout result)
        (let [cleanup (terminate-child-tree! child)]
          (unregister-sweep-child! child)
          {:status :timeout :cleanup cleanup})
        (do
          (unregister-sweep-child! child)
          {:status :completed :result result})))
    (catch InterruptedException interrupted
      (try (terminate-child-tree! child) (catch Throwable _ nil))
      (unregister-sweep-child! child)
      (throw interrupted))))

(defn cancel-sweep-runtime! [runtime]
  (let [registered
        (locking runtime
          (reset! (:cancelled? runtime) true)
          (vec @(:children runtime)))
        cleanups
        (mapv
         (fn [[child {:keys [label]}]]
           (let [cleanup (terminate-child-tree! child)]
             (swap! (:children runtime) dissoc child)
             {:label label :cleanup cleanup}))
         registered)]
    {:registered (count registered)
     :terminated (count cleanups)
     :surviving (count @(:children runtime))}))

;; ---- LIVENESS-DERIVED REAPING (design 019f4418) -----------------------------
;; Two terminal verdicts the reactor writes on its cadence (or via sweep-once):
;;   1. a `building` concern whose owner has been LAPSED >24h  -> reached=abandoned-stale
;;      (likely-to-land is EXEMPT — it survives owner death as an ORPHANED retained
;;      recovery candidate, not evidence a handoff procedure occurred).
;;   2. a kind=lane agent LAPSED >30min with no COMMITTED lane/run terminal
;;      -> a committed process=died-unreported, delivery=blocked terminal; if it
;;      carries a coordinator/supervisor, ping it.
;; Every write goes through :7977 (coord/append!/put!), so the audit trail is a fact.
(def CONCERN-STALE-MS north.reap/CONCERN-STALE-MS)   ; 24h
(def LANE-STALE-MS    north.reap/LANE-STALE-MS)      ; 30min
(def CONCERN-TRANSITION-TIMEOUT-MS 45000)
(def concern-transition-cli
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile (io/file "concern-cli.clj") .getPath))

(defn q-col [body]
  (->> (:ok (north.coord/send-op port {:op :query
              :query {:find "e" :rules [{:head {:rel "e" :args [{:var "e"}]} :body body}]}}))
       (map first)))

(defn strip-sigil [s pfx] (if (str/starts-with? s pfx) (subs s (count pfx)) s))

;; declare-time is embedded in the id: @concern-<epoch-ms>-<hex>. A stale-age LOWER
;; BOUND when the owner never held a lease at all (dead-agent concerns predate presence).
(defn concern-mint-ms [c]
  (some-> (re-find #"concern-(\d{10,})" (str c)) second parse-long))

(defn owner-lapse-ms
  "How long this concern's owner has been OFFLINE, in ms — or nil if the owner is
   ONLINE (unexpired lease) or the concern is agent-less. When the owner holds an
   expired lease the lapse is exact; when it never held a lease (a pre-presence dead
   agent) the concern's own age is the staleness lower bound."
  [c]
  (let [a (north.coord/resolved port c "agent")]
    (when (and a (seq a))
      (let [now (System/currentTimeMillis)
            l   (north.coord/lease-of port (str "session:" (strip-sigil a "@")))]
        (cond
          (and l (> (:exp l) now)) nil                          ; owner ONLINE
          l                        (- now (:exp l))             ; expired lease -> exact lapse
          :else (when-let [m (concern-mint-ms c)] (- now m))))))) ; no lease -> age lower bound

(defn building-only?
  "True iff the concern reached `building` and never progressed past it (and isn't
   already abandoned). likely-to-land/landed are EXCLUDED — an ORPHANED retained
   recovery candidate must survive."
  [rs]
  (and (contains? rs "building")
       (not (rs "likely-to-land")) (not (rs "landed")) (not (rs "abandoned-stale"))))

(defn retire-stale-concern!
  "Invoke concern-cli's transition-aware terminal boundary. Its atomic outbox
   + reached batch is the authority; the reactor never appends terminal concern
   facts directly."
  [concern]
  (let [child
        (start-sweep-child!
         :concern-terminal-transition
         {:out :string :err :string}
         "bb" concern-transition-cli (str port) "retire-stale" concern)
        awaited (await-sweep-child! child CONCERN-TRANSITION-TIMEOUT-MS)]
    (when (= :timeout (:status awaited))
      (throw
       (ex-info "concern terminal transition timed out"
                {:type :concern-terminal-transition-timeout
                 :concern concern
                 :timeout-ms CONCERN-TRANSITION-TIMEOUT-MS})))
    (let [result (:result awaited)]
    (when-not (zero? (:exit result))
      (throw
       (ex-info "concern terminal transition failed"
                {:type :concern-terminal-transition-failed
                 :concern concern
                 :exit (:exit result)
                 :error (str/trim (str (:err result)))})))
    (let [transition
          (try
            (edn/read-string (str/trim (str (:out result))))
            (catch Exception error
              (throw
               (ex-info "concern terminal transition returned malformed output"
                        {:type :malformed-concern-terminal-transition
                         :concern concern}
                        error))))]
      (when-not
       (and (map? transition)
            (= #{:status :concern :trigger-status}
               (set (keys transition)))
            (#{:committed :already :ineligible} (:status transition))
            (= concern (:concern transition))
            (= "abandoned-stale" (:trigger-status transition)))
        (throw
         (ex-info "concern terminal transition returned an invalid result"
                  {:type :malformed-concern-terminal-transition
                   :concern concern
                   :result transition})))
      transition))))

(defn live-concern-repos
  "Container paths a LIVE concern currently claims, by the same lease rule
   `concern ls` renders. Only the unregistered-worktree sweep consumes it, and
   only when a tree is otherwise reapable, so the extra per-concern reads are
   never paid on an idle sweep."
  []
  (let [index (north.worktree-census/container-index
               (north.worktree-census/containers))]
    (into #{}
          (for [c (distinct (q-col [{:rel "triple" :args [{:var "e"} "kind" "concern"]}]))
                :let [reached (set (north.coord/many port c "reached"))]
                :when (and (not (reached "landed"))
                           (not (reached "abandoned-stale"))
                           (nil? (owner-lapse-ms c)))
                :let [container (north.worktree-census/resolve-container
                                 index (north.coord/resolved port c "repo"))]
                :when container]
            container))))

(defn sweep-concerns! [dry?]
  (let [concerns (distinct (q-col [{:rel "triple" :args [{:var "e"} "kind" "concern"]}]))
        hits (for [c concerns
                   :let  [rs (set (north.coord/many port c "reached"))]
                   :when (building-only? rs)
                   :let  [lapse (owner-lapse-ms c)]
                   :when (and lapse (>= lapse CONCERN-STALE-MS)
                              (or (nil? sweep-repo)
                                  (= sweep-repo (north.coord/resolved port c "repo"))))]
               {:c c :lapse lapse :agent (north.coord/resolved port c "agent")})]
    (reduce
     (fn [retired {:keys [c lapse agent]}]
       (if dry?
         (do
           (println
            (str "[sweep] WOULD abandon " c
                 "  owner " agent " lapsed " (long (/ lapse 3600000)) "h"))
           (inc retired))
         (let [{:keys [status]} (retire-stale-concern! c)]
           (println
            (str "[sweep] "
                 (case status
                   :committed "abandoned-stale"
                   :already "already abandoned-stale"
                   :ineligible "skipped stale retirement")
                 " " c "  owner " agent
                 " lapsed " (long (/ lapse 3600000)) "h"))
           (if (= :committed status) (inc retired) retired))))
     0
     hits)))

(defn ping-coordinator [coord h]
  (try
    (proc/shell {:out :string :err :string :continue true}
                "bb" (str (.getParent (io/file (System/getProperty "babashka.file"))) "/msg-cli.clj")
                (str port) "send" "north-reactor" coord "URGENT"
                (str "lane " h
                     " died unreported (presence lapsed >30min, no committed terminal) — reaped by reactor"))
    (catch Throwable _ nil)))

;; ---- impure GATHER for the reap verdict (pure logic lives in north.reap) ------------
;; The synchronous lane terminal is primary. A committed kind=run row is a
;; secondary trail; body facts from a crashed run writer remain invisible until
;; its last kind=run write. @swarm agent_death is a notification receipt only:
;; a hard kill between that ping and terminal publication must remain reapable.
(def max-lane-run-candidates 128)

(defn subject-facts
  "Read only the finite lifecycle predicates through subject+predicate indexes,
  preserving multi-value conflicts as sets."
  [subject predicates]
  (into {}
        (keep (fn [predicate]
                (let [values (set (north.coord/many port subject predicate))]
                  (when (seq values) [predicate values]))))
        predicates))

(defn runs-tagged-agent
  "All @run subjects tagged agent=<h>, including a latest torn/uncommitted row
  that must block fallback to an older terminal."
  [h]
  (try
    (let [response
          (north.coord/indexed-query-in-domain
           port
           :telemetry
           {:find "lane_run_candidate"
            :rules
            [{:head {:rel "lane_run_candidate" :args [{:var "e"}]}
              :body [{:rel "triple"
                      :args [{:var "e"} "agent" h]}]}]}
           ;; Ask for one sentinel row beyond the accepted bound. This simple
           ;; one-literal shape routes through Fram's warm predicate/object
           ;; index. query-page would rebuild a whole-corpus Datalog fixpoint
           ;; once per lane before applying its wire-page bound.
           (inc max-lane-run-candidates))
          rows (:ok response)]
      (if (and (= "index" (:engine response))
               (vector? rows)
               (<= (count rows) max-lane-run-candidates)
               (every? #(and (vector? %) (= 1 (count %))
                             (string? (first %)))
                       rows))
        {:ok true
         :subjects
         (->> rows
              (map first)
              (filter north.terminal-projection/valid-run-entity?)
              distinct
              vec)}
        {:ok false
         :reason (if (or (= :query-row-limit (:code response))
                         (and (vector? rows)
                              (> (count rows) max-lane-run-candidates)))
                   :run-projection-over-broad
                   :run-projection-unavailable)}))
    (catch Exception error
      {:ok false
       :reason (if (= :indexed-query-row-limit (:type (ex-data error)))
                 :run-projection-over-broad
                 :run-projection-unavailable)})))

(defn lane-resolution* [h]
  (let [run-projection (runs-tagged-agent h)
        subjects (:subjects run-projection)]
    (if-not (:ok run-projection)
      {:status :indeterminate :reason (:reason run-projection)}
      ;; The lifecycle predicates now read through the validated resolved
      ;; primitive, so an error map / malformed / timed-out projection THROWS
      ;; instead of silently reading as "no terminal facts". Catch it as
      ;; :indeterminate — an unreadable lane is reap-BLOCKED (lane-reap-blocked?*
      ;; treats every non-:unresolved status as protective), never falsely
      ;; :unresolved and never a bare throw out of the sweep.
      (try
        (let [lane-facts
              (subject-facts
               (str "@agent:" h)
               (conj north.terminal-projection/terminal-projection-predicates
                     "terminal_manifest_sha256"))]
          (north.terminal-projection/lane-resolution
           h lane-facts
           (mapv
            (fn [subject]
              {:subject subject
               :facts
               (subject-facts
                subject north.terminal-projection/run-resolution-predicates)})
            subjects)))
        (catch Exception _
          {:status :indeterminate :reason :lane-facts-unavailable})))))

(defn lane-resolved?* [h]
  (= :resolved (:status (lane-resolution* h))))

(defn lane-reap-blocked?* [h]
  ;; Reaping is destructive, so indeterminate lifecycle evidence protects the
  ;; lane. Janitor cleanup remains stricter and accepts only :resolved above.
  (not= :unresolved (:status (lane-resolution* h))))

(def agent-fact-writer
  (str (.getParent (io/file (System/getProperty "babashka.file")))
       "/agent-fact-internal.clj"))

(defn publish-reaped-terminal!
  "Use the harness's scoped terminal writer so a reaper verdict has the same
  readback and last-write digest protocol as every SDK terminal."
  [subject]
  (let [payload (json/generate-string
                 {"outcome" "died-unreported"
                  "process_outcome" "died-unreported"
                  "delivery_outcome" "blocked"
                  "delivery_reason" "presence_lapsed_without_committed_terminal"})
        result (proc/shell {:out :string :err :string :continue true}
                           "bb" agent-fact-writer (str port) "terminal" subject payload)]
    (when-not (zero? (:exit result))
      (throw (ex-info "failed to commit reaper terminal"
                      {:subject subject :stderr (:err result)})))))

(defn spawned-ms
  "@agent:<id> spawned_at (ISO) -> epoch ms, or nil (the leaseless-dead staleness axis)."
  [e]
  (when-let [ts (north.coord/resolved port e "spawned_at")]
    (try (.toEpochMilli (java.time.Instant/parse ts)) (catch Throwable _ nil))))

(defn driver-pairs []
  (:ok (north.coord/send-op port {:op :query
        :query {:find "row"
                :rules [{:head {:rel "row" :args [{:var "e"} {:var "driver"}]}
                         :body [{:rel "triple" :args [{:var "e"} "driver" {:var "driver"}]}]}]}})))

;; Historical compatibility: a pre-run-telemetry lane may have left a legacy
;; agent clock open. Close only that exact actor's legacy session. Current SDK
;; lanes never open billing clocks; their elapsed time is kind=run telemetry.
;; Idempotent and best-effort. north-bin is computed inline (its def is later).
(defn orphan-clock! [h]
  (try
    (proc/shell {:out :string :err :string :continue true}
                (-> (io/file (System/getProperty "babashka.file"))
                    .getParentFile .getParentFile (io/file "bin" "north") .getPath)
                "clock" "orphan" h)
    (catch Throwable _ nil)))

(defn release-orphaned-drivers! [h]
  ;; A hard-killed dispatch cannot run its finally/release. Once the SAME lane
  ;; crosses the 30-minute reap bar, retract only exact @<handle> driver refs.
  ;; A successor that won between query and retract has a different object and
  ;; is therefore untouched by the exact-value retraction.
  (let [driver-ref (str "@" h)
        threads (q-col [{:rel "triple" :args [{:var "e"} "driver" driver-ref]}])]
    (doseq [thread threads]
      (north.coord/retract! port thread "driver" driver-ref))))

(defn sweep-unpublished-driver-claims! [dry?]
  ;; Claim is intentionally the first dispatch side effect. A hard kill before
  ;; identity publication therefore leaves no kind=lane row for sweep-lanes!.
  ;; Current SDK IDs encode a mint timestamp; after the same 30-minute bar, an
  ;; unpublished holder is unrecoverable and its exact driver ref can be retired.
  ;; Legacy/malformed IDs have no trusted clock and are never guessed at.
  (let [now (System/currentTimeMillis)
        lanes (->> (q-col [{:rel "triple" :args [{:var "e"} "kind" "lane"]}])
                   (map #(strip-sigil % "@agent:"))
                   set)
        hits (north.reap/orphaned-unpublished-driver-pairs now lanes (driver-pairs))]
    (doseq [[thread driver-ref] hits]
      (when-not dry? (north.coord/retract! port thread "driver" driver-ref))
      (println (str "[sweep] " (if dry? "WOULD release" "released")
                    " unpublished driver " driver-ref " from " thread
                    "  age >=30min")))
    (count hits)))

(defn sweep-lanes! [dry?]
  (let [lanes (distinct (q-col [{:rel "triple" :args [{:var "e"} "kind" "lane"]}]))
        now   (System/currentTimeMillis)
        deaths (north.coord/many port "@swarm" "agent_death")
        hits (for [e lanes
                   :let  [h        (strip-sigil e "@agent:")
                          l        (north.coord/lease-of port (str "session:" h))
                          lease-exp (:exp l)
                          sp       (or (spawned-ms e) (north.reap/sdk-agent-mint-ms h))]
                   :when (north.reap/reap-lane? now (lane-reap-blocked?* h) lease-exp sp)]
               {:e e :h h :lapse (north.reap/lane-lapse-ms now lease-exp sp)})]
    (doseq [{:keys [e h lapse]} hits]
      (when-not dry?
        (publish-reaped-terminal! e)
        (orphan-clock! h)                                                  ; close any orphan clock session the dead lane left open
        (release-orphaned-drivers! h)                                      ; unblock threads held by the hard-killed lane
        ;; The reaper is the ONLY terminal for dead-lane spend: settle any open
        ;; spend reservation at FULL (status unknown) — idempotent (settled_at guard).
        (try (north.spend-breaker/reap-settle-lane-reservations! port h false)
             (catch Throwable t (println (str "[sweep] spend reaper-settle error: " (.getMessage t)))))
        ;; Death is terminal evidence, not a mutation of identity/name caches.
        ;; Every UI derives its decoration from the committed process/delivery facts.
        (let [coord (or (north.coord/resolved port e "coordinator")
                        (north.coord/resolved port e "supervisor"))]
          (when (and coord (seq coord)
                     (not (north.reap/death-reported? h deaths)))
            (ping-coordinator coord h))))
      (println (str "[sweep] " (if dry? "WOULD reap" "reaped") " lane " e
                    "  lapsed " (long (/ lapse 60000))
                    "min -> process=died-unreported delivery=blocked")))
    (count hits)))

;; ---- DAILY CLOCK-AUDIT TICK (drift telemetry) -------------------------------
;; The clock-audit output evaporates; a drift TREND is exactly the telemetry the
;; billing failure mode needs. Piggyback the 5-min sweep with a once-per-day gate:
;; state is the LAST clock_audit_run's run_at (a fact, never a loose state file), so
;; the gate is self-describing and survives a reactor restart. --dry-run reports WOULD
;; without writing, keeping sweep-once --dry-run clean.
(def CLOCK-AUDIT-INTERVAL-MS (* 24 60 60 1000))         ; once per day
(def DEFAULT-CLOCK-AUDIT-TIMEOUT-MS 45000)
(def MAX-CLOCK-AUDIT-TIMEOUT-MS 120000)
(def clock-audit-bin
  (or (System/getenv "NORTH_REACTOR_CLOCK_AUDIT_BIN")
      (-> (io/file (System/getProperty "babashka.file"))
          .getParentFile .getParentFile (io/file "bin" "north-clock-audit") .getPath)))

(defn clock-audit-timeout-ms []
  (let [raw (System/getenv "NORTH_REACTOR_CLOCK_AUDIT_TIMEOUT_MS")
        value (try
                (Long/parseLong (or raw (str DEFAULT-CLOCK-AUDIT-TIMEOUT-MS)))
                (catch Throwable _ -1))]
    (when-not (<= 1 value MAX-CLOCK-AUDIT-TIMEOUT-MS)
      (throw
       (ex-info
        (str "NORTH_REACTOR_CLOCK_AUDIT_TIMEOUT_MS must be between 1 and "
             MAX-CLOCK-AUDIT-TIMEOUT-MS " milliseconds")
        {:type :invalid-sweep-lifecycle-setting
         :name "NORTH_REACTOR_CLOCK_AUDIT_TIMEOUT_MS"
         :value raw
         :maximum MAX-CLOCK-AUDIT-TIMEOUT-MS})))
    value))

(defn last-clock-audit-ms
  "Newest kind=clock_audit_run run_at as epoch-ms, or nil if none exists yet."
  []
  (let [runs (distinct (q-col [{:rel "triple" :args [{:var "e"} "kind" "clock_audit_run"]}]))
        ms   (->> runs
                  (keep #(north.coord/resolved port % "run_at"))
                  (keep (fn [ts] (try (.toEpochMilli (java.time.Instant/parse ts))
                                      (catch Throwable _ nil)))))]
    (when (seq ms) (reduce max ms))))

(defn maybe-clock-audit!
  "Run clock-audit --persist at most once per day in an owned, bounded child.
   Exit 1 means uncovered commits and is still a completed audit. Timeout or
   launch failure is an explicit deferral, never a lost heartbeat."
  [dry?]
  (let [last (last-clock-audit-ms)
        due? (or (nil? last) (>= (- (System/currentTimeMillis) last) CLOCK-AUDIT-INTERVAL-MS))]
    (cond
      (not due?) {:status :skipped :reason :not-due}
      dry?       (do (println (str "[sweep] WOULD run clock-audit --persist"
                                   (when last (str " (last " (long (/ (- (System/currentTimeMillis) last) 3600000)) "h ago)"))))
                     {:status :would-run})
      :else
      (let [timeout-ms (clock-audit-timeout-ms)]
        (try
          (let [child (start-sweep-child!
                       :clock-audit
                       {:out :string :err :string}
                       clock-audit-bin "--persist")
                awaited (await-sweep-child! child timeout-ms)]
            (if (= :timeout (:status awaited))
              (do
                (println (str "[sweep] clock-audit deferred reason=timeout"
                              " timeout_ms=" timeout-ms))
                (flush)
                {:status :deferred :reason :timeout :timeout-ms timeout-ms
                 :cleanup (:cleanup awaited)})
              (let [result (:result awaited)]
                (println (str "[sweep] clock-audit --persist exit=" (:exit result)))
                (when (seq (str/trim (str (:err result))))
                  (println (str "[sweep] clock-audit stderr: "
                                (str/trim (str (:err result))))))
                (flush)
                {:status :completed :exit (:exit result)})))
          (catch InterruptedException interrupted
            (throw interrupted))
          (catch Throwable error
            (println (str "[sweep] clock-audit deferred reason=error error="
                          (pr-str (.getMessage error))))
            (flush)
            {:status :deferred :reason :error :error error}))))))

;; ---- REBUILD WINDOW OWNER ---------------------------------------------------
;; Agents queue rebuild asks (`north rebuild request`) and never fire; this sweep
;; is the ONLY collector. At most one coordinated rebuild per window, and a window
;; is consumed only by a firing — a parked queue never burns one.
(def REBUILD-WINDOW-UNIT "north-rebuild-window")

(defn launch-rebuild-window!
  "Run the claimed window OUTSIDE this sweep: a rebuild takes minutes and the
   sweep is bounded to four. The FIXED unit name is the double-fire mutex —
   systemd refuses a second start while one is running."
  [window-id]
  (try
    (let [north (-> (io/file (System/getProperty "babashka.file"))
                    .getParentFile .getParentFile (io/file "bin" "north") .getPath)
          r (proc/shell {:out :string :err :string :continue true}
                        "systemd-run" "--user" "--collect"
                        (str "--unit=" REBUILD-WINDOW-UNIT)
                        "--description=north coordinated rebuild window"
                        north "rebuild" "run-window" window-id)]
      (if (zero? (:exit r))
        {:launched true :unit REBUILD-WINDOW-UNIT}
        {:launched false :reason (str/trim (str (:err r)))}))
    (catch Throwable t {:launched false :reason (str (.getMessage t))})))

(defn maybe-rebuild-window!
  "Collect open requests into ONE coordinated rebuild. With rebuild-coordination
   off the owner only queues and reports; firing arms when that flip lands."
  [dry?]
  (try
    (let [plan (north.rebuild-request/plan-window port)
          n (:count plan)]
      (case (:action plan)
        :idle {:action "idle" :count 0}
        :queued {:action "queued" :count n}
        :waiting {:action "waiting" :count n}
        :fire
        (if dry?
          (do (println (str "[sweep] WOULD open a rebuild window for " n " request(s)"))
              {:action "would-fire" :count n})
          (let [window-id (north.rebuild-request/open-window! port (mapv :id (:open plan)))
                launch (launch-rebuild-window! window-id)]
            (if (:launched launch)
              (do (println (str "[sweep] rebuild window " window-id " launched for " n
                                " request(s) — unit " (:unit launch)))
                  {:action "fired" :count n :window window-id})
              (do (north.rebuild-request/set-window-action! port window-id "deferred")
                  (println (str "[sweep] rebuild window " window-id " deferred: "
                                (:reason launch)))
                  {:action "deferred" :count n :window window-id}))))))
    (catch Throwable t
      (println (str "[sweep] rebuild window error: " (.getMessage t)))
      {:action "error" :count 0 :error t})))

;; ---- AGENT STREAM-LOG ROTATION (durable-but-untidy GC) ----------------------
;; north-data/agents/*.log are per-agent SDK stream logs — hundreds of files,
;; unbounded, off-graph. Two BOUNDED hygiene ops, piggybacked on the sweep and gated
;; exactly like the reaper — the JANITOR never declares death, it only prunes what the
;; REAPER already marked terminal:
;;   (a) DELETE a log whose agent has committed terminal evidence AND mtime >30d.
;;       Without committed evidence it is NEVER touched, regardless of age — a
;;       silent-but-alive or not-yet-reaped trail must survive for the reaper/audit.
;;   (b) CAP any single log at 5MB, keeping the TAIL (recent turns are the useful end;
;;       the stale head is dropped). Independent of outcome — a runaway log is bounded
;;       even while its agent is live.
;; The expensive terminal-outcome query is gated behind the cheap mtime filter, so a
;; set of young logs costs zero coordinator round-trips. --dry-run prints WOULD-prune/
;; WOULD-cap without writing. Dir override NORTH_AGENT_LOGS_DIR (tests only), mirroring
;; TRIPWIRE_LOG_DIR / sweep-repo.
(def AGENT-LOG-STALE-MS (* 30 24 60 60 1000))    ; 30 days terminal -> prunable
(def AGENT-LOG-CAP-BYTES (* 5 1024 1024))        ; 5 MB tail cap
(def agent-logs-dir
  (or (System/getenv "NORTH_AGENT_LOGS_DIR")
      ;; <parent-of-repo>/north-data/agents — north-data is a SIBLING of the north repo.
      (-> (io/file (System/getProperty "babashka.file"))
          .getParentFile .getParentFile .getParentFile
          (io/file "north-data" "agents") .getPath)))

(defn cap-log-tail!
  "If f exceeds AGENT-LOG-CAP-BYTES, rewrite it to its last CAP bytes, dropping the
   partial leading line. Byte-exact (no charset round-trip). Returns bytes trimmed, or 0
   if under cap / dry-run. Atomic via .tmp + rename."
  [^java.io.File f dry?]
  (let [len (.length f)]
    (if (<= len AGENT-LOG-CAP-BYTES)
      0
      (let [trim (- len AGENT-LOG-CAP-BYTES)]
        (when-not dry?
          (let [buf (byte-array AGENT-LOG-CAP-BYTES)]
            (with-open [raf (java.io.RandomAccessFile. f "r")]
              (.seek raf trim)
              (.readFully raf buf))
            (let [nl    (loop [i 0] (cond (>= i (alength buf)) -1
                                          (= (aget buf i) (byte 10)) i
                                          :else (recur (inc i))))
                  start (if (>= nl 0) (inc nl) 0)
                  tmp   (io/file (str (.getPath f) ".tmp"))]
              (with-open [os (io/output-stream tmp)]
                (.write os buf start (- (alength buf) start)))
              (.renameTo tmp f))))
        trim))))

(defn sweep-agent-logs! [dry?]
  (let [dir  (io/file agent-logs-dir)
        now  (System/currentTimeMillis)
        logs (when (.isDirectory dir)
               (filter #(and (.isFile ^java.io.File %) (str/ends-with? (.getName ^java.io.File %) ".log"))
                       (.listFiles dir)))
        deleted (atom 0) capped (atom 0)]
    (doseq [^java.io.File f logs]
      (let [age (- now (.lastModified f))]
        (if (and (>= age AGENT-LOG-STALE-MS)
                 ;; expensive terminal-outcome join — reached ONLY for >30d logs
                 (lane-resolved?* (str/replace (.getName f) #"\.log$" "")))
          (do (when-not dry? (.delete f))
              (swap! deleted inc)
              (println (str "[sweep] " (if dry? "WOULD delete" "deleted") " log " (.getName f)
                            "  age " (long (/ age 86400000)) "d (agent resolved)")))
          (let [trimmed (cap-log-tail! f dry?)]
            (when (pos? trimmed)
              (swap! capped inc)
              (println (str "[sweep] " (if dry? "WOULD cap" "capped") " log " (.getName f)
                            "  -" (long (/ trimmed 1048576)) "MB (tail kept)")))))))
    {:deleted @deleted :capped @capped}))

(def ATTENTION-RECONCILE-TIMEOUT-MS 45000)
(def attention-reconcile-cli concern-transition-cli)

(defn reconcile-attention-bounded!
  "Run concern attention healing out of process. Failure or timeout is reported
   and deferred to the next sweep; it never escapes into reactor liveness."
  [reason]
  (try
    (let [child
          (start-sweep-child!
           :attention-reconcile
           {:out :string :err :string}
           "bb" attention-reconcile-cli (str port) "reconcile-attention")
          awaited (await-sweep-child! child ATTENTION-RECONCILE-TIMEOUT-MS)]
      (if (= :timeout (:status awaited))
        (do
          (println
           (str "[sweep] attention reconcile deferred reason=timeout"
                " timeout_ms=" ATTENTION-RECONCILE-TIMEOUT-MS
                " trigger=" reason))
          {:status :timeout})
        (let [result (:result awaited)
              ok? (zero? (:exit result))]
          (println
           (str "[sweep] attention reconcile "
                (if ok? "completed" "deferred")
                " trigger=" reason
                (when-not ok?
                  (str " exit=" (:exit result)
                       " error=" (pr-str (str/trim (str (:err result))))))))
          {:status (if ok? :completed :failed)
           :exit (:exit result)})))
    (catch Throwable error
      (println
       (str "[sweep] attention reconcile deferred trigger=" reason
            " error=" (pr-str (.getMessage error))))
      {:status :failed})))

(defn sweep-maintenance! [dry? rw]
  ;; A rejected concern transition is one stage's failure, never maintenance's;
  ;; only aggregate cancellation may still abort the run.
  (let [nc (run-sweep-stage!
            :concerns
            #(try
               (sweep-concerns! dry?)
               (catch Throwable t
                 (if (= :sweep-deadline (:type (ex-data t)))
                   (throw t)
                   (do (println (str "[sweep] concerns error: " (.getMessage t)))
                       0)))))
        nl (run-sweep-stage! :lanes #(sweep-lanes! dry?))
        nd (run-sweep-stage!
            :unpublished-driver-claims
            #(sweep-unpublished-driver-claims! dry?))
        wt (run-sweep-stage!
            :worktree-janitor
            #(try
               (north.worktree-janitor/sweep-worktrees!
                {:port port
                 :dry? dry?
                 :repo-filter sweep-repo
                 :lane-resolved? lane-resolved?*})
               (catch Throwable t
                 (println (str "[sweep] worktree janitor error: " (.getMessage t)))
                 {:scanned 0 :unresolved 0 :dirty 0 :uncertain 0 :partial 0
                  :already-removed 0
                  :removed 0 :would-remove 0 :orphan-facts-written 0
                  :errors 1})))
        uw (run-sweep-stage!
            :unregistered-worktree-janitor
            #(try
               (north.worktree-janitor/sweep-unregistered-worktrees!
                {:dry? dry?
                 :repo-filter sweep-repo
                 :claimed-worktrees
                 (delay (north.worktree-janitor/registered-worktree-paths
                         (north.coord/expected-log)))
                 :live-concern-repos (delay (live-concern-repos))})
               (catch Throwable t
                 (println (str "[sweep] unregistered worktree janitor error: "
                               (.getMessage t)))
                 {:scanned 0 :claimed 0 :fresh 0 :review 0 :live-concern 0
                  :uncertain 0 :partial 0 :removed 0 :would-remove 0
                  :errors 1})))
        al (run-sweep-stage! :agent-logs #(sweep-agent-logs! dry?))
        ;; Spend-guard backstop (step 3): burn-rate breach TRIPS the global breaker;
        ;; a tripped breaker SIGKILLs every verified live breached lane + settles it.
        ;; Best-effort — a coordinator hiccup here never crashes the liveness sweep.
        burn (run-sweep-stage!
              :spend-burn
              #(try
                 (north.spend-breaker/sweep-burn! port dry?)
                 (catch Throwable t
                   (println (str "[sweep] burn error: " (.getMessage t)))
                   {:tripped false})))
        nk (run-sweep-stage!
            :spend-kill
            #(try
               (north.spend-breaker/sweep-kill! port dry?)
               (catch Throwable t
                 (println (str "[sweep] sweep-kill error: " (.getMessage t)))
                 0)))
        ;; The liveness work is complete at this boundary. Stamp it before
        ;; optional maintenance so a deferred audit cannot make a healthy core
        ;; sweep look dead.
        _ (run-sweep-stage!
           :core-heartbeat
           #(when-not dry?
              (north.reactor-heartbeat/write-heartbeat!
               port {:worktrees wt :unregistered-worktrees uw})))
        ca (run-sweep-stage! :clock-audit #(maybe-clock-audit! dry?))
        audit-deferred? (= :deferred (:status ca))
        attention (if audit-deferred?
                    {:status :skipped :reason :clock-audit-deferred}
                    (run-sweep-stage!
                     :attention-reconcile
                     #(if dry?
                        {:status :skipped}
                        (reconcile-attention-bounded! "post-sweep"))))
        summary {:concerns nc :lanes nl :unpublished-drivers nd
                 :worktrees wt :unregistered-worktrees uw
                 :agent-logs al :breaker burn
                 :lanes-killed nk :clock-audit ca
                 :rebuild-window rw
                 :attention-reconcile attention
                 :terminal-status (if audit-deferred? :deferred :completed)
                 :deferred-reason (when audit-deferred? :clock-audit)}]
    (println (str "[sweep] " (when dry? "(dry-run) ") "concerns abandoned=" nc
                  " lanes reaped=" nl " unpublished drivers released=" nd
                  " worktrees removed=" (:removed wt)
                  " dirty-kept=" (:dirty wt)
                  " uncertain-kept=" (:uncertain wt)
                  " partial-cleanup=" (:partial wt)
                  " already-reclaimed=" (:already-removed wt)
                  " orphan-facts=" (:orphan-facts-written wt)
                  " worktree-errors=" (get wt :errors 0)
                  " unregistered scanned=" (:scanned uw)
                  " removed=" (:removed uw)
                  " would-remove=" (:would-remove uw)
                  " needs-review=" (:review uw)
                  " concern-held=" (:live-concern uw)
                  " kept-uncertain=" (:uncertain uw)
                  " partial=" (:partial uw)
                  " errors=" (get uw :errors 0)
                  " logs deleted=" (:deleted al) " capped=" (:capped al)
                  " breaker-tripped=" (:tripped burn) " lanes-killed=" nk
                  " clock-audit=" (name (:status ca))
                  (when-let [reason (:reason ca)] (str ":" (name reason)))
                  " rebuild-window=" (:action rw) ":" (:count rw)
                  " attention-reconcile=" (name (:status attention))))
    (flush)
    summary))

(defn sweep! [dry?]
  ;; A fired window is durable; independent maintenance cannot rewrite that outcome.
  (let [rw (run-sweep-stage! :rebuild-window #(maybe-rebuild-window! dry?))]
    (when (= "error" (:action rw))
      (throw
       (ex-info "rebuild window collection failed"
                {:type :rebuild-window-collection-failed
                 :rebuild-window (dissoc rw :error)}
                (:error rw))))
    (try
      (sweep-maintenance! dry? rw)
      (catch Throwable error
        (if (= "fired" (:action rw))
          (let [stage (or (some-> *sweep-runtime* :stage deref) :maintenance)
                maintenance {:status :degraded :stage stage :error error}
                summary {:rebuild-window rw
                         :maintenance maintenance
                         :terminal-status :completed}]
            (println (str "[sweep] maintenance=degraded after rebuild-window=fired"
                          " stage=" (name stage)
                          " error=" (pr-str (.getMessage error))
                          " action=retry-maintenance-on-next-scheduled-run"))
            (flush)
            summary)
          (throw error))))))

(declare sweep-once-exit-code)

(defn sweep-loop []
  (loop []
    (Thread/sleep (* 5 60 1000))                    ; 5-min cadence, first sweep after one interval
    ;; Both automatic owners enter through the same lock and aggregate deadline.
    (try (sweep-once-exit-code)
         (catch Throwable t (println (str "[sweep] error: " (.getMessage t))) (flush)))
    (recur)))

;; bin/north is a sibling of this cli/ dir: <repo>/cli/north-reactor.clj -> <repo>/bin/north
(def north-bin
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile (io/file "bin" "north") .getPath))

;; Coordination-EPHEMERAL subjects: never projected to a thread .md AND written at
;; tool-call frequency (presence leases, session stamps, per-run costs, messages,
;; command envelopes, agent/role registry). Skipping them keeps heal firing only on
;; REAL thread edits instead of on every heartbeat — the reactor's whole cost budget.
(def ephemeral-prefixes
  ["@lease:" "@session:" "@run:" "@cmd:" "@agent:" "@role:"
   "@notification:" "@subscription:"])
(defn ephemeral? [l]
  (and (string? l) (boolean (some #(str/starts-with? l %) ephemeral-prefixes))))

(def last-commit (atom 0))   ; wall-clock of the most recent projected-relevant commit
(def dirty       (atom false))
(def running     (atom false))
(def last-heal-out (atom nil))  ; last heal output line — dedup repeated identical output

(defn heal! []
  ;; Shell the SAME `north heal` a human runs — byte-identical projection, fail-closed
  ;; on hand edits, reads the flat log directly (no daemon dependency). FRAM_LOG
  ;; is pinned to this coordination corpus; FRAM_THREADS/FRAM_PORT stay inherited.
  ;; NOISE FIX: a permanent hand-edit refusal re-prints "heal REFUSED …" on EVERY flush,
  ;; so a single unresolved conflict grew reactor-7977.log to 642KB of one repeated line —
  ;; burying real events. Dedup: log heal output only when it CHANGES from the last line.
  ;; A resolved conflict (output goes empty/different) prints again, so no signal is lost.
  (try
    (let [r    (proc/shell
                {:out :string :err :string :continue true
                 :extra-env {"FRAM_LOG" (north.coord/expected-log)
                             "NORTH_TELEMETRY_PARTITION" "0"
                             "FRAM_TELEMETRY_LOG" ""}}
                north-bin "heal")
          out  (str/trim (str (:out r) (when (seq (:err r)) (str "\n" (:err r)))))
          line (when (seq out) (str "[reactor] " (str/replace out #"\n+" " | ")))]
      (when (and line (not= line @last-heal-out))
        (println line) (flush))
      (reset! last-heal-out line))
    (catch Throwable t
      (println (str "[reactor] heal error: " (.getMessage t))) (flush))))

;; Flusher: once a burst goes quiet for debounce-ms, project. Coalesced — only one
;; heal in flight; commits arriving mid-heal re-arm dirty for the next quiet window.
(defn flusher []
  (loop []
    (Thread/sleep 100)
    (when (and @dirty (not @running)
               (>= (- (System/currentTimeMillis) @last-commit) debounce-ms))
      (reset! dirty false)
      (reset! running true)
      (try (heal!) (finally (reset! running false))))
    (recur)))

(defn mark! [l]
  (when-not (ephemeral? l)
    (reset! last-commit (System/currentTimeMillis))
    (reset! dirty true)))

(defn subscribe-once
  "Open one subscription and pump commit events until the socket drops. Returns on
   disconnect (daemon bounce / restart) so -main can reconnect."
  []
  (with-open [s (north.coord/connect-socket port)]
    (let [w (.getOutputStream s)
          reader (north.coord/coordinator-reader s)]
      (.write w
              (.getBytes
               (str (pr-str
                     (north.coord/log-envelope {:op :subscribe}))
                    "\n")
               java.nio.charset.StandardCharsets/UTF_8))
      (.flush w)
      (north.coord/validate-subscription!
       (north.coord/read-line-bounded! reader))
      (.setSoTimeout s 0)               ; validated long-lived stream: wait indefinitely for pushes
      (loop []
        (when-let [line
                   (north.coord/read-stream-line-bounded! reader)]
          (let [ev (try (edn/read-string line) (catch Throwable _ nil))]
            (when (and (map? ev) (= (:event ev) :commit))
              (mark! (:l ev))))
          (recur))))))

(defn -main []
  (println (str "[reactor] coordinator auto-export: subscribe :" port
                " (debounce " debounce-ms "ms) -> " north-bin " heal"
                " | liveness sweep every 5min"))
  (flush)
  ;; Stamp once at startup so a just-booted reactor reads FRESH in doctor immediately,
  ;; rather than MISSING for the first 5-min interval before sweep-loop's first pass.
  (north.reactor-heartbeat/write-heartbeat! port)
  ;; Startup healing is isolated from subscription admission and bounded inside
  ;; the child process wrapper.
  (future (reconcile-attention-bounded! "startup"))
  (future (flusher))
  (future (sweep-loop))       ; liveness-derived reaping on the reactor cadence
  (loop []
    (try (subscribe-once)
         (catch Throwable t
           (println (str "[reactor] subscription lost (" (.getMessage t) ") — reconnecting")) (flush)))
    (Thread/sleep 1000)               ; brief backoff, then reconnect (survives a bounce)
    (recur)))

;; ---- BOUNDED ONE-SHOT LIFECYCLE --------------------------------------------
;; The user timer invokes sweep-once every five minutes. Individual coordinator
;; reads are bounded, but a sweep performs many of them; summing those per-call
;; bounds allowed one oneshot to stay activating for more than twenty minutes.
;; Keep the aggregate deadline below the timer cadence and serialize all entry
;; paths with an OS lock. A coordinator bounce is retryable and ultimately
;; DEFERRED (the next timer tick is the retry), while a code/configuration defect
;; remains a nonzero terminal failure.
(def MAX-SWEEP-TIMEOUT-MS (* 4 60 1000))
(def DEFAULT-SWEEP-RETRY-MS 250)

(defn positive-ms
  [name default maximum]
  (let [raw (System/getenv name)
        value (try
                (Long/parseLong (or raw (str default)))
                (catch Throwable _ -1))]
    (when-not (<= 1 value maximum)
      (throw (ex-info (str name " must be between 1 and " maximum " milliseconds")
                      {:type :invalid-sweep-lifecycle-setting
                       :name name :value raw :maximum maximum})))
    value))

(defn sweep-lock-path []
  (or (System/getenv "NORTH_REACTOR_SWEEP_LOCK_PATH")
      (let [filename (if dry-run?
                       "north-reactor-sweep-dry-run.lock"
                       "north-reactor-sweep.lock")]
        (if-let [runtime-dir (System/getenv "XDG_RUNTIME_DIR")]
          (.getPath (io/file runtime-dir filename))
          (.getPath (io/file (System/getProperty "user.home")
                             ".cache" "north"
                             (if dry-run?
                               "reactor-sweep-dry-run.lock"
                               "reactor-sweep.lock")))))))

(def retryable-coordinator-types
  #{:coordinator-response-timeout
    :coordinator-response-closed
    :coordinator-response-truncated})

(defn throwable-chain [throwable]
  (take-while some? (iterate #(.getCause ^Throwable %) throwable)))

(defn retryable-coordinator-error? [throwable]
  (boolean
   (some (fn [cause]
           (let [data (ex-data cause)]
             (or (instance? java.net.ConnectException cause)
                 (instance? java.net.SocketTimeoutException cause)
                 (instance? java.net.SocketException cause)
                 (instance? java.io.EOFException cause)
                 (contains? retryable-coordinator-types (:type data))
                 (and (= :indexed-query-error (:type data))
                      (= :query-time-limit (:code data))))))
         (throwable-chain throwable))))

(defn concise-error [throwable]
  (let [root (last (throwable-chain throwable))]
    (str (.getSimpleName (class root))
         (when-let [message (.getMessage ^Throwable root)]
           (str ": " message)))))

(defn remaining-ms [deadline-ns]
  (deadline-remaining-ms deadline-ns))

(defn run-sweep-attempt!
  "Run one attempt without allowing it past deadline-ns. Cancellation closes
   phase admission first, then drains every registered child before returning."
  [dry? deadline-ns]
  (let [result (promise)
        runtime {:deadline-ns deadline-ns
                 :cancelled? (atom false)
                 :stage (atom :starting)
                 :children (atom {})}
        task (future
               (binding [*sweep-runtime* runtime]
                 (try
                   (let [summary (sweep! dry?)]
                     (deliver result
                              {:status (:terminal-status summary)
                               :reason (:deferred-reason summary)
                               :summary summary}))
                   (catch Throwable throwable
                     (deliver
                      result
                      (if (= :sweep-deadline (:type (ex-data throwable)))
                        {:status :deadline :stage (:stage (ex-data throwable))}
                        {:status :error :error throwable}))))))
        remaining (remaining-ms deadline-ns)
        observed (if (pos? remaining)
                   (deref result remaining ::deadline)
                   ::deadline)]
    (if (or (= ::deadline observed) (= :deadline (:status observed)))
      (try
        ;; Snapshot+close admission before interrupting the task. Otherwise its
        ;; InterruptedException cleanup can unregister the child between those
        ;; two acts and make the terminal receipt nondeterministically say 0/0.
        (let [cleanup (cancel-sweep-runtime! runtime)]
          (future-cancel task)
          {:status :deadline
           :stage (or (:stage observed) @(:stage runtime))
           :child-cleanup cleanup})
        (catch Throwable cleanup-error
          (future-cancel task)
          {:status :failed
           :stage @(:stage runtime)
           :error cleanup-error}))
      observed)))

(defn run-bounded-sweep!
  [dry? timeout-ms retry-ms]
  (let [started-ns (System/nanoTime)
        deadline-ns (+ started-ns (* 1000000 timeout-ms))]
    (loop [attempt 1]
      (let [{:keys [status error] :as observed}
            (run-sweep-attempt! dry? deadline-ns)]
        (cond
          (= :completed status)
          (assoc observed :attempts attempt
                          :elapsed-ms (long (/ (- (System/nanoTime) started-ns) 1000000)))

          (= :deferred status)
          (assoc observed :attempts attempt
                          :elapsed-ms (long (/ (- (System/nanoTime) started-ns) 1000000)))

          (= :deadline status)
          (assoc observed :attempts attempt :timeout-ms timeout-ms)

          (not (retryable-coordinator-error? error))
          {:status :failed :attempts attempt :error error}

          (<= (remaining-ms deadline-ns) retry-ms)
          {:status :unavailable :attempts attempt :timeout-ms timeout-ms :error error}

          :else
          (let [backoff-ms (min 5000
                                (remaining-ms deadline-ns)
                                (* retry-ms (bit-shift-left 1 (min 4 (dec attempt)))))]
            (when (or (= 1 attempt) (zero? (mod attempt 10)))
              (println (str "[sweep] coordinator unavailable (" (concise-error error)
                            "); retrying in " backoff-ms "ms (attempt " attempt ")"))
              (flush))
            (Thread/sleep backoff-ms)
            (recur (inc attempt))))))))

(defn with-sweep-lock [f]
  (let [path (sweep-lock-path)
        file (io/file path)]
    (when-let [parent (.getParentFile file)] (.mkdirs parent))
    (with-open [random-access (java.io.RandomAccessFile. file "rw")
                channel (.getChannel random-access)]
      (let [lock (.tryLock channel)]
        (if-not lock
          {:status :already-running :lock path}
          ;; Closing the channel releases the lock. Babashka intentionally does
          ;; not expose FileLock.release, so keep ownership scoped by with-open.
          (f))))))

(defn sweep-once-exit-code []
  (try
    (let [timeout-ms (positive-ms "NORTH_REACTOR_SWEEP_TIMEOUT_MS"
                                  MAX-SWEEP-TIMEOUT-MS MAX-SWEEP-TIMEOUT-MS)
          retry-ms (positive-ms "NORTH_REACTOR_SWEEP_RETRY_MS"
                                (min DEFAULT-SWEEP-RETRY-MS timeout-ms) timeout-ms)
          result (with-sweep-lock
                   #(run-bounded-sweep! dry-run? timeout-ms retry-ms))]
      (cond
        (= :completed (:status result))
        (let [maintenance (get-in result [:summary :maintenance])]
          (println (str "[sweep] terminal=completed attempts=" (:attempts result)
                        " elapsed_ms=" (:elapsed-ms result)
                        (when (= :degraded (:status maintenance))
                          (str " rebuild-window="
                               (get-in result [:summary :rebuild-window :action])
                               " maintenance=degraded"
                               " stage=" (name (:stage maintenance))))))
            (flush)
            0)

        (= :already-running (:status result))
        (do (println (str "[sweep] terminal=deferred reason=already-running"
                          " action=wait-for-current-sweep lock=" (:lock result)))
            (flush)
            0)

        (= :deferred (:status result))
        (let [audit (get-in result [:summary :clock-audit])]
          (println (str "[sweep] terminal=deferred reason="
                        (name (or (:reason result) :maintenance))
                        (when-let [audit-reason (:reason audit)]
                          (str " audit_reason=" (name audit-reason)))
                        (when-let [timeout (:timeout-ms audit)]
                          (str " timeout_ms=" timeout))
                        " attempts=" (:attempts result)
                        " action=retry-on-next-scheduled-run"))
          (flush)
          0)

        (contains? #{:deadline :unavailable} (:status result))
        (do (println (str "[sweep] terminal=deferred reason=" (name (:status result))
                          " timeout_ms=" timeout-ms
                          " attempts=" (:attempts result)
                          (when-let [stage (:stage result)]
                            (str " stage=" (name stage)))
                          (when-let [cleanup (:child-cleanup result)]
                            (str " child_cleanup="
                                 (:terminated cleanup) "/" (:registered cleanup)
                                 " surviving=" (:surviving cleanup)))
                          (when-let [error (:error result)]
                            (str " last_error=\"" (concise-error error) "\""))
                          " action=retry-on-next-scheduled-run"))
            (flush)
            0)

        :else
        (do (binding [*out* *err*]
              (println (str "[sweep] terminal=failed attempts=" (:attempts result)
                            " error=\"" (concise-error (:error result)) "\""
                            " action=inspect-north-reactor-journal"))
              (flush))
            1)))
    (catch Throwable throwable
      (binding [*out* *err*]
        (println (str "[sweep] terminal=failed error=\"" (concise-error throwable) "\""
                      " action=check-sweep-lifecycle-configuration"))
        (flush))
      1)))

(if sweep-verb?
  (System/exit (sweep-once-exit-code))
  (-main))
