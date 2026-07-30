;; presence-cli.clj — presence-as-facts (North gate-2 #30).
;;
;; THE TRICK: presence = a renewable LEASE. Liveness is judged by the COORDINATOR's
;; clock (the lease expiry), never a self-stamped wall-clock heartbeat. This kills
;; agentchat's heartbeat clock-skew AND its separate reaper in one move: a dead
;; agent's lease simply lapses and online? flips false on its own.
;;
;; Sibling to lease-cli.clj. Wire (daemon b619283): :assert/:version/:acquire-lease/:resolved/:query.
;; A live presence descriptor is @agent:<handle> in the coordination origin;
;; liveness = lease on resource session:<handle> -> fact
;; @lease:session:<handle> = "holder|exp|epoch". Historical @session:<handle>
;; telemetry descriptors remain readable history, but never participate in the
;; live roster.
;;
;; usage:
;;   bb presence-cli.clj <port> register <handle> <dir> <session_id>
;;   bb presence-cli.clj <port> renew    <handle>                     ; the new heartbeat
;;   bb presence-cli.clj <port> task     <handle> "<task>"
;;   bb presence-cli.clj <port> presence                             ; projection (replaces ls presence/ + age math)
;;   bb presence-cli.clj <port> presence-online                      ; bounded live-only projection for cockpit/roster
;;   bb presence-cli.clj <port> presence-online-json                 ; stable machine projection (never parse columns)
;;   bb presence-cli.clj <port> coordination-probe-json              ; doctor health probe: fence + write-readback + lease/lineage divergence
;;   bb presence-cli.clj <port> slackers [minutes]                   ; derived: online + holds no work-lease
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[cheshire.core :as json])

(def TTL 1800000)         ; 30min lease; renewed on every tool call (PostToolUse hook)
;; shared coord substrate: the cardinality-typed write verbs (move-C) live once in
;; cli/coord.clj. append! = MULTI coexist; put! = SINGLE last-writer-wins.
;; decode-lease/lease-of/online? — the renewable-lease liveness rule — ALSO live there
;; now, so this roster and concern-cli judge "online" by the exact same definition.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(def send-op  north.coord/send-op)
(def append!  north.coord/append!)
(def put!     north.coord/put!)
(def retract! north.coord/retract!)
(def resolved north.coord/resolved)
(def decode-lease north.coord/decode-lease)
(def lease-of     north.coord/lease-of)

(declare strict-query-rows)

