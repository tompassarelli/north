;; presence-cli.clj — presence-as-claims (Lodestar gate-2 #30).
;;
;; THE TRICK: presence = a renewable LEASE. Liveness is judged by the COORDINATOR's
;; clock (the lease expiry), never a self-stamped wall-clock heartbeat. This kills
;; agentchat's heartbeat clock-skew AND its separate reaper in one move: a dead
;; agent's lease simply lapses and online? flips false on its own.
;;
;; Sibling to lease-cli.clj. Wire (daemon b619283): :assert/:version/:acquire-lease/:resolved/:query.
;; A session is @session:<handle> (descriptive claims); liveness = lease on resource
;; session:<handle> -> claim @lease:session:<handle> = "holder|exp|epoch".
;;
;; usage:
;;   bb presence-cli.clj <port> register <handle> <dir> <session_id>
;;   bb presence-cli.clj <port> renew    <handle>                     ; the new heartbeat
;;   bb presence-cli.clj <port> task     <handle> "<task>"
;;   bb presence-cli.clj <port> presence                             ; projection (replaces ls presence/ + age math)
;;   bb presence-cli.clj <port> slackers [minutes]                   ; derived: online + holds no work-lease
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str])

(def TTL 600000)          ; 10min lease; renew every ~3min
(def LEASE-PRED "lease")

(defn send-op [port op]
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w (.getBytes (str (pr-str op) "\n"))) (.flush w)
      (edn/read-string (.readLine r)))))

