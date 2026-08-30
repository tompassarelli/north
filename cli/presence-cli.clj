;; presence-cli.clj — presence-as-facts (North gate-2 #30).
;;
;; Presence is a renewable Beagle Store lease. Liveness is judged by Beagle Store's clock,
;; never a self-stamped wall-clock heartbeat. This kills
;; agentchat's heartbeat clock-skew AND its separate reaper in one move: a dead
;; agent's lease simply lapses and online? flips false on its own.
;;
;; A live presence descriptor is @agent:<handle>; read-only rosters scan Beagle Store's
;; canonical :kernel/lease projection. Exact epochs stay with the owning SDK.
;;
;; usage:
;;   bb presence-cli.clj <port> register <handle> <dir> <session_id>
;;   bb presence-cli.clj <port> renew    <handle> <fence-json>        ; the new heartbeat
;;   bb presence-cli.clj <port> task     <handle> "<task>"
;;   bb presence-cli.clj <port> presence                             ; projection (replaces ls presence/ + age math)
;;   bb presence-cli.clj <port> presence-online                      ; bounded live-only projection for cockpit/roster
;;   bb presence-cli.clj <port> presence-online-json                 ; stable machine projection (never parse columns)
;;   bb presence-cli.clj <port> coordination-probe-json              ; doctor health probe: fence + write-readback + lease/lineage divergence
(require '[clojure.java.io :as io] '[clojure.string :as str]
         '[cheshire.core :as json])

(def TTL 1800000)         ; 30min lease; renewed on every tool call (PostToolUse hook)
;; Shared coordination substrate: canonical Store RPC reads, writes, and leases.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-contract.clj"))
(def append!  north.coord/append!)
(def put!     north.coord/put!)
(def retract! north.coord/retract!)
(def resolved north.coord/resolved!)

(def presence-agent-prefix "@agent:")
(def max-live-session-controls 256)
(def max-control-bytes 256)
(def max-safe-integer 9007199254740991)

(defn valid-control? [value]
  (north.message-contract/safe-handle? value max-control-bytes))

(defn presence-entity [handle]
  (str presence-agent-prefix handle))

(defn canonical-fence [value]
  (select-keys value [:resource :holder :epoch]))

(defn release-grant-best-effort! [port grant]
  (when (and (string? (:resource grant))
             (string? (:holder grant))
             (integer? (:epoch grant))
             (pos? (:epoch grant)))
    (try
      (north.coord/release-lease! port (canonical-fence grant))
      (catch Exception _ nil))))

(defn fence-json! [handle raw]
  (let [parsed
        (try
          (json/parse-string (str raw))
          (catch Exception error
            (throw (ex-info "liveness fence must be valid JSON"
                            {:type :invalid-liveness-fence}
                            error))))
        expected-resource (str "session:" handle)]
    (when-not (and (map? parsed)
                   (= #{"resource" "holder" "epoch"} (set (keys parsed)))
                   (= expected-resource (get parsed "resource"))
                   (= handle (get parsed "holder"))
                   (integer? (get parsed "epoch"))
                   (pos? (get parsed "epoch"))
                   (<= (get parsed "epoch") max-safe-integer))
      (throw (ex-info "liveness fence does not match its session"
                      {:type :invalid-liveness-fence :handle handle})))
    {:resource expected-resource
     :holder handle
     :epoch (get parsed "epoch")}))

(defn print-fence! [fence]
  (println
   (json/generate-string
    (array-map "resource" (:resource fence)
               "holder" (:holder fence)
               "epoch" (:epoch fence)))))

(defn subject-values [rows]
  (reduce (fn [values [predicate value]]
            (update values predicate (fnil conj #{}) value))
          {}
          rows))

(defn replace-subject-facts! [port subject desired]
  (loop [remaining 8]
    (let [{:keys [version rows]} (north.coord/show-envelope! port subject)
          current (subject-values rows)
          actions
          (vec
           (concat
            (mapcat (fn [[predicate desired-value]]
                      (for [old-value (get current predicate #{})
                            :when (not= old-value desired-value)]
                        {:op :retract :subject subject
                         :predicate predicate :value old-value}))
                    desired)
            (keep (fn [[predicate desired-value]]
                    (when-not (contains? (get current predicate #{}) desired-value)
                      {:op :assert :subject subject
                       :predicate predicate :value desired-value}))
                  desired)))
          result (if (seq actions)
                   (north.coord/transact! port actions {:expected-version version})
                   {:ok version :changed? false})]
      (cond
        (nil? (:reject result)) result
        (and (= :conflict (:reject result)) (> remaining 1)) (recur (dec remaining))
        :else (throw (ex-info "liveness lease projection transaction failed"
                              {:type :lease-write-rejected
                               :subject subject :reject (:reject result)}))))))

(defn remove-subject-facts! [port subject predicates]
  (loop [remaining 8]
    (let [{:keys [version rows]} (north.coord/show-envelope! port subject)
          current (subject-values rows)
          actions (vec (for [predicate predicates
                             value (get current predicate #{})]
                         {:op :retract :subject subject
                          :predicate predicate :value value}))
          result (if (seq actions)
                   (north.coord/transact! port actions {:expected-version version})
                   {:ok version :changed? false})]
      (cond
        (nil? (:reject result)) result
        (and (= :conflict (:reject result)) (> remaining 1)) (recur (dec remaining))
        :else (throw (ex-info "liveness lease removal transaction failed"
                              {:type :lease-write-rejected
                               :subject subject :reject (:reject result)}))))))

(defn session-grant! [handle grant]
  (when-not (and (= (str "session:" handle) (:resource grant))
                 (= handle (:holder grant))
                 (integer? (:epoch grant)) (pos? (:epoch grant))
                 (<= (:epoch grant) max-safe-integer)
                 (integer? (:exp grant)) (pos? (:exp grant))
                 (<= (:exp grant) max-safe-integer))
    (throw (ex-info "Beagle Store returned an invalid session lease grant"
                    {:type :invalid-liveness-grant :handle handle})))
  (canonical-fence grant))

(defn presence-registrations
  "Return coordination-owned presence descriptors. Historical @session rows
   intentionally do not enter this projection."
  [port]
  (let [rows (north.coord/query-rows!
              port {:find "presence"
                    :rules [{:head {:rel "presence"
                                    :args [{:var "e"} {:var "h"}]}
                             :body [{:rel "triple"
                                     :args [{:var "e"} "agent" {:var "h"}]}]}]})
        registrations
        (->> rows
             (filter (fn [[entity _]]
                       (str/starts-with? entity presence-agent-prefix)))
             (mapv (fn [[entity handle]]
                     (when-not (and (valid-control? handle)
                                    (= entity (presence-entity handle)))
                       (throw (ex-info "lease descriptor does not match its control"
                                       {:type :malformed-lease-control
                                        :entity entity})))
                     [entity handle])))]
    (when-not (= (count registrations)
                 (count (set (map second registrations))))
      (throw (ex-info "coordinator returned duplicate lease descriptors"
                      {:type :duplicate-lease-descriptor})))
    registrations))

(defn online-sessions
  "Return registered sessions whose canonical kernel lease is unexpired."
  [port now]
  (let [registered (set (map second (presence-registrations port)))
        live (north.coord/online-session-leases! port now)]
    (when (> (count live) max-live-session-controls)
      (throw (ex-info "live session roster exceeds its bounded control set"
                      {:controls (count live) :max max-live-session-controls})))
    (->> live
         (keep (fn [{:keys [handle exp] :as lease}]
                 (when-not (and (valid-control? handle)
                                (integer? exp) (pos? exp)
                                (<= exp max-safe-integer))
                   (throw (ex-info "malformed canonical session lease"
                                   {:type :malformed-liveness-lease
                                    :lease lease})))
                 (when (contains? registered handle)
                   {:entity (presence-entity handle)
                    :handle handle :lease {:exp exp}})))
         (sort-by :handle)
         vec)))

(defn print-presence!
  [port now session-rows]
  (let [enriched (mapv (fn [{:keys [entity handle lease]}]
                         (let [ae (str "@agent:" handle)
                               status (when-not lease
                                        (north.coord/session-lease-status! port handle))
                               l (or lease (when (:online? status) {:exp (:exp status)}))
                               on (boolean (and l (> (:exp l) now)))
                               pinned (= "true" (resolved port ae "pinned"))
                               exp (if (and l on) (str (int (/ (- (:exp l) now) 1000)) "s") "lapsed")
                               rs (north.coord/many! port ae "holds")
                               resp (if (seq rs) (str/join "," (map #(subs % 6) (sort rs))) "-")
                               focus (or (resolved port entity "active_workflow")
                                         (resolved port entity "current_thread")
                                         (resolved port entity "task") "-")]
                           {:h handle :on on :pinned pinned :exp exp :roles resp :focus focus}))
                       session-rows)
        sorted (sort-by (fn [r] [(not (:pinned r)) (not (:on r)) (:h r)]) enriched)]
    (println (format "%-14s %-4s %-6s %-7s %-26s %s" "AGENT" "PIN" "ONLINE" "EXPIRES" "ROLES" "FOCUS"))
    (doseq [r sorted]
      (println (format "%-14s %-4s %-6s %-7s %-26s %s"
                       (:h r) (if (:pinned r) " *" "") (if (:on r) "yes" "no") (:exp r) (:roles r) (:focus r))))))

(defn print-presence-json!
  [now session-rows]
  (println
   (json/generate-string
    {"version" "north:live-leases:v1"
     "sessions"
     (mapv (fn [{:keys [handle lease]}]
             {"control_id" handle
              "online" true
              "expires_s" (max 0 (quot (- (:exp lease) now) 1000))})
           session-rows)})))

;; ---- coordination health probe ----------------------------------------------
;; MUST be invoked the way the hooks are (direct bb, unwrapped env) or it cannot
;; observe a hook-path fence fault.
(def probe-lease-resource "doctor-probe:liveness")   ; not session:* — never joins the roster
(def probe-lease-ttl-ms 5000)
(def max-lineage-rows 4096)

(defn- probe-fence [port]
  (let [status (north.coord/status! port)]
    {"space_id" (:space-id status)
     "space_fence_ok" (string? (:space-id status))}))

(defn- probe-write-readback [port]
  ;; Registration is a WRITE. Reading the registry cannot prove a hook can renew.
  ;; A rejected write throws here; that is a FALSE readback, never a probe error,
  ;; or the fence diagnosis below is lost behind a generic exception.
  (try
    (let [holder (str "doctor-" (System/currentTimeMillis))
          granted (north.coord/acquire-lease!
                   port probe-lease-resource holder probe-lease-ttl-ms)
          fence (canonical-fence granted)]
      (try
        (boolean (:valid? (north.coord/check-lease! port fence)))
        (finally (north.coord/release-lease! port fence))))
    (catch Exception _ false)))

(defn- lineage-registrations-within [port window-ms now]
  (let [rows (north.coord/query-rows!
              port {:find "s"
                    :rules [{:head {:rel "s" :args [{:var "e"} {:var "v"}]}
                             :body [{:rel "triple"
                                     :args [{:var "e"} "started_at" {:var "v"}]}]}]})]
    (when (> (count rows) max-lineage-rows)
      (throw (ex-info "session lineage projection exceeds its bound" {})))
    (count
     (filter (fn [[e v]]
               (and (string? e) (str/starts-with? e presence-agent-prefix)
                    (try (< (- now (.toEpochMilli (java.time.Instant/parse v))) window-ms)
                         (catch Exception _ false))))
             rows))))

(defn- exception-message [e]
  (or (not-empty (str (.getMessage e))) (.getName (class e))))

(defn print-coordination-probe-json! [port now]
  ;; The fence verdict is reported even when everything downstream of a broken
  ;; fence fails: a diagnosis lost inside a generic exception is the defect.
  (let [fence (try (probe-fence port) (catch Exception e {"error" (exception-message e)}))
        base (merge {"version" "north:coordination-probe:v1"} fence)
        base (if (contains? base "error")
               base
               (assoc base
                      "space_fence_ok" (true? (get fence "space_fence_ok"))
                      "lease_write_readback_ok" (probe-write-readback port)))
        payload
        (merge base
               (try {"live_session_leases" (count (online-sessions port now))
                     "lineage_registrations_in_ttl" (lineage-registrations-within port TTL now)
                     "lease_ttl_ms" TTL}
                    (catch Exception e {"error" (exception-message e)})))]
    (println (json/generate-string payload))
    (when (or (contains? payload "error")
              (not (get payload "space_fence_ok"))
              (not (get payload "lease_write_readback_ok")))
      (System/exit 1))))

;; The hook throttle advances only on a zero exit, so a lease that did not land
;; must exit nonzero — a printed :reject with rc=0 reads as a renewed lease.
(defn acquire-session-lease! [port handle registration]
  (let [grant (north.coord/acquire-lease!
               port (str "session:" handle) handle TTL)
        validated-fence (session-grant! handle grant)
        fence (try
                (replace-subject-facts!
                 port (presence-entity handle) registration)
                validated-fence
                (catch Exception error
                  (release-grant-best-effort! port grant)
                  (throw error)))]
    (print-fence! fence)
    fence))

(defn renew-session-lease! [port handle fence]
  (let [grant (north.coord/renew-lease! port fence TTL)
        next-fence (session-grant! handle grant)]
    (print-fence! next-fence)
    next-fence))

(try
  (let [[port verb & args] *command-line-args*
        port (Integer/parseInt port)
        now  (System/currentTimeMillis)]      ; same machine as coord -> agent-now ~ coord-now
    (case verb
    "register"
    (let [[h dir sid] args
          _ (when-not (= 3 (count args))
              (throw (ex-info "register requires handle, dir, and session id"
                              {:type :invalid-presence-registration})))
          _ (when-not (valid-control? h)
              (throw (ex-info "presence handle is malformed or too large"
                              {:type :invalid-presence-registration})))
          se (presence-entity h)
          sid (str (or sid "?"))
          prior-sid (resolved port se "session_id")
          prior-started (resolved port se "started_at")
          started-at (if (and (= sid prior-sid) prior-started)
                       prior-started
                       (str (java.time.Instant/now)))]
      (acquire-session-lease!
       port h {"agent" h
               "dir" (or dir "?")
               "session_id" sid
               "started_at" started-at}))

    "renew"
    (let [[h raw-fence] args]
      (renew-session-lease! port h (fence-json! h raw-fence)))

    "task"
    (let [[h t] args] (prn (put! port (presence-entity h) "task" t)))   ; single

    ;; ===========================================================================
    ;; AGENT REGISTRY. Handle is an opaque uuid (an ADDRESS, never a name). Identity is a
    ;; COLLECTION OF ROLES (@role:<slug> facts), each exclusive (one holder,
    ;; lease-enforced) or inclusive (shared). You ADDRESS a role (routes to its
    ;; holder) or a uuid; the uuid is just the non-colliding instance id.
    ;; ===========================================================================
    "identify"                              ; <uuid> [model] [effort] [context_tokens] [lifecycle] [supervisor]
    ;; NOTE: these agent-card fields are registry-single (one value per agent) and the
    ;; intended semantics is last-writer-wins, hence put!. They are not yet in the
    ;; engine's BEAGLE_STORE_SINGLE_VALUED set, so the engine still treats them as multi — put!
    ;; here is presently wire-identical to a bare append; the LWW becomes native once
    ;; thread B folds these into the engine cardinality FACT. Verb names the intent.
    (let [[h model effort ctx life sup] args, ae (str "@agent:" h)]
      (north.topology-authority/require-self-agent! "identify peer agent" h)
      (when (and (resolved port ae "identity_manifest_sha256")
                 (or (and model (seq model)) (and effort (seq effort))))
        (throw (ex-info "managed lane route identity is publisher-owned; lease identify may not rewrite model/effort"
                        {:north/authority-denied true :agent h})))
      (when (and model  (seq model))  (put! port ae "model" model))           ; single
      (when (and effort (seq effort)) (put! port ae "effort" effort))         ; single
      (when (and ctx    (seq ctx))    (put! port ae "context_tokens" ctx))    ; single
      (put! port ae "lifecycle" (or life "standing"))                         ; single
      (when (and sup (seq sup)) (put! port ae "supervisor" sup))              ; single
      (prn {:agent ae :model model :effort effort :lifecycle (or life "standing")}))

    "card"                                  ; <uuid>  — the agent card + held roles
    (let [[h] args, ae (str "@agent:" h)]
      (doseq [p ["model" "effort" "context_tokens" "lifecycle" "supervisor"]]
        (println (format "%-15s %s" p (or (resolved port ae p) "-"))))
      (let [rs (north.coord/many! port ae "holds")]
        (println (format "%-15s %s" "roles" (if (seq rs) (str/join ", " (map #(subs % 6) (sort rs))) "-"))))
      (let [ws (north.coord/many! port ae "watches")]
        (println (format "%-15s %s" "watches" (if (seq ws) (str/join ", " (sort ws)) "-")))))

    "define-role"                           ; <slug> <exclusive|inclusive> "<title>"  — register a role
    (let [[slug excl title] args, re (str "@role:" slug)]
      (put! port re "title" (or title slug))             ; single
      (put! port re "exclusivity" (or excl "inclusive")) ; single
      (prn {:role re :exclusivity (or excl "inclusive")}))

    ;; assign/unassign — COEXIST-ELECT, no lease. A role holder is graph-internal,
    ;; so it collapses onto coexist-elect. `holds` is MULTI, so rival assigns to an exclusive role BOTH land
    ;; (no block, no refusal); the single true holder is ELECTED at read time (earliest holder
    ;; wins — `holders` lists them in election order). A loser sees it lost on its next read and
    ;; yields — dup is cheaper than coordination. (Lease survives only for EXTERNAL resources.)
    "assign"                                ; <uuid> <slug>  — agent takes a role (coexist-elect)
    (let [[h slug] args]
      (north.topology-authority/require-self-agent! "assign peer agent" h)
      (let [ae (str "@agent:" h), re (str "@role:" slug)
            excl (resolved port re "exclusivity")
            prior (->> (north.coord/query-rows!
                        port {:find "a"
                              :rules [{:head {:rel "a" :args [{:var "a"}]}
                                       :body [{:rel "triple" :args [{:var "a"} "holds" re]}]}]})
                       (mapv first) (remove #(= ae %)) vec)]
        (append! port ae "holds" re)          ; coexist — both land, no lease
        (if (= excl "exclusive")
          (prn {:assigned re :to h :exclusive true :coexist true
                :prior-holders prior
                :note "exclusive resolved by coexist-elect (earliest holder wins; see `holders`)"})
          (prn {:assigned re :to h :exclusive false}))))

    "unassign"                              ; <uuid> <slug>  — drop the holds fact (no lease)
    (let [[h slug] args, ae (str "@agent:" h), re (str "@role:" slug)]
      (north.topology-authority/require-self-agent! "unassign peer agent" h)
      (retract! port ae "holds" re)
      (prn {:unassigned re :from h}))

    "roles"                      ; <uuid>  — what this agent holds
    (let [[h] args, ae (str "@agent:" h)
          rs (north.coord/many! port ae "holds")]
      (doseq [r (sort rs)]
        (println (format "%-22s %-10s %s" (subs r 6) (or (resolved port r "exclusivity") "?") (or (resolved port r "title") "")))))

    "holders"                               ; <slug>  — which agents hold this role (reverse edge)
    (let [[slug] args, re (str "@role:" slug)
          hs (north.coord/query-rows!
              port {:find "a"
                    :rules [{:head {:rel "a" :args [{:var "a"}]}
                             :body [{:rel "triple" :args [{:var "a"} "holds" re]}]}]})]
      (println (str "@role:" slug " (" (or (resolved port re "exclusivity") "?") ") held by:"))
      (doseq [row (or hs [])] (println "  " (first row))))

    "focus"                                 ; <uuid> <current_thread> [active_workflow] — VOLATILE, on the session
    (let [[h ct wf] args, se (presence-entity h)]
      (put! port se "current_thread" (or ct "-"))   ; single (LWW intent; see identify note)
      (when wf (put! port se "active_workflow" wf))  ; single
      (prn {:focus se :current_thread ct :active_workflow wf}))

    "roster"                                ; agents + held roles + focus. Pinned first, then online, then rest.
    (print-presence! port now (mapv (fn [[entity handle]] {:entity entity :handle handle})
                                    (presence-registrations port)))

    "live-leases"                           ; bounded projection used by live-only UIs
    (print-presence! port now (online-sessions port now))

    "live-leases-json"                      ; stable bounded machine projection
    (print-presence-json! now (online-sessions port now))

    "coordination-probe-json"               ; doctor's honest health signal; exits 1 when broken
    (print-coordination-probe-json! port now)

    "pin"                                   ; <uuid> [reason]  — mark agent as important (surfaces first in roster)
    (let [[h & reason-parts] args, ae (str "@agent:" h)]
      (put! port ae "pinned" "true")    ; single (flag; LWW intent)
      (when (seq reason-parts) (put! port ae "pin_reason" (str/join " " reason-parts)))  ; single
      (prn {:pinned ae}))

    "unpin"                                 ; <uuid>  — remove pin
    (let [[h] args, ae (str "@agent:" h)]
      (retract! port ae "pinned" "true")
      (prn {:unpinned ae}))

    "stale"                                 ; composite staleness: idle time + generation + playbook drift
    (let [;; playbook learning count (from :7977) — how many learnings exist now
          playbook-count (try (count (north.coord/many! 7977 "@2026-06-22-232740" "learning"))
                              (catch Exception _ 0))
          ss (presence-registrations port)]
      (println (format "%-14s %-5s %5s %4s %4s %-7s %-4s %s"
                       "AGENT" "SCORE" "IDLE" "GEN" "PBOK" "BUCKET" "PIN" "ROLES"))
      (doseq [[_e h] (sort-by second ss)]
        (let [ae (str "@agent:" h)
              pinned (= "true" (resolved port ae "pinned"))
              last-run (resolved port ae "last_run_at")
              gen-s (resolved port ae "generation")
              gen (or (when gen-s (parse-long gen-s)) 0)
              boot-playbook-s (resolved port ae "playbook_count_at_boot")
              boot-playbook (or (when boot-playbook-s (parse-long boot-playbook-s)) 0)
              playbook-drift (if (pos? playbook-count)
                               (/ (double (- playbook-count boot-playbook)) playbook-count)
                               0.0)
              idle-h (when last-run
                       (try (/ (- now (.toEpochMilli (java.time.Instant/parse last-run))) 3600000.0)
                            (catch Exception _ nil)))
              idle-score (if idle-h (min 1.0 (/ idle-h 24.0)) 0.5)
              gen-score (min 1.0 (/ (double gen) 5.0))
              score (+ (* 0.4 idle-score) (* 0.35 gen-score) (* 0.25 playbook-drift))
              bucket (cond pinned "PINNED" (< score 0.3) "GREEN" (< score 0.7) "YELLOW" :else "RED")
              rs (north.coord/many! port ae "holds")
              resp (if (seq rs) (str/join "," (map #(subs % 6) (sort rs))) "-")]
          (println (format "%-14s %5.2f %5s %4d %4d %-7s %-4s %s"
                           h score
                           (if idle-h (format "%.0fh" idle-h) "?")
                           gen
                           (- playbook-count boot-playbook)
                           bucket
                           (if pinned "*" "")
                           resp)))))

    "staleness"                             ; <uuid>  — single agent staleness detail + dispatch recommendation
    (let [[h] args
          ae (str "@agent:" h)
          pinned (= "true" (resolved port ae "pinned"))
          last-run (resolved port ae "last_run_at")
          gen-s (resolved port ae "generation")
          gen (or (when gen-s (parse-long gen-s)) 0)
          spawn-at (resolved port ae "spawned_at")
          model (resolved port ae "model")
          lifecycle (resolved port ae "lifecycle")
          prev-input (resolved port ae "prev_input_tokens")
          playbook-count (try (count (north.coord/many! 7977 "@2026-06-22-232740" "learning"))
                              (catch Exception _ 0))
          boot-playbook-s (resolved port ae "playbook_count_at_boot")
          boot-playbook (or (when boot-playbook-s (parse-long boot-playbook-s)) 0)
          playbook-drift (- playbook-count boot-playbook)
          idle-h (when last-run
                   (try (/ (- now (.toEpochMilli (java.time.Instant/parse last-run))) 3600000.0)
                        (catch Exception _ nil)))
          idle-score (if idle-h (min 1.0 (/ idle-h 24.0)) 0.5)
          gen-score (min 1.0 (/ (double gen) 5.0))
          pb-score (if (pos? playbook-count) (/ (double playbook-drift) playbook-count) 0.0)
          score (+ (* 0.4 idle-score) (* 0.35 gen-score) (* 0.25 pb-score))
          bucket (cond pinned "PINNED" (< score 0.3) "GREEN" (< score 0.7) "YELLOW" :else "RED")]
      (println (format "%-18s %s" "agent" ae))
      (println (format "%-18s %s" "pinned" (if pinned "YES" "no")))
      (println (format "%-18s %s" "lifecycle" (or lifecycle "?")))
      (println (format "%-18s %s" "model" (or model "?")))
      (println (format "%-18s %s" "spawned_at" (or spawn-at "?")))
      (println (format "%-18s %s" "last_run_at" (or last-run "?")))
      (println (format "%-18s %s" "idle" (if idle-h (format "%.1f hours" idle-h) "unknown")))
      (println (format "%-18s %d" "generation" gen))
      (println (format "%-18s %s" "prev_input_tokens" (or prev-input "?")))
      (println (format "%-18s %d new since boot" "playbook_drift" playbook-drift))
      (println (format "%-18s %.2f" "staleness_score" score))
      (println (format "%-18s %s" "BUCKET" bucket))
      (println)
      (case bucket
        "PINNED" (println "DISPATCH: reuse (pinned — user trusts this context)")
        "GREEN"  (println "DISPATCH: reuse (fresh context)")
        "YELLOW" (println "DISPATCH: reuse with caution — inject rehydration hint")
        "RED"    (println
                  (str "DISPATCH: REPLACE — coordinator should delegate a fresh "
                       "managed lane with an explicit context brief; do not reuse "
                       h "."))))

    "forget"
    (let [[h raw-fence] args
          fence (fence-json! h raw-fence)
          released (north.coord/release-lease! port fence)]
      (when-not (:released? released)
        (throw (ex-info "liveness lease was not released"
                        {:type :lease-release-failed :handle h})))
      (remove-subject-facts!
       port (presence-entity h)
       ["agent" "dir" "session_id" "started_at" "task"
        "current_thread" "active_workflow"])
      (println (json/generate-string {"released" true})))

    "runmeta"                               ; <uuid> <session_id> <json>  — full per-run telemetry tuple
    (let [[h sid json-str] args
          re (str "@run:" sid)
          m (json/parse-string json-str true)]
      (put! port re "kind" "run")                  ; single; canonical run discovery
      (put! port re "agent" h)                     ; single (write-once on a fresh @run)
      (put! port re "ended_at" (str (java.time.Instant/now)))  ; single
      (doseq [[k v] m :when (some? v)]
        (append! port re (name k) (str v)))        ; DYNAMIC pred -> append! (safe default)
      (prn {:recorded re :agent h :fields (count m)}))

    ;; --- subscriptions: thread-watches as facts (consumed by north-listen.clj) ---
    ;; subject = the agent's self node @<handle> (its self-reference channel is implicit; this
    ;; ADDS threads beyond it). multi-valued: an agent watches many threads.
    "watch"                                 ; <uuid> <thread-ref>  — subscribe to a thread
    (let [[h t] args]
      (north.topology-authority/require-self-agent! "watch for peer agent" h)
      (prn (append! port (str "@agent:" h) "watches" t)))   ; multi (watches many threads)

    "unwatch"                               ; <uuid> <thread-ref>  — drop a subscription
    (let [[h t] args]
      (north.topology-authority/require-self-agent! "unwatch for peer agent" h)
      (prn (retract! port (str "@agent:" h) "watches" t)))

    "subscriptions"                         ; <uuid>  — channel = uuid ∪ held roles ∪ watched threads
    (let [[h] args, ae (str "@agent:" h)
          rs (north.coord/many! port ae "holds")
          ws (north.coord/many! port ae "watches")]
      (println (str "@agent:" h " self-channel: to ∈ {" h ", "
                    (str/join ", " (map #(subs % 6) (sort rs))) ", *}  (uuid ∪ held-roles)"))
      (doseq [t (sort ws)] (println (str "  watches " t))))

    (do (println "usage: north lease-internal <port> {register|renew|task|focus|forget|runmeta  (session/run)")
        (println "                                      |identify|card  (agent card)")
        (println "                                      |define-role|assign|unassign|roles|holders  (roles)")
        (println "                                      |watch|unwatch|subscriptions  (thread subs)")
        (println "                                      |roster|live-leases|live-leases-json}  (projections)")
        (System/exit 2))))
  (catch Exception error
    (binding [*out* *err*]
      (println (str "north lease-internal: " (.getMessage error))))
    (System/exit 2)))
