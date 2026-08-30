#!/usr/bin/env bb
;; Run exactly one scheduled maintenance responsibility. Systemd owns cadence,
;; concurrency, restart, and failure isolation; this host never chains tasks.
(require '[cheshire.core :as json]
         '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[babashka.process :as proc])

;; shared coord substrate (write verbs + renewable-lease liveness) — the sweep judges
;; owner death by the SAME lease rule presence-cli/concern-cli use, and writes its
;; verdict through the coordinator (auditable facts, never a mutated cell).
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
;; PURE reap decisions (verdict off in-memory facts) — split out so tests/reap_test.clj can
;; drive the join/lapse/verdict logic with no live daemon. Sibling of coord.clj.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/reap.clj"))
;; Shared Git-derived worktree read model. `north worktrees` renders it; the
;; janitor's unregistered sweep decides off it. Must load before the janitor.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file")))
                "/worktree-census.clj"))
;; Side-effect-free managed-worktree janitor.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file")))
                "/worktree-janitor.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file")))
                "/worker-heartbeat.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/spend-breaker.clj"))
(load-file
 (str (-> (io/file (System/getProperty "babashka.file"))
          .getParentFile .getParentFile .getCanonicalPath)
      "/out/north/worker_policy.clj"))

(def raw-args *command-line-args*)
(def maintenance-task
  (some-> (first raw-args) keyword))