(defn assert! [port te p r]                 ; OCC: assert at current :version; retry on reject
  (loop [tries 4]
    (let [v (:version (send-op port {:op :version}))
          res (send-op port {:op :assert :te te :p p :r (str r) :base v})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

(defn retract! [port te p r]
  (loop [tries 4]
    (let [v (:version (send-op port {:op :version}))
          res (send-op port {:op :retract :te te :p p :r (str r) :base v})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

(defn resolved [port te p] (:value (send-op port {:op :resolved :te te :p p})))

(defn decode-lease [v]
  (when (string? v)
    (let [[h e ep] (str/split v #"\|")]
      (when (and h e) {:holder h :exp (parse-long e) :epoch (parse-long (or ep "0"))}))))

(defn lease-of [port res] (decode-lease (resolved port (str "@lease:" res) LEASE-PRED)))

(defn sessions [port]      ; -> [[session-entity handle] ...]  ONE row per uuid.
  ;; `agent` is overloaded: it anchors @session:<h> (the session) AND every @run:<sid> (cost
  ;; record). The raw query returns one row PER run -> the roster showed N rows per agent. Scope
  ;; to @session:* (the real anchor; exactly one per handle) and dedup by handle, so the roster is
  ;; latest-per-uuid, lease-judged. (Datalog can't prefix-filter the entity, so we filter here.)
  (let [rows (:ok (send-op port {:op :query
                      :query {:find "s"
                              :rules [{:head {:rel "s" :args [{:var "e"} {:var "h"}]}
                                       :body [{:rel "triple" :args [{:var "e"} "agent" {:var "h"}]}]}]}}))]
    (->> (or rows [])
         (filter (fn [[e _]] (str/starts-with? (str e) "@session:")))
         (reduce (fn [m [e h]] (assoc m h [e h])) {})   ; one row per handle
         vals
         vec)))

(let [[port verb & args] *command-line-args*
      port (Integer/parseInt port)
      now  (System/currentTimeMillis)]      ; same machine as coord -> agent-now ~ coord-now
  (case verb
    "register"
    (let [[h dir sid] args, se (str "@session:" h)]
      (assert! port se "agent" h)
      (assert! port se "dir" (or dir "?"))
      (assert! port se "session_id" (or sid "?"))
      (assert! port se "started_at" (str (java.time.Instant/now)))
      (prn (send-op port {:op :acquire-lease :res (str "session:" h) :holder h :ttl-ms TTL})))

    "renew"
    (let [[h] args] (prn (send-op port {:op :acquire-lease :res (str "session:" h) :holder h :ttl-ms TTL})))

    "task"
    (let [[h t] args] (prn (assert! port (str "@session:" h) "task" t)))

    ;; ===========================================================================
    ;; AGENT REGISTRY. Handle is an opaque uuid (an ADDRESS, never a name). Identity is a
    ;; COLLECTION OF ROLES (@role:<slug> claims), each exclusive (one holder,
    ;; lease-enforced) or inclusive (shared). You ADDRESS a role (routes to its
    ;; holder) or a uuid; the uuid is just the non-colliding instance id.
    ;; ===========================================================================
    "identify"                              ; <uuid> [model] [effort] [context_tokens] [lifecycle] [supervisor]
    (let [[h model effort ctx life sup] args, ae (str "@agent:" h)]
      (when (and model  (seq model))  (assert! port ae "model" model))
      (when (and effort (seq effort)) (assert! port ae "effort" effort))
      (when (and ctx    (seq ctx))    (assert! port ae "context_tokens" ctx))
      (assert! port ae "lifecycle" (or life "standing"))
      (when (and sup (seq sup)) (assert! port ae "supervisor" sup))
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
      (assert! port re "title" (or title slug))
      (assert! port re "exclusivity" (or excl "inclusive"))
      (prn {:role re :exclusivity (or excl "inclusive")}))

    "assign"                                ; <uuid> <slug>  — agent takes a role (exclusive => lease-gated)
    (let [[h slug] args, ae (str "@agent:" h), re (str "@role:" slug)
          excl (resolved port re "exclusivity")]
      (if (= excl "exclusive")
        (let [r (send-op port {:op :acquire-lease :res (str "role:" slug) :holder h :ttl-ms TTL})]
          (if (:ok r)
            (do (assert! port ae "holds" re) (prn {:assigned re :to h :exclusive true}))
            (prn {:refused re :reason :exclusive-held :by (:holder r)})))
        (do (assert! port ae "holds" re) (prn {:assigned re :to h :exclusive false}))))

    "unassign"                              ; <uuid> <slug>
    (let [[h slug] args, ae (str "@agent:" h), re (str "@role:" slug)]
      (retract! port ae "holds" re)
      (when (= "exclusive" (resolved port re "exclusivity"))
        (send-op port {:op :release-lease :res (str "role:" slug) :holder h}))
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
    (let [[h ct wf] args, se (str "@session:" h)]
      (assert! port se "current_thread" (or ct "-"))
      (when wf (assert! port se "active_workflow" wf))
      (prn {:focus se :current_thread ct :active_workflow wf}))

    "presence"                              ; THE PROJECTION — agents + held roles + focus
    (let [ss (sort-by second (or (sessions port) []))]
      (println (format "%-14s %-6s %-7s %-26s %s" "AGENT" "ONLINE" "EXPIRES" "ROLES" "FOCUS"))
      (doseq [[e h] ss]
        (let [l (lease-of port (str "session:" h))
              on (boolean (and l (> (:exp l) now)))
              exp (if (and l on) (str (int (/ (- (:exp l) now) 1000)) "s") "lapsed")
              rs (:values (send-op port {:op :resolved :te (str "@agent:" h) :p "holds"}))
              resp (if (seq rs) (str/join "," (map #(subs % 6) (sort rs))) "-")
              focus (or (resolved port e "active_workflow") (resolved port e "current_thread") (resolved port e "task") "-")]
          (println (format "%-14s %-6s %-7s %-26s %s" h (if on "yes" "no") exp resp focus)))))

    "slackers"                              ; derived; replaces the polling slacker-detector/reaper
    (let [_mins (if (seq args) (parse-long (first args)) 10)
          ss (sort-by second (or (sessions port) []))]
      (println "online but holding NO build-lease (slacker candidates):")
      (doseq [[_e h] ss]
        (let [l (lease-of port (str "session:" h))
              on (boolean (and l (> (:exp l) now)))
              b (lease-of port "build")
              has-build (boolean (and b (= (:holder b) h) (> (:exp b) now)))]
          (when (and on (not has-build)) (println "  -" h)))))

    "forget"                                ; deregister: retract session claims + release the lease
    (let [[h] args, se (str "@session:" h)]
      (doseq [p ["agent" "dir" "session_id" "started_at" "task"]]
        (when-let [v (resolved port se p)] (retract! port se p v)))
      (prn (send-op port {:op :release-lease :res (str "session:" h) :holder h})))

    "cost"                                  ; per-run real $ cost -> claim on the fleet graph (per-task token tracking)
    (let [[h sid c] args re (str "@run:" sid)]
      (assert! port re "agent" h)
      (assert! port re "cost_usd" (or c "0"))
      (assert! port re "ended_at" (str (java.time.Instant/now)))
      (prn {:recorded re :agent h :cost_usd c}))

    ;; --- subscriptions: thread-watches as claims (consumed by fleet-listen.clj) ---
    ;; subject = the agent's self node @<handle> (its self-reference channel is implicit; this
    ;; ADDS threads beyond it). multi-valued: an agent watches many threads.
    "watch"                                 ; <uuid> <thread-ref>  — subscribe to a thread
    (let [[h t] args] (prn (assert! port (str "@agent:" h) "watches" t)))

    "unwatch"                               ; <uuid> <thread-ref>  — drop a subscription
    (let [[h t] args] (prn (retract! port (str "@agent:" h) "watches" t)))

    "subscriptions"                         ; <uuid>  — channel = uuid ∪ held roles ∪ watched threads
    (let [[h] args, ae (str "@agent:" h)
          rs (:values (send-op port {:op :resolved :te ae :p "holds"}))
          ws (:values (send-op port {:op :resolved :te ae :p "watches"}))]
      (println (str "@agent:" h " self-channel: to ∈ {" h ", "
                    (str/join ", " (map #(subs % 6) (sort rs))) ", *}  (uuid ∪ held-roles)"))
      (doseq [t (sort ws)] (println (str "  watches " t))))

    (do (println "usage: presence-cli.clj <port> {register|renew|task|focus|cost|forget  (session)")
        (println "                                |identify|card  (agent card)")
        (println "                                |define-role|assign|unassign|roles|holders  (roles)")
        (println "                                |watch|unwatch|subscriptions  (thread subs)")
        (println "                                |presence|slackers}  (projections)")
        (System/exit 2))))