(def lease-session-prefix "@lease:session:")
(def presence-agent-prefix "@agent:")
(def max-session-lease-rows 100000)
(def max-live-session-controls 256)
(def max-control-bytes 256)
(def max-safe-integer 9007199254740991)
(def control-pattern #"^[A-Za-z0-9][A-Za-z0-9._:-]*$")

(defn valid-control? [value]
  (and (string? value)
       (<= (alength (.getBytes value java.nio.charset.StandardCharsets/UTF_8))
           max-control-bytes)
       (boolean (re-matches control-pattern value))))

(defn presence-entity [handle]
  (str presence-agent-prefix handle))

(defn strict-query-rows
  "Return an exact coordinator query result or throw. A transport/protocol
   failure is not an empty graph: callers must never publish a successful empty
   roster after the coordinator failed or returned a malformed row."
  [response]
  (when-not (and (map? response)
                 (= #{:ok :version :engine} (set (keys response)))
                 (integer? (:version response))
                 (not (neg? (:version response)))
                 (<= (:version response) max-safe-integer)
                 (#{"index" "scan"} (:engine response))
                 (vector? (:ok response))
                 (every? #(and (vector? %)
                               (= 2 (count %))
                               (string? (nth % 0))
                               (string? (nth % 1)))
                         (:ok response)))
    (throw (ex-info "coordinator returned a malformed presence projection"
                    {:type :malformed-presence-query
                     :response response})))
  (:ok response))

(defn presence-registrations
  "Return coordination-owned presence descriptors. Historical @session rows
   intentionally do not enter this projection."
  [port]
  (let [rows (strict-query-rows
              (send-op port {:op :query
                             :query {:find "presence"
                                     :rules [{:head {:rel "presence"
                                                     :args [{:var "e"} {:var "h"}]}
                                              :body [{:rel "triple"
                                                      :args [{:var "e"} "agent"
                                                             {:var "h"}]}]}]}}))
        registrations
        (->> rows
             (filter (fn [[entity _]]
                       (str/starts-with? entity presence-agent-prefix)))
             (mapv (fn [[entity handle]]
                     (when-not (and (valid-control? handle)
                                    (= entity (presence-entity handle)))
                       (throw (ex-info "presence descriptor does not match its control"
                                       {:type :malformed-presence-control
                                        :entity entity})))
                     [entity handle])))]
    (when-not (= (count registrations)
                 (count (set (map second registrations))))
      (throw (ex-info "coordinator returned duplicate presence descriptors"
                      {:type :duplicate-presence-descriptor})))
    registrations))

(defn strict-lease
  [entity value]
  (let [lease (north.coord/decode-lease value)]
    (when-not (north.coord/authoritative-lease? lease)
      (throw (ex-info "coordinator returned a malformed lease value"
                      {:type :malformed-presence-lease
                       :entity entity})))
    lease))

(defn online-sessions
  "Return only unexpired session leases using one indexed graph query. The full
   historical session registry contains thousands of lapsed rows, so enriching
   all of it and filtering later makes live rosters grow without bound."
  [port now]
  (let [rows (strict-query-rows
              (send-op port {:op :query
                             :query {:find "lease"
                                     :rules [{:head {:rel "lease" :args [{:var "e"} {:var "v"}]}
                                              :body [{:rel "triple" :args [{:var "e"} "lease" {:var "v"}]}]}]}}))]
    (when (> (count rows) max-session-lease-rows)
      (throw (ex-info "live session lease projection exceeds its bounded input"
                      {:rows (count rows) :max max-session-lease-rows})))
    (let [parsed (mapv (fn [[entity value]]
                         {:entity entity
                          :lease (strict-lease entity value)})
                       rows)
          session-entities (->> parsed
                                (map :entity)
                                (filter #(str/starts-with? % lease-session-prefix))
                                vec)]
      (when-not (= (count session-entities) (count (distinct session-entities)))
        (throw (ex-info "coordinator returned duplicate session lease rows"
                        {:type :duplicate-presence-lease})))
      (let [live
            (->> parsed
                 (keep (fn [{:keys [entity lease]}]
                         (when (and (str/starts-with? entity lease-session-prefix)
                                    (> (:exp lease) now))
                           (let [handle (subs entity (count lease-session-prefix))]
                             (when-not (= handle (:holder lease))
                               (throw (ex-info "session lease holder does not match its control"
                                               {:type :malformed-presence-lease
                                                :entity entity})))
                             (when-not (valid-control? handle)
                               (throw (ex-info "session lease control is malformed"
                                               {:type :malformed-presence-control
                                                :entity entity})))
                             {:entity (presence-entity handle)
                              :handle handle
                              :lease lease}))))
                 vec)]
        (when (> (count live) max-live-session-controls)
          (throw (ex-info "live session roster exceeds its bounded control set"
                          {:controls (count live) :max max-live-session-controls})))
        (->> live (sort-by :handle) vec)))))

(defn print-presence!
  [port now session-rows]
  (let [enriched (mapv (fn [{:keys [entity handle lease]}]
                         (let [ae (str "@agent:" handle)
                               l (or lease (lease-of port (str "session:" handle)))
                               on (boolean (and l (> (:exp l) now)))
                               pinned (= "true" (resolved port ae "pinned"))
                               exp (if (and l on) (str (int (/ (- (:exp l) now) 1000)) "s") "lapsed")
                               rs (:values (send-op port {:op :resolved :te ae :p "holds"}))
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
    {"version" "north:presence-online:v1"
     "sessions"
     (mapv (fn [{:keys [handle lease]}]
             {"control_id" handle
              "online" true
              "expires_s" (max 0 (quot (- (:exp lease) now) 1000))})
           session-rows)})))

;; ---- coordination health probe ----------------------------------------------
;; MUST be invoked the way the hooks are (direct bb, unwrapped env) or it cannot
;; observe a hook-path fence fault.
(def probe-lease-resource "doctor-probe:presence")   ; not session:* — never joins the roster
(def probe-lease-ttl-ms 5000)
(def max-lineage-rows 4096)

(defn- probe-fence [port]
  (let [expected (north.coord/expected-log)
        raw (north.coord/send-raw-op port {:op :version})
        served (:served-log raw)]
    {"expected_log" expected
     "served_log" (when (string? served) (.getCanonicalPath (io/file served)))}))

(defn- probe-write-readback [port]
  ;; Registration is a WRITE. Reading the registry cannot prove a hook can renew.
  ;; A rejected write throws here; that is a FALSE readback, never a probe error,
  ;; or the fence diagnosis below is lost behind a generic exception.
  (try
    (let [holder (str "doctor-" (System/currentTimeMillis))
          granted (send-op port {:op :acquire-lease :res probe-lease-resource
                                 :holder holder :ttl-ms probe-lease-ttl-ms})
          back (resolved port (str "@lease:" probe-lease-resource) "lease")]
      (boolean (and (map? granted) (nil? (:reject granted)) (integer? (:exp granted))
                    (string? back) (str/starts-with? back (str holder "|")))))
    (catch Exception _ false)))

(defn- lineage-registrations-within [port window-ms now]
  ;; Registration and the liveness marker share the coordination origin. Legacy
  ;; telemetry session descriptors are history and cannot satisfy this probe.
  (let [resp (north.coord/query-page-in-domain
              port :coordination
              {:find "s" :rules [{:head {:rel "s" :args [{:var "e"} {:var "v"}]}
                                  :body [{:rel "triple"
                                          :args [{:var "e"} "started_at" {:var "v"}]}]}]}
              max-lineage-rows nil)
        rows (:ok resp)]
    (when-not (and (vector? rows) (false? (:more resp)))
      (throw (ex-info "session lineage projection was truncated or malformed" {})))
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
                      "log_fence_ok" (= (get fence "expected_log") (get fence "served_log"))
                      "lease_write_readback_ok" (probe-write-readback port)))
        payload
        (merge base
               (try {"live_session_leases" (count (online-sessions port now))
                     "lineage_registrations_in_ttl" (lineage-registrations-within port TTL now)
                     "lease_ttl_ms" TTL}
                    (catch Exception e {"error" (exception-message e)})))]
    (println (json/generate-string payload))
    (when (or (contains? payload "error")
              (not (get payload "log_fence_ok"))
              (not (get payload "lease_write_readback_ok")))
      (System/exit 1))))

;; The hook throttle advances only on a zero exit, so a lease that did not land
;; must exit nonzero — a printed :reject with rc=0 reads as a renewed lease.
(defn acquire-session-lease! [port h]
  (let [reply (send-op port {:op :acquire-lease :res (str "session:" h) :holder h :ttl-ms TTL})]
    (prn reply)
    (when-not (and (map? reply)
                   (nil? (:reject reply))
                   (= h (:holder reply))
                   (integer? (:epoch reply))
                   (pos? (:epoch reply))
                   (= (:epoch reply) (:ok reply))
                   (integer? (:exp reply))
                   (> (:exp reply) (System/currentTimeMillis)))
      (binding [*out* *err*]
        (println (str "presence: session lease was not granted for " h ": " (pr-str reply))))
      (System/exit 1))
    reply))

(let [[port verb & args] *command-line-args*
      port (Integer/parseInt port)
      now  (System/currentTimeMillis)]      ; same machine as coord -> agent-now ~ coord-now
  (case verb
    "register"
    ;; SESSION-START facts (agent/dir/session_id/started_at) are written ONCE per
    ;; session — NOT on every register. The PostToolUse hook calls `register` on every
    ;; tool call to renew the liveness lease; before this guard it ALSO re-put!
    ;; started_at with a fresh (Instant/now) each time, and since started_at is single-
    ;; valued that supersede appended ~1 log line PER TOOL CALL of pure churn (a busy
    ;; agent bloats the log by hundreds of lines/session; agent/dir/session_id were
    ;; already idempotent — same value each call — so started_at's ever-moving value was
    ;; the entire bloat). The LEASE renewal MUST stay per-call: that IS the heartbeat.
    ;; Re-stamp the session-start block only on a genuinely NEW session (session_id
    ;; changed for this handle) or if started_at is somehow missing.
    (let [[h dir sid] args, se (presence-entity h)
          new-session? (or (nil? (resolved port se "started_at"))
                           (not= (str (or sid "?")) (str (resolved port se "session_id"))))]
      (when new-session?
        (put! port se "agent" h)                     ; single
        (put! port se "dir" (or dir "?"))            ; single
        (put! port se "session_id" (or sid "?"))     ; single
        (put! port se "started_at" (str (java.time.Instant/now))))  ; single, once/session
      (acquire-session-lease! port h))

    "renew"
    (let [[h] args] (acquire-session-lease! port h))

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
    ;; engine's FRAM_SINGLE_VALUED set, so the engine still treats them as multi — put!
    ;; here is presently wire-identical to a bare append; the LWW becomes native once
    ;; thread B folds these into the engine cardinality FACT. Verb names the intent.
    (let [[h model effort ctx life sup] args, ae (str "@agent:" h)]
      (north.topology-authority/require-self-agent! "identify peer agent" h)
      (when (and (resolved port ae "identity_manifest_sha256")
                 (or (and model (seq model)) (and effort (seq effort))))
        (throw (ex-info "managed lane route identity is publisher-owned; presence identify may not rewrite model/effort"
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
      (let [rs (:values (send-op port {:op :resolved :te ae :p "holds"}))]
        (println (format "%-15s %s" "roles" (if (seq rs) (str/join ", " (map #(subs % 6) (sort rs))) "-"))))
      (let [ws (:values (send-op port {:op :resolved :te ae :p "watches"}))]
        (println (format "%-15s %s" "watches" (if (seq ws) (str/join ", " (sort ws)) "-")))))

    "define-role"                           ; <slug> <exclusive|inclusive> "<title>"  — register a role
    (let [[slug excl title] args, re (str "@role:" slug)]
      (put! port re "title" (or title slug))             ; single
      (put! port re "exclusivity" (or excl "inclusive")) ; single
      (prn {:role re :exclusivity (or excl "inclusive")}))

    ;; assign/unassign — COEXIST-ELECT, no lease (thread 019f100f-eefe). The exclusive-role
    ;; @lease:role:<slug> family is DELETED: a role holder is graph-internal, so it collapses
    ;; onto coexist-elect. `holds` is MULTI, so rival assigns to an exclusive role BOTH land
    ;; (no block, no refusal); the single true holder is ELECTED at read time (earliest holder
    ;; wins — `holders` lists them in election order). A loser sees it lost on its next read and
    ;; yields — dup is cheaper than coordination. (Lease survives only for EXTERNAL resources.)
    "assign"                                ; <uuid> <slug>  — agent takes a role (coexist-elect)
    (let [[h slug] args]
      (north.topology-authority/require-self-agent! "assign peer agent" h)
      (let [ae (str "@agent:" h), re (str "@role:" slug)
            excl (resolved port re "exclusivity")
            prior (->> (send-op port {:op :query :query {:find "a"
                         :rules [{:head {:rel "a" :args [{:var "a"}]}
                                  :body [{:rel "triple" :args [{:var "a"} "holds" re]}]}]}})
                       :ok (mapv first) (remove #(= ae %)) vec)]
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
          rs (:values (send-op port {:op :resolved :te ae :p "holds"}))]
      (doseq [r (sort rs)]
        (println (format "%-22s %-10s %s" (subs r 6) (or (resolved port r "exclusivity") "?") (or (resolved port r "title") "")))))

    "holders"                               ; <slug>  — which agents hold this role (reverse edge)
    (let [[slug] args, re (str "@role:" slug)
          hs (:ok (send-op port {:op :query :query {:find "a"
                  :rules [{:head {:rel "a" :args [{:var "a"}]}
                           :body [{:rel "triple" :args [{:var "a"} "holds" re]}]}]}}))]
      (println (str "@role:" slug " (" (or (resolved port re "exclusivity") "?") ") held by:"))
      (doseq [row (or hs [])] (println "  " (first row))))

    "focus"                                 ; <uuid> <current_thread> [active_workflow] — VOLATILE, on the session
    (let [[h ct wf] args, se (presence-entity h)]
      (put! port se "current_thread" (or ct "-"))   ; single (LWW intent; see identify note)
      (when wf (put! port se "active_workflow" wf))  ; single
      (prn {:focus se :current_thread ct :active_workflow wf}))

    "presence"                              ; THE PROJECTION — agents + held roles + focus. Pinned first, then online, then rest.
    (print-presence! port now (mapv (fn [[entity handle]] {:entity entity :handle handle})
                                    (presence-registrations port)))

    "presence-online"                       ; bounded projection used by live-only UIs
    (print-presence! port now (online-sessions port now))

    "presence-online-json"                  ; stable bounded machine projection
    (print-presence-json! now (online-sessions port now))

    "coordination-probe-json"               ; doctor's honest health signal; exits 1 when broken
    (print-coordination-probe-json! port now)

    "slackers"                              ; derived; replaces the polling slacker-detector/reaper
    (let [_mins (if (seq args) (parse-long (first args)) 10)
          ss (sort-by second (presence-registrations port))]
      (println "online but holding NO build-lease (slacker candidates):")
      (doseq [[_e h] ss]
        (let [l (lease-of port (str "session:" h))
              on (boolean (and l (> (:exp l) now)))
              b (lease-of port "build")
              has-build (boolean (and b (= (:holder b) h) (> (:exp b) now)))]
          (when (and on (not has-build)) (println "  -" h)))))

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
          playbook-count (try (count (:values (send-op 7977 {:op :resolved :te "@2026-06-22-232740" :p "learning"})))
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
              rs (:values (send-op port {:op :resolved :te ae :p "holds"}))
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
          playbook-count (try (count (:values (send-op 7977 {:op :resolved :te "@2026-06-22-232740" :p "learning"})))
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

    "forget"                                ; deregister: retract session facts + release the lease
    (let [[h] args, se (presence-entity h)]
      (doseq [p ["agent" "dir" "session_id" "started_at" "task"
                 "current_thread" "active_workflow"]]
        (when-let [v (resolved port se p)] (retract! port se p v)))
      (prn (send-op port {:op :release-lease :res (str "session:" h) :holder h})))

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
          rs (:values (send-op port {:op :resolved :te ae :p "holds"}))
          ws (:values (send-op port {:op :resolved :te ae :p "watches"}))]
      (println (str "@agent:" h " self-channel: to ∈ {" h ", "
                    (str/join ", " (map #(subs % 6) (sort rs))) ", *}  (uuid ∪ held-roles)"))
      (doseq [t (sort ws)] (println (str "  watches " t))))

    (do (println "usage: presence-cli.clj <port> {register|renew|task|focus|forget|runmeta  (session/run)")
        (println "                                |identify|card  (agent card)")
        (println "                                |define-role|assign|unassign|roles|holders  (roles)")
        (println "                                |watch|unwatch|subscriptions  (thread subs)")
        (println "                                |presence|presence-online|presence-online-json|slackers}  (projections)")
        (System/exit 2))))