(def s-args (vec (rest raw-args)))
(def sweep-flags (set (filter #(str/starts-with? % "--") s-args)))
(def dry-run? (contains? sweep-flags "--dry-run"))
(def sweep-repo
  (let [i (.indexOf (vec s-args) "--repo")]
    (when (>= i 0) (get s-args (inc i)))))
(def port
  (Integer/parseInt
   (or (System/getenv "NORTH_PORT")
       (System/getenv "BEAGLE_STORE_PORT")
       "7977")))

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
           (ex-info "maintenance child process tree survived cancellation"
                    {:type :sweep-child-cleanup-failed
                     :pids (mapv #(.pid ^java.lang.ProcessHandle %) survivors)})))
        {:handles (count expanded) :terminated (count expanded)}))))

(defn sweep-deadline-ex [stage]
  (ex-info "maintenance task deadline reached"
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
  "Start and register a task-owned child under the same lock that publishes
   aggregate cancellation. Outside a scheduled task, retain the normal bounded child."
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
;; Two terminal verdicts the lifecycle janitors write on their cadences:
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

(def automatic-index-row-limit 4096)

(defn indexed-subjects-for [predicate object]
  (->> (north.coord/bounded-query-in-domain!
        port
        :coordination
        {:find "maintenance_subject"
         :rules
         [{:head {:rel "maintenance_subject"
                  :args [{:var "entity"}]}
           :body
           [{:rel "triple"
             :args [{:var "entity"} predicate object]}]}]}
        automatic-index-row-limit)
       :rows
       (mapv first)))

(defn indexed-predicate-rows [predicate]
  (:rows
   (north.coord/bounded-query-in-domain!
    port
    :coordination
    {:find "maintenance_predicate_rows"
     :rules
     [{:head
       {:rel "maintenance_predicate_rows"
        :args [{:var "entity"} {:var "value"}]}
       :body
       [{:rel "triple"
         :args [{:var "entity"} predicate {:var "value"}]}]}]}
    automatic-index-row-limit)))

(defn strip-sigil [s pfx] (if (str/starts-with? s pfx) (subs s (count pfx)) s))

;; declare-time is embedded in the id: @concern-<epoch-ms>-<hex>. A stale-age LOWER
;; BOUND when the owner never held a lease at all (dead-agent concerns predate presence).
(defn concern-mint-ms [c]
  (some-> (re-find #"concern-(\d{10,})" (str c)) second parse-long))

(defn fact-values [rows predicate]
  (->> rows
       (keep (fn [[row-predicate value]]
               (when (= predicate row-predicate) value)))
       distinct
       sort
       vec))

(defn fact-value [rows predicate]
  (first (fact-values rows predicate)))

(defn subject-fact-rows [domain subjects]
  (if (empty? subjects)
    {}
    (:rows (north.coord/show-many-in-domain! port domain subjects))))

(defn online-session-handles [now]
  (into #{} (map :handle) (north.coord/online-session-leases! port now)))

(defn stale-owner-lapse-ms
  "Return exact stale-owner lapse for an old offline concern. One batch scan
   eliminates live owners before this point read is needed."
  [now online-handles concern rows]
  (when-let [agent (fact-value rows "agent")]
    (let [handle (strip-sigil agent "@")
          minted (concern-mint-ms concern)]
      (when (and (not (contains? online-handles handle))
                 (or (nil? minted) (>= (- now minted) CONCERN-STALE-MS)))
        (let [{:keys [online? exp]} (north.coord/session-lease-status! port handle)]
          (when-not online?
            (cond
              (integer? exp) (- now exp)
              minted (- now minted))))))))

(defn building-only?
  "True iff the concern reached `building` and never progressed past it (and isn't
   already abandoned). likely-to-land/landed are EXCLUDED — an ORPHANED retained
   recovery candidate must survive."
  [rs]
  (and (contains? rs "building")
       (not (rs "likely-to-land")) (not (rs "landed")) (not (rs "abandoned-stale"))))

(defn retire-stale-concern!
  "Invoke concern-cli's transition-aware terminal boundary. Its atomic outbox
   + reached batch is the authority; the janitor never appends terminal concern
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
   `concern ls` renders. The subjects and all of their facts are read in one
   pinned batch; liveness comes from one canonical lease scan."
  []
  (let [concerns (indexed-subjects-for "kind" "concern")
        rows-by-concern (subject-fact-rows :coordination concerns)
        online (online-session-handles (System/currentTimeMillis))
        index (north.worktree-census/container-index
               (north.worktree-census/containers))]
    (into #{}
          (for [c concerns
                :let [rows (get rows-by-concern c [])
                      reached (set (fact-values rows "reached"))
                      agent (fact-value rows "agent")]
                :when (and (not (reached "landed"))
                           (not (reached "abandoned-stale"))
                           (or (nil? agent)
                               (contains? online (strip-sigil agent "@"))))
                :let [container (north.worktree-census/resolve-container
                                 index (fact-value rows "repo"))]
                :when container]
            container))))

(defn sweep-concerns! [dry?]
  (let [concerns (indexed-subjects-for "kind" "concern")
        rows-by-concern (subject-fact-rows :coordination concerns)
        now (System/currentTimeMillis)
        online (online-session-handles now)
        hits (for [c concerns
                   :let  [rows (get rows-by-concern c [])
                          rs (set (fact-values rows "reached"))
                          repo (fact-value rows "repo")]
                   :when (building-only? rs)
                   :when (or (nil? sweep-repo) (= sweep-repo repo))
                   :let  [lapse (stale-owner-lapse-ms now online c rows)]
                   :when (and lapse (>= lapse CONCERN-STALE-MS))]
               {:c c :lapse lapse :agent (fact-value rows "agent")})]
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
                (str port) "send" "north-lane-lifecycle-janitor" coord "URGENT"
                (str "lane " h
                     " died unreported (liveness lease lapsed >30min, no committed terminal) — reaped by the lane lifecycle janitor"))
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
                (let [values (set (north.coord/many! port subject predicate))]
                  (when (seq values) [predicate values]))))
        predicates))

(defn subject-facts-from-rows [rows predicates]
  (into {}
        (keep (fn [predicate]
                (let [values (set (fact-values rows predicate))]
                  (when (seq values) [predicate values]))))
        predicates))

(defn runs-tagged-agent
  "All @run subjects tagged agent=<h>, including a latest torn/uncommitted row
  that must block fallback to an older terminal."
  [h]
  (try
    (let [response
          (north.coord/bounded-query-in-domain!
           port
           :telemetry
           {:find "lane_run_candidate"
            :rules
            [{:head {:rel "lane_run_candidate" :args [{:var "e"}]}
              :body [{:rel "triple"
                      :args [{:var "e"} "agent" h]}]}]}
           ;; Ask for one sentinel row beyond the accepted bound. This simple
          ;; one-literal shape routes through Beagle Store's warm predicate/object
          ;; index. query-page would rebuild a whole-corpus Datalog fixpoint
          ;; once per lane before applying its wire-page bound.
           (inc max-lane-run-candidates))
          rows (:rows response)]
      (if (and (vector? rows)
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
         :reason (if (and (vector? rows)
                          (> (count rows) max-lane-run-candidates))
                   :run-projection-over-broad
                   :run-projection-unavailable)}))
    (catch Exception error
      {:ok false
       :reason (if (= :query-row-limit (:type (ex-data error)))
                 :run-projection-over-broad
                 :run-projection-unavailable)})))

(def max-lane-run-batch-handles
  (quot (dec automatic-index-row-limit) (inc max-lane-run-candidates)))

(defn runs-tagged-agents [handles]
  (reduce
   (fn [results handle-batch]
     (let [handle-set (set handle-batch)]
       (try
         (let [rows
               (:rows
                (north.coord/bounded-query-in-domain!
                 port
                 :telemetry
                 {:find "lane_run_candidates"
                  :rules
                  (mapv
                   (fn [handle]
                     {:head {:rel "lane_run_candidates"
                             :args [handle {:var "run"}]}
                      :body [{:rel "triple"
                              :args [{:var "run"} "agent" handle]}]})
                   handle-batch)}
                 (inc (* max-lane-run-candidates (count handle-batch)))))
               valid? (and (vector? rows)
                           (every?
                            (fn [row]
                              (and (vector? row) (= 2 (count row))
                                   (contains? handle-set (first row))
                                   (north.terminal-projection/valid-run-entity?
                                    (second row))))
                            rows))
               by-handle (group-by first rows)]
           (if valid?
             (reduce
              (fn [batch-results handle]
                (let [subjects (->> (get by-handle handle [])
                                    (map second) distinct vec)]
                  (assoc batch-results handle
                         (if (<= (count subjects) max-lane-run-candidates)
                           {:ok true :subjects subjects}
                           {:ok false :reason :run-projection-over-broad}))))
              results handle-batch)
             (reduce #(assoc %1 %2 {:ok false
                                    :reason :run-projection-unavailable})
                     results handle-batch)))
         (catch Exception error
           (let [reason (if (= :query-row-limit (:type (ex-data error)))
                          :run-projection-over-broad
                          :run-projection-unavailable)]
             (reduce #(assoc %1 %2 {:ok false :reason reason})
                     results handle-batch))))))
   {}
   (partition-all max-lane-run-batch-handles handles)))

(defn lane-resolutions-batch [handles]
  (if (empty? handles)
    {}
    (let [run-projections (runs-tagged-agents handles)
          run-subjects (->> run-projections vals (filter :ok)
                            (mapcat :subjects) distinct vec)
          lane-subjects (mapv #(str "@agent:" %) handles)
          lane-rows (subject-fact-rows :coordination lane-subjects)
          run-rows (if (empty? run-subjects)
                     {}
                     (subject-fact-rows :telemetry run-subjects))]
      (into {}
            (map
             (fn [handle]
               (let [{:keys [ok subjects reason]} (get run-projections handle)]
                 [handle
                  (if-not ok
                    {:status :indeterminate :reason reason}
                    (try
                      (north.terminal-projection/lane-resolution
                       handle
                       (subject-facts-from-rows
                        (get lane-rows (str "@agent:" handle) [])
                        (conj north.terminal-projection/terminal-projection-predicates
                              "terminal_manifest_sha256"))
                       (mapv
                        (fn [subject]
                          {:subject subject
                           :facts
                           (subject-facts-from-rows
                            (get run-rows subject [])
                            north.terminal-projection/run-resolution-predicates)})
                        subjects))
                      (catch Exception _
                        {:status :indeterminate
                         :reason :lane-facts-unavailable})))]))
             handles)))))

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
                  "delivery_reason" "liveness_lease_lapsed_without_committed_terminal"})
        result (proc/shell {:out :string :err :string :continue true}
                           "bb" agent-fact-writer (str port) "terminal" subject payload)]
    (when-not (zero? (:exit result))
      (throw (ex-info "failed to commit reaper terminal"
                      {:subject subject :stderr (:err result)})))))

(defn spawned-ms
  "@agent:<id> spawned_at (ISO) -> epoch ms, or nil (the leaseless-dead staleness axis)."
  [e]
  (when-let [ts (north.coord/resolved! port e "spawned_at")]
    (try (.toEpochMilli (java.time.Instant/parse ts)) (catch Throwable _ nil))))

(defn spawned-ms-from-rows [rows]
  (when-let [timestamp (fact-value rows "spawned_at")]
    (try
      (.toEpochMilli (java.time.Instant/parse timestamp))
      (catch Throwable _ nil))))

(defn driver-pairs []
  (indexed-predicate-rows "driver"))

(defn release-orphaned-drivers! [h]
  ;; A hard-killed dispatch cannot run its finally/release. Once the SAME lane
  ;; crosses the 30-minute reap bar, retract only exact @<handle> driver refs.
  ;; A successor that won between query and retract has a different object and
  ;; is therefore untouched by the exact-value retraction.
  (let [driver-ref (str "@" h)
        threads (indexed-subjects-for "driver" driver-ref)]
    (doseq [thread threads]
      (north.coord/retract! port thread "driver" driver-ref))))

(defn sweep-unpublished-driver-claims! [dry?]
  ;; Claim is intentionally the first dispatch side effect. A hard kill before
  ;; identity publication therefore leaves no kind=lane row for sweep-lanes!.
  ;; Current SDK IDs encode a mint timestamp; after the same 30-minute bar, an
  ;; unpublished holder is unrecoverable and its exact driver ref can be retired.
  ;; Legacy/malformed IDs have no trusted clock and are never guessed at.
  (let [now (System/currentTimeMillis)
        lanes (->> (indexed-subjects-for "kind" "lane")
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
  (let [lanes (indexed-subjects-for "kind" "lane")
        now (System/currentTimeMillis)
        rows-by-subject (subject-fact-rows :coordination (conj lanes "@swarm"))
        online (online-session-handles now)
        offline-old
        (keep
         (fn [entity]
           (let [handle (strip-sigil entity "@agent:")
                 rows (get rows-by-subject entity [])
                 spawned (or (spawned-ms-from-rows rows)
                             (north.reap/sdk-agent-mint-ms handle))]
             (when (and (not (contains? online handle))
                        spawned
                        (>= (- now spawned) LANE-STALE-MS))
               (let [{:keys [online? exp]}
                     (north.coord/session-lease-status! port handle)]
                 (when (and (not online?)
                            (north.reap/reap-lane? now false exp spawned))
                   {:e entity :h handle :lease-exp exp :sp spawned
                    :rows rows})))))
         lanes)
        resolutions (lane-resolutions-batch (mapv :h offline-old))
        deaths (fact-values (get rows-by-subject "@swarm" []) "agent_death")
        hits
        (for [{:keys [e h lease-exp sp rows]} offline-old
              :let [resolution (get resolutions h
                                    {:status :indeterminate
                                     :reason :lane-facts-unavailable})]
              :when (north.reap/reap-lane?
                     now (not= :unresolved (:status resolution)) lease-exp sp)]
          {:e e :h h :rows rows
           :lapse (north.reap/lane-lapse-ms now lease-exp sp)})]
    (doseq [{:keys [e h lapse]} hits]
      (when-not dry?
        (publish-reaped-terminal! e)
        (release-orphaned-drivers! h)                                      ; unblock threads held by the hard-killed lane
        ;; The reaper is the ONLY terminal for dead-lane spend: settle any open
        ;; spend reservation at FULL (status unknown) — idempotent (settled_at guard).
        (try (north.spend-breaker/reap-settle-lane-reservations! port h false)
             (catch Throwable t (println (str "[sweep] spend reaper-settle error: " (.getMessage t)))))
        ;; Death is terminal evidence, not a mutation of identity/name caches.
        ;; Every UI derives its decoration from the committed process/delivery facts.
        (let [rows (get rows-by-subject e [])
              coord (or (fact-value rows "coordinator")
                        (fact-value rows "supervisor"))]
          (when (and coord (seq coord)
                     (not (north.reap/death-reported? h deaths)))
            (ping-coordinator coord h))))
      (println (str "[sweep] " (if dry? "WOULD reap" "reaped") " lane " e
                    "  lapsed " (long (/ lapse 60000))
                    "min -> process=died-unreported delivery=blocked")))
    (count hits)))

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
(defn run-worktree-task! [dry?]
  (let [registered
        (run-sweep-stage!
         :registered-worktrees
         #(north.worktree-janitor/sweep-worktrees!
           {:port port
            :dry? dry?
            :repo-filter sweep-repo
            :lane-resolved? lane-resolved?*}))
        unregistered
        (run-sweep-stage!
         :unregistered-worktrees
         #(north.worktree-janitor/sweep-unregistered-worktrees!
           {:dry? dry?
            :repo-filter sweep-repo
            :claimed-worktrees
            (delay
             (set (keys (north.worktree-census/claimed-worktrees port))))
            :live-concern-repos (delay (live-concern-repos))}))]
    {:registered registered :unregistered unregistered}))

(defn run-maintenance-task! [dry?]
  (let [result
        (case maintenance-task
          :stale-concerns
          (run-sweep-stage!
           :stale-concerns
           #(sweep-concerns! dry?))

          :stale-lanes
          {:lanes
           (run-sweep-stage! :stale-lanes #(sweep-lanes! dry?))
           :unpublished-drivers
           (run-sweep-stage!
            :unpublished-driver-claims
            #(sweep-unpublished-driver-claims! dry?))}

          :worktrees
          (run-worktree-task! dry?)

          :agent-logs
          (run-sweep-stage! :agent-logs #(sweep-agent-logs! dry?))

          :spend-guard
          {:breaker
           (run-sweep-stage!
            :spend-burn
            #(north.spend-breaker/sweep-burn! port dry?))
           :lanes-killed
           (run-sweep-stage!
            :spend-kill
            #(north.spend-breaker/sweep-kill! port dry?))})]
    (when-not dry?
      (north.worker-heartbeat/write-heartbeat!
       (name maintenance-task) port {:result result}))
    (println
     (str "[coordination-maintenance-task] task=" (name maintenance-task)
          (when dry? " dry-run=true")
          " result=" (pr-str result)))
    (flush)
    {:task maintenance-task
     :result result
     :terminal-status :completed}))

(defn sweep! [dry?]
  (run-maintenance-task! dry?))

;; ---- BOUNDED ONE-SHOT LIFECYCLE --------------------------------------------
;; Each task has its own timer, deadline, and lock. A coordinator bounce is
;; retryable within that task's budget and otherwise defers to its next cadence.
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
  (or (System/getenv "NORTH_MAINTENANCE_TASK_LOCK_PATH")
      (let [task-name (name maintenance-task)
            filename (str "north-" task-name
                          (when dry-run? "-dry-run")
                          ".lock")]
        (if-let [runtime-dir (System/getenv "XDG_RUNTIME_DIR")]
          (.getPath (io/file runtime-dir filename))
          (.getPath (io/file (System/getProperty "user.home")
                             ".cache" "north" filename))))))

(def retryable-store-rpc-types
  #{:rpc-truncated
    :query-cancelled
    :query-time-limit
    :query-work-limit
    :rpc/cancelled})

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
                 (contains? retryable-store-rpc-types (:type data)))))
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

(defn maintenance-task-exit-code []
  (try
    (let [default-timeout
          (north.worker-policy/task-timeout-ms maintenance-task)
          timeout-ms (positive-ms "NORTH_MAINTENANCE_TASK_TIMEOUT_MS"
                                  default-timeout MAX-SWEEP-TIMEOUT-MS)
          retry-ms (positive-ms "NORTH_MAINTENANCE_TASK_RETRY_MS"
                                (min DEFAULT-SWEEP-RETRY-MS timeout-ms) timeout-ms)
          result (with-sweep-lock
                   #(run-bounded-sweep! dry-run? timeout-ms retry-ms))]
      (cond
        (= :completed (:status result))
        (do
          (println
           (str "[coordination-maintenance-task] task="
                (name maintenance-task)
                " terminal=completed attempts=" (:attempts result)
                " elapsed_ms=" (:elapsed-ms result)))
          (flush)
          0)

        (= :already-running (:status result))
        (do (println (str "[coordination-maintenance-task] task="
                          (name maintenance-task)
                          " terminal=deferred reason=already-running"
                          " action=wait-for-current-task lock=" (:lock result)))
            (flush)
            0)

        (= :deferred (:status result))
        (do
          (println (str "[coordination-maintenance-task] task="
                        (name maintenance-task)
                        " terminal=deferred reason="
                        (name (or (:reason result) :maintenance))
                        " attempts=" (:attempts result)
                        " action=retry-on-next-scheduled-run"))
          (flush)
          0)

        (contains? #{:deadline :unavailable} (:status result))
        (do (println (str "[coordination-maintenance-task] task="
                          (name maintenance-task)
                          " terminal=deferred reason=" (name (:status result))
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
              (println (str "[coordination-maintenance-task] task="
                            (name maintenance-task)
                            " terminal=failed attempts=" (:attempts result)
                            " error=\"" (concise-error (:error result)) "\""
                            " action=inspect-task-journal"))
              (flush))
            1)))
    (catch Throwable throwable
      (binding [*out* *err*]
        (println (str "[coordination-maintenance-task] task="
                      (some-> maintenance-task name)
                      " terminal=failed error=\"" (concise-error throwable) "\""
                      " action=check-task-configuration"))
        (flush))
      1)))

(when-not
 (= "1" (System/getProperty "north.coordination-maintenance-task-host.lib"))
 (if (north.worker-policy/scheduled-task? maintenance-task)
   (System/exit (maintenance-task-exit-code))
   (do
     (binding [*out* *err*]
       (println
        "usage: coordination-maintenance-task-host.clj {stale-concerns|stale-lanes|worktrees|agent-logs|spend-guard} [--dry-run] [--repo PATH]"))
     (System/exit 2))))
