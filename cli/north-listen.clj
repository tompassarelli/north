;; north-listen.clj <uuid> [--once] [--ack] — dormant-until-pinged listener.
;;
;; Fact-native pub/sub, client-side. An agent is @agent:<uuid> (an opaque address). Its SCOPE is:
;;   self-channel : a direct commit to {uuid} ∪ {roles it HOLDS}, or a broadcast
;;                  whose finite send-time audience contains uuid
;;   watched thread: a commit whose SUBJECT is a thread it watches                  — that thread moved
;; You ADDRESS a role (e.g. `to fram-engine`) and it routes to the current holder — agents are
;; fungible, roles are the stable address. holds/watches are facts (@agent:<uuid> holds @role:…
;; / watches @thread), so an assign/unassign/watch/unwatch LIVE-updates the scope with NO reconnect.
;; The daemon's :subscribe firehoses every commit (it ignores :filter); ALL matching is here. Dormant on
;; the socket between pushes: zero poll, zero tokens until something is actually addressed.
;;
;; --once : exit after the first ping — the interactive bridge (run as a bg task; completion == "you have mail").
;; --ack  : auto-assert acked_by <uuid> on each delivered message.
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[babashka.process :as proc])

;; shared coord substrate (Foundation Part B): wire helpers live once in cli/coord.clj
;; (rf/rmany = the single/multi resolved variants — semantics unchanged).
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/attention.clj"))
(def send-op north.coord/send-op)
(def append! north.coord/append!)
(def rf      north.coord/resolved)
(def rmany   north.coord/many)
(defn role-slug [r] (when (and (string? r) (>= (count r) 6) (= "@role:" (subs r 0 6))) (subs r 6)))

(defn ack! [port me id] (append! port id "acked_by" me))   ; acked_by is multi — append (coexist)

(defn unknown-attention-principal? [error]
  (= :unknown-attention-principal (:type (ex-data error))))

(defn listener-attention-principals [port node agent-id]
  ;; Concerns historically store their owner as @<lane-id>, while follow
  ;; ownership prefers @agent:<lane-id> or a role. Keep both exact refs in the
  ;; listener's attention set; direct-message addresses remain bare literals.
  (into #{node (str "@" agent-id)}
        (north.attention/role-principals port agent-id)))

(defn principal-follows [port principal]
  ;; Direct listeners predate graph-backed identities and remain valid for
  ;; mail even when their ephemeral @agent node has not been registered.
  (try
    (north.attention/following port principal)
    (catch clojure.lang.ExceptionInfo error
      (if (unknown-attention-principal? error)
        []
        (throw error)))))

(defn listener-attention-scope [port principals]
  (let [subscriptions
        (->> principals
             sort
             (mapcat #(principal-follows port %))
             (sort-by :id)
             vec)]
    {:subscriptions subscriptions
     :followed (set (keep :about subscriptions))
     :about->principals
     (reduce
      (fn [index {:keys [about subscriber]}]
        (update index about (fnil conj #{}) subscriber))
      {}
      subscriptions)}))

(defn attention-principals-for [scope about]
  (get (:about->principals scope) about #{}))

(defn print-attention-notification! [row]
  (println
   (format "◉  NOTICE %s  (for %s)\n   kind:    %s\n   about:   %s\n   subject: %s\n   body:    %s"
           (:id row) (:recipient row) (:attention-kind row) (:about row)
           (:subject row) (:body row)))
  (flush))

(defn catch-up-attention!
  "Replay durable follows for PRINCIPALS. Inbox remains silent; notify is a
   non-terminal notice delivered by the publisher's scoped `target` wake edge. The
   returned IDs are newly materialized by this sync."
  [port principals]
  (vec
   (mapcat
    (fn [principal]
      (try
        (let [result (north.attention/sync-principal! port principal)
              ids (distinct (:notification-ids result))]
          ids)
        (catch clojure.lang.ExceptionInfo error
          (if (unknown-attention-principal? error)
            []
            (throw error)))))
    (sort principals))))

(defn positive-env-ms [name default-value]
  (let [raw (or (System/getenv name) (str default-value))
        value (when (re-matches #"[1-9][0-9]{0,5}" raw)
                (parse-long raw))]
    (when-not value
      (throw
       (ex-info (str name " must be an integer from 1 through 999999 milliseconds")
                {:type :invalid-listener-backoff :name name :value raw})))
    value))

(def listener-initial-backoff-ms
  (positive-env-ms "NORTH_LISTEN_INITIAL_BACKOFF_MS" 250))
(def listener-max-backoff-ms
  (positive-env-ms "NORTH_LISTEN_MAX_BACKOFF_MS" 5000))
(def listener-lease-ttl-ms
  (positive-env-ms "NORTH_LISTENER_LEASE_TTL_MS" 120000))
(def listener-renew-interval-ms
  (max 10 (quot listener-lease-ttl-ms 3)))

(when (< listener-max-backoff-ms listener-initial-backoff-ms)
  (throw
   (ex-info "NORTH_LISTEN_MAX_BACKOFF_MS must be at least the initial backoff"
            {:type :invalid-listener-backoff-order})))

(defn reconnect-backoff-ms [attempt]
  (let [shift (min 20 (max 0 attempt))
        multiplier (bit-shift-left 1 shift)]
    (min listener-max-backoff-ms
         (* listener-initial-backoff-ms multiplier))))

(defn listener-resource [agent-id]
  (str "listener:" agent-id))

(defn require-listener-lease-grant!
  [holder response]
  (let [epoch (:epoch response)
        expiry (:exp response)]
    (when-not
     (and (map? response)
          (nil? (:reject response))
          (= holder (:holder response))
          (integer? epoch)
          (pos? epoch)
          (= epoch (:ok response))
          (integer? expiry)
          (> expiry (System/currentTimeMillis)))
      (throw
       (ex-info "coordinator did not grant the listener generation lease"
                {:type :invalid-listener-lease-grant
                 :holder holder
                 :response response})))
    response))

(defn checked-listener-write!
  [operation response]
  (when (or (not (map? response)) (:reject response))
    (throw
     (ex-info "coordinator rejected listener route publication"
              {:type :listener-route-publication-rejected
               :operation operation
               :response response})))
  response)

(defn listener-fence [generation]
  {:resource (:resource generation)
   :holder (:holder generation)
   :epoch @(:epoch generation)})

(defn exact-listener-envelope?
  [envelope target]
  (and (= 1 (:members envelope))
       (false? (:ambiguous? envelope))
       (= [target] (:values envelope))
       (= target (:value envelope))))

(defn empty-listener-envelope?
  [envelope]
  (and (zero? (:members envelope))
       (false? (:ambiguous? envelope))
       (= [] (:values envelope))
       (nil? (:value envelope))))

(defn require-listener-envelope!
  [operation predicate target envelope expected?]
  (when-not (expected? envelope)
    (throw
     (ex-info "listener route publication did not converge to its exact envelope"
              {:type :listener-route-envelope-not-exact
               :operation operation
               :predicate predicate
               :target target
               :envelope envelope})))
  envelope)

(defn fenced-replace-listener-value!
  "Replace every non-target live value under the current listener fence, assert
   TARGET, then prove the resulting envelope is exactly one unambiguous value.
   The multi-turn transition fails closed: arming happens only after frozen
   state and the exact generation are already authoritative."
  [generation predicate target]
  (let [port (:port generation)
        node (:node generation)
        fence (listener-fence generation)
        before (north.coord/resolved-envelope port node predicate)]
    (doseq [value (:values before)
            :when (not= value target)]
      (checked-listener-write!
       [:listener-value-retract predicate value]
       (north.coord/retract-with-fence!
        port fence node predicate value)))
    (checked-listener-write!
     [:listener-value-assert predicate target]
     (north.coord/put-with-fence!
      port fence node predicate target))
    (require-listener-envelope!
     :replace predicate target
     (north.coord/resolved-envelope port node predicate)
     #(exact-listener-envelope? % target))))

(defn fenced-clear-listener-values!
  "Retract every live value under the current listener fence and prove absence."
  [generation predicate]
  (let [port (:port generation)
        node (:node generation)
        fence (listener-fence generation)
        before (north.coord/resolved-envelope port node predicate)]
    (doseq [value (:values before)]
      (checked-listener-write!
       [:listener-value-retract predicate value]
       (north.coord/retract-with-fence!
        port fence node predicate value)))
    (require-listener-envelope!
     :clear predicate nil
     (north.coord/resolved-envelope port node predicate)
     empty-listener-envelope?)))

(defn fenced-listener-state!
  [generation state]
  (fenced-replace-listener-value!
   generation "live_input_state" state))

(defn acquire-listener-generation!
  [port node agent-id]
  (let [holder (str (java.util.UUID/randomUUID))
        resource (listener-resource agent-id)
        response
        (send-op port {:op :acquire-lease
                       :res resource
                       :holder holder
                       :ttl-ms listener-lease-ttl-ms})
        grant (require-listener-lease-grant! holder response)
        generation {:port port
                    :node node
                    :resource resource
                    :holder holder
                    :epoch (atom (:epoch grant))
                    :active? (atom true)
                    :stop-renewal? (atom false)}]
    (try
      ;; A predecessor killed without cleanup may leave durable `armed` behind.
      ;; Publish a false boundary under this new fence before installing its
      ;; generation, then make armed the last route write.
      (fenced-listener-state! generation "frozen")
      (fenced-replace-listener-value!
       generation "live_input_epoch" holder)
      (fenced-listener-state! generation "armed")
      generation
      (catch Exception error
        (try
          (send-op port {:op :release-lease
                         :res resource
                         :holder holder
                         :epoch @(:epoch generation)})
          (catch Exception _ nil))
        (throw error)))))

(defn renew-listener-generation!
  [generation]
  (locking generation
    (when @(:active? generation)
      (let [response
            (send-op
             (:port generation)
             {:op :renew-lease
              :res (:resource generation)
              :holder (:holder generation)
              :epoch @(:epoch generation)
              :ttl-ms listener-lease-ttl-ms})
            grant (require-listener-lease-grant!
                   (:holder generation) response)]
        (reset! (:epoch generation) (:epoch grant))
        grant))))

(defn finish-listener-generation!
  "Freeze before release while this generation still owns its fence. Cleanup is
   idempotent and best-effort: a lost fence means a successor already owns the
   route, so the predecessor must not touch its state."
  [generation]
  (reset! (:stop-renewal? generation) true)
  (locking generation
    (when (compare-and-set! (:active? generation) true false)
      (try
        (fenced-listener-state! generation "frozen")
        (fenced-clear-listener-values! generation "live_input_epoch")
        (catch Exception error
          (binding [*out* *err*]
            (println
             (str "north listen: listener cleanup skipped: "
                  (or (.getMessage error) (.getName (class error)))))
            (flush))))
      (try
        (checked-listener-write!
         [:listener-lease-release (:holder generation)]
         (send-op (:port generation)
                  {:op :release-lease
                   :res (:resource generation)
                   :holder (:holder generation)
                   :epoch @(:epoch generation)}))
        (catch Exception error
          (binding [*out* *err*]
            (println
             (str "north listen: listener lease release skipped: "
                  (or (.getMessage error) (.getName (class error)))))
            (flush)))))))

(defn start-listener-renewer!
  [generation unavailable!]
  (future
    (loop []
      (Thread/sleep listener-renew-interval-ms)
      (when-not @(:stop-renewal? generation)
        (if-let [error
                 (try
                   (renew-listener-generation! generation)
                   nil
                   (catch Exception error error))]
          (unavailable! error)
          (recur))))))

(defn with-native-listener-generation!
  "Run BODY after the subscription handshake. Native sessions publish one
   renewable listener generation; managed lanes retain SDK-only route authority."
  [port node agent-id socket body]
  (if-not (= "session" (rf port node "kind"))
    (body)
    (let [generation (acquire-listener-generation! port node agent-id)
          renewer (start-listener-renewer!
                   generation
                   (fn [_]
                     (try (.close ^java.net.Socket socket)
                          (catch Exception _ nil))))
          shutdown-hook
          (Thread.
           (fn [] (finish-listener-generation! generation))
           (str "north-listener-cleanup-" agent-id))]
      (.addShutdownHook (Runtime/getRuntime) shutdown-hook)
      (try
        (body)
        (finally
          (reset! (:stop-renewal? generation) true)
          (future-cancel renewer)
          (finish-listener-generation! generation)
          (try
            (.removeShutdownHook (Runtime/getRuntime) shutdown-hook)
            (catch IllegalStateException _ nil)))))))

(defn with-validated-native-listener-generation!
  [port node agent-id socket subscription-response body]
  (north.coord/validate-subscription! subscription-response)
  (with-native-listener-generation! port node agent-id socket body))

(defn validate-listener-corpus!
  "Fail before scope reads when this process is pointed at a coordinator for a
   different corpus. Retrying cannot change the process's FRAM_LOG."
  [port]
  (let [reply (north.coord/send-op port {:op :version})]
    (when (contains? #{:log-mismatch "log-mismatch"} (:code reply))
      (throw
       (ex-info
        (str "coordinator refused the fenced subscription: " (pr-str reply))
        {:type :invalid-subscription-handshake
         :expected-log (north.coord/expected-log)
         :reply reply})))
    reply))

(defn stop-listener! []
  (throw (ex-info "listener one-shot complete" {:type :listener-stop})))

(defn terminal-subscription-error?
  "A corpus mismatch cannot heal while this process keeps the same FRAM_LOG.
   Connection loss and incomplete handshakes remain transient restart races."
  [error]
  (let [{:keys [type reply]} (ex-data error)]
    (and (= :invalid-subscription-handshake type)
         (map? reply)
         (contains? #{:log-mismatch "log-mismatch"} (:code reply)))))

(defn listener-pass-failure [error]
  (cond
    (= :listener-stop (:type (ex-data error)))
    {:reason :stop}

    (terminal-subscription-error? error)
    {:reason :fatal
     :message (or (.getMessage error) (.getName (class error)))
     :error error}

    :else
    {:reason :unavailable
     :message (or (.getMessage error) (.getName (class error)))}))

(defn run-with-reconnect!
  "Drive transient subscription passes until a test/embedding pass returns
   :stop. A fatal fenced refusal is returned as an exception, not retried."
  [pass! sleep! notice!]
  (loop [failure-attempt 0]
    (let [result (pass!)]
      (case (:reason result)
        :stop result
        :rescope (recur 0)
        :fatal
        (throw
         (or (:error result)
             (ex-info (or (:message result) "listener subscription failed")
                      {:type :listener-fatal})))
        (let [delay-ms (reconnect-backoff-ms failure-attempt)]
          (notice! result delay-ms)
          (sleep! delay-ms)
          (recur (inc failure-attempt)))))))

;; --- Phase 1: the reactor — a forward-chaining rule over fact-patterns ------
;; The reactor NO LONGER string-parses a command envelope (the parse-envelope copy that
;; "MUST stay in sync" with msg-cli is DELETED). A command is FACTS on @cmd:<id>; the
;; reactor matches PENDING ones (op+target, NOT acked_by) via the shared Datalog rule
;; (coord/pending-cmds) and reads each arg as a fact with rf — no parsing, no settle sleep.
(def acquire-cli
  (str (.getParent (io/file (System/getProperty "babashka.file"))) "/acquire-cli.clj"))

;; Execute only operations whose effect is safely repeatable across listener
;; crash/replay and rival subscribers. Peer spawn/dispatch require an atomic
;; claim + child reconciliation protocol and are fail-closed this release; the
;; canonical MCP/CLI spawn surfaces remain available.
(defn react! [port self op cmd]
  (north.topology-authority/require-coordination! "listen --react")
  (case op
    "spawn"    {:ok false :retryable false
                 :message "peer spawn is unsupported until atomic command claim + child reconciliation land; use North MCP/CLI spawn"}
    "dispatch" {:ok false :retryable false
                 :message "peer dispatch is unsupported until atomic command claim + child reconciliation land; use North MCP/CLI dispatch"}
    ;; tell — the most fact-native op: assert a single fact (id pred value). No executor.
    "tell"     (let [id (rf port cmd "id") pred (rf port cmd "pred") value (rf port cmd "value")]
                 (cond
                   (some #(str/blank? (str %)) [id pred value])
                   {:ok false :retryable false :message "tell requires id, pred, and value"}
                   (str/starts-with? (str/replace-first (str id) #"^@" "") "agent:")
                   {:ok false :retryable false :message "peer tell cannot mutate harness-owned @agent identity"}
                   (and (= pred "judgment_grade")
                        (not (contains? #{"s" "m" "l"} value)))
                   {:ok false :retryable false
                    :message "judgment_grade must be exactly one of: s, m, l"}
                   :else
                   (let [result (append! port id pred value)]
                     (if (:reject result)
                       {:ok false :retryable true :message (str "coordinator rejected tell " id " " pred)}
                       {:ok true :retryable false :message (str "told " id " " pred)}))))
    ;; acquire — route every automatic pickup through the same OCC driver command
    ;; as SDK dispatch. A no-base put! would be LWW and could silently steal a
    ;; live dispatch's thread; acquire-cli denies a different holder instead.
    "acquire"  (let [res (rf port cmd "resource") holder (or (rf port cmd "holder") (rf port cmd "from") self)
                     subj (if (str/starts-with? (str res) "@") res (str "@" res))
                     bare-holder (str/replace-first (str holder) #"^@" "")
                     result (proc/sh {:out :string :err :string :continue true}
                                     "bb" acquire-cli (str port) "acquire" subj bare-holder)]
                 (if (zero? (:exit result))
                   {:ok true :retryable false :message (str "acquired " subj " driver=@" bare-holder)}
                   {:ok false :retryable true :message (str "acquire denied for " subj " — already driven")}))
    {:ok false :retryable false :message (str "op " op " not wired in the reactor")}))

;; The forward-chaining loop: every PENDING command targeting one of my addrs -> execute,
;; ack (acked_by removes it from the pending set — exactly-once), and reply with a FACT
;; (validated by the coordinator's existing commit rule-check, not a JSON-Schema sidecar).
(defn react-pending! [port self addrs]
  (doseq [[cmd op tgt] (sort (or (north.coord/pending-cmds port) []))]
    (when (contains? addrs tgt)
      (println (format "⚙  REACT %s  op=%s  (target %s, from %s)"
                       cmd op tgt (or (rf port cmd "from") "?")))
      (flush)
      (let [result (try (react! port self op cmd)
                        (catch Exception error
                          {:ok false :retryable false :message (.getMessage error)}))]
        (if (:ok result)
          (do
            (append! port cmd "execution_status" "succeeded")
            (append! port cmd "reply" (str op " succeeded by " self ": " (:message result)))
            ;; Terminal success marker LAST.
            (ack! port self cmd)
            (println (str "   ↳ succeeded + acked_by " self)))
          (do
            (append! port cmd "execution_status" "failed")
            (doseq [prior (rmany port cmd "retryable")]
              (north.coord/retract! port cmd "retryable" prior))
            (append! port cmd "retryable" (str (boolean (:retryable result))))
            (append! port cmd "failed_at" (str (java.time.Instant/now)))
            (append! port cmd "reply" (str op " failed by " self
                                           " retryable=" (boolean (:retryable result))
                                           ": " (:message result)))
            ;; Terminal failure marker LAST.
            (append! port cmd "failed_by" self)
            (println (str "   ↳ FAILED (not acknowledged) by " self
                          " · retryable=" (boolean (:retryable result))))))
        (flush)))))

(when-not (= "1" (System/getenv "NORTH_LISTEN_LIB"))
 (let [[ps uuid & flags] *command-line-args*
      port    (Integer/parseInt ps)
      node    (str "@agent:" uuid)
      once?   (boolean (some #{"--once"} flags))
      ack?    (boolean (some #{"--ack"} flags))
      react?  (boolean (some #{"--react"} flags))   ; execute repeat-safe tell/acquire commands + terminal marker
      _       (when-let [problem (and react? (north.topology-authority/authority-problem "listen --react"))]
                (binding [*out* *err*] (println problem))
                (System/exit 1))
      scoped? (boolean (some #{"--scoped"} flags))  ; P5: server-side scoped subscribe (daemon pushes only my commits)
      addrs   (atom #{uuid})
      watched (atom #{})
      attention-principals (atom #{node})
      attention-scope (atom {:subscriptions [] :followed #{} :about->principals {}})]
  ;; Every pass refreshes graph-backed scope, then arms one subscription. A
  ;; scoped address change reconnects immediately. Transient coordinator EOF or
  ;; restart-time protocol failure retries with bounded backoff; a static corpus
  ;; mismatch fails instead of retrying the same rejected fence forever.
  (run-with-reconnect!
   (fn []
    (let [reconnect? (atom false)]
          (try
            (validate-listener-corpus! port)
            (reset! addrs
                    (into #{uuid}
                          (keep role-slug (rmany port node "holds"))))
            (reset! watched (set (rmany port node "watches")))
            (reset! attention-principals
                    (listener-attention-principals port node uuid))
            (reset! attention-scope
                    (listener-attention-scope port @attention-principals))
            (with-open [s (north.coord/connect-socket port)]
        (let [w (.getOutputStream s)
              reader (north.coord/coordinator-reader s)
              transport-watch
              (into
               (into @watched (:followed @attention-scope))
               (keep :id (:subscriptions @attention-scope)))
              ;; The daemon still needs "*" in its transport filter to forward
              ;; broadcast trigger commits. Client-side snapshot membership is
              ;; the delivery authority; "*" is never a command/direct address.
              sub (cond-> {:op :subscribe}
                    scoped? (assoc :filter {:addrs (conj @addrs north.message-audience/broadcast-address)
                                            :watch transport-watch :node node}))]
          (.write w
                  (.getBytes
                   (str (pr-str (north.coord/log-envelope sub)) "\n")
                   java.nio.charset.StandardCharsets/UTF_8))
          (.flush w)
          ;; Publication starts only after the daemon has acknowledged this
          ;; exact subscription. A listener that never completes the handshake
          ;; therefore cannot advertise itself as reachable.
          (with-validated-native-listener-generation!
           port node uuid s
           (north.coord/read-line-bounded! reader)
           (fn []
          (.setSoTimeout s 0)            ; validated long-lived stream: wait indefinitely for pushes
          (println
           (format
            "● @agent:%s listening%s — addrs %s + %d watched + %d followed thread(s)%s"
            uuid (if scoped? " [scoped]" "") (pr-str (sort @addrs))
            (count @watched) (count (:followed @attention-scope))
            (if once? "  [--once]" "")))
          (flush)
          ;; The subscription is armed before replay. A change racing this
          ;; catch-up is therefore either in the durable replay window, on the
          ;; live stream, or both (where deterministic notification IDs dedupe).
          (catch-up-attention! port @attention-principals)
          (let [before (:followed @attention-scope)
                refreshed
                (listener-attention-scope port @attention-principals)]
            (reset! attention-scope refreshed)
            ;; A follow committed between the pre-connect snapshot and the
            ;; validated handshake was caught up, but a scoped transport must
            ;; reconnect once so its server-side watch set includes it.
            (when (and scoped? (not= before (:followed refreshed)))
              (reset! reconnect? true)))
          ;; Replay any repeat-safe command whose effect/diagnostics landed
          ;; before its terminal marker when a prior listener crashed.
          (when react? (react-pending! port uuid @addrs))
          (loop []
            (when-not @reconnect?
             (when-let [line
                        (north.coord/read-stream-line-bounded! reader)]
              (let [ev (try (edn/read-string line) (catch Exception _ nil))]
                (when (and (map? ev) (= :commit (:event ev)))
                  (let [{:keys [op l p r]} ev]
                    (cond
                      ;; (a) role (un)assigned to me -> address set changes; re-scope if --scoped
                      (and (= l node) (= p "holds"))
                      (do (when-let [sl (role-slug r)]
                            (swap! addrs (if (= op "assert") conj disj) sl)
                            (swap! attention-principals
                                   (if (= op "assert") conj disj) r)
                            ;; A newly held stable role may already have pending
                            ;; followed changes. Catch it up before changing the
                            ;; logical/transport scope.
                            (when (= op "assert")
                              (catch-up-attention! port #{r}))
                            (reset! attention-scope
                                    (listener-attention-scope
                                     port @attention-principals))
                            (println (format "  ↳ addrs: %s %s (now %s)"
                                             (if (= op "assert") "+role" "-role") sl (pr-str (sort @addrs)))) (flush)
                            (when scoped? (reset! reconnect? true))))

                      ;; (b) thread watch/unwatch -> re-scope if --scoped
                      (and (= l node) (= p "watches"))
                      (do (swap! watched (if (= op "assert") conj disj) r)
                          (println (format "  ↳ scope: %s %s (now %d watched)"
                                           (if (= op "assert") "watch" "unwatch") r (count @watched))) (flush)
                          (when scoped? (reset! reconnect? true)))

                      ;; (b2) An unscoped listener sees subscription births and
                      ;; endings on the firehose. Refresh its logical follow
                      ;; scope immediately; scoped listeners pick up initial
                      ;; follows and re-evaluate on their normal reconnects.
                      (and (#{"assert" "retract"} op)
                           (or (and (= p "kind") (= r "subscription"))
                               (and (= p "ended_at")
                                    (= "subscription" (rf port l "kind"))))
                           (contains? @attention-principals
                                      (rf port l "subscriber")))
                      (let [principal (rf port l "subscriber")
                            before (:followed @attention-scope)]
                        ;; Ended subscriptions still replay through their
                        ;; captured end cursor, so catch-up precedes removal
                        ;; from the active followed set.
                        (catch-up-attention! port #{principal})
                        (let [refreshed
                              (listener-attention-scope
                               port @attention-principals)]
                          (reset! attention-scope refreshed)
                          (println
                           (format "  ↳ attention scope: %d followed thread(s)"
                                   (count (:followed refreshed))))
                          (flush)
                          (when (and scoped?
                                     (not= before (:followed refreshed)))
                            (reset! reconnect? true))))

                      ;; (b3) Attention ownership stays on recipient/subscriber
                      ;; refs. `target` is the routing-only birth/notification
                      ;; edge. An unfollow retracts that exact target after its
                      ;; atomic end boundary; active subscription subjects also
                      ;; stay in the scoped watch set so ended_at is sufficient
                      ;; to drive rescope.
                      (and (#{"assert" "retract"} op)
                           (= "target" p)
                           (contains? @addrs r)
                           (#{"notification" "subscription"}
                            (rf port l "kind")))
                      (case (rf port l "kind")
                        "notification"
                        (let [principal (rf port l "recipient")]
                          (when (and (= op "assert")
                                     (contains? @attention-principals principal))
                            (let [row
                                  (north.attention/notification-row
                                   port l principal)]
                              (when (= "notify" (:delivery row))
                                (print-attention-notification! row)))))

                        "subscription"
                        (let [principal (rf port l "subscriber")
                              before (:followed @attention-scope)]
                          (when (contains? @attention-principals principal)
                            ;; Ended subscriptions replay through their captured
                            ;; boundary before the active scope is removed.
                            (catch-up-attention! port #{principal})
                            (let [refreshed
                                  (listener-attention-scope
                                   port @attention-principals)]
                              (reset! attention-scope refreshed)
                              (println
                               (format
                                "  ↳ attention scope: %d followed thread(s)"
                                (count (:followed refreshed))))
                              (flush)
                              (when (and scoped?
                                         (not= before (:followed refreshed)))
                                (reset! reconnect? true)))))
                        nil)

                      ;; (c) self-channel: a human message to my uuid OR a role I hold. `to` lands
                      ;; LAST now (msg-cli send writes it after the body), so from/subject/body are
                      ;; already visible — no settle sleep, and no envelope parsing (commands are
                      ;; @cmd: subjects handled in (c2), not mail bodies).
                      (and (= op "assert") (= p "to")
                           (not (#{"notification" "subscription"}
                                 (rf port l "kind")))
                           (north.message-audience/deliverable?
                            port l r uuid @addrs))
                      (let [claim (when ack?
                                    (north.message-audience/claim-delivery! port l uuid))]
                        ;; Raw listeners without --ack remain an explicit
                        ;; monitoring surface. Canonical one-shot listeners use
                        ;; --ack and must win the coordinator claim before print.
                        (when (or (not ack?) claim)
                          (println (format "✉  MAIL %s  (to %s)\n   from:    %s\n   subject: %s\n   body:    %s"
                                           l r (rf port l "from") (rf port l "subject") (rf port l "body")))
                          (flush)
                          (when ack?
                            (north.message-audience/complete-delivery! port l uuid claim)
                            (println (str "   ↳ acked_by " uuid))
                            (flush))
                          (when once? (stop-listener!))))

                      ;; (c2) command landing: a @cmd:<id> whose routing `target` is one of my addrs.
                      ;; `target` is asserted LAST, so op+args are present. Drive the forward-chaining
                      ;; rule (coord/pending-cmds) — no string parse, no settle sleep.
                      (and (= op "assert") (= p "target")
                           (not (#{"notification" "subscription"}
                                 (rf port l "kind")))
                           (contains? @addrs r))
                      (do (if react?
                            (react-pending! port uuid @addrs)   ; --react: execute + ack + reply
                            (println (format "⌘  COMMAND %s  op=%s  (target %s)" l (rf port l "op") r))) (flush)
                          (when once? (stop-listener!)))

                      ;; Backward-compatible recovery for unscoped listeners and
                      ;; historical producers that used failed_by retraction as
                      ;; their activation edge.
                      (and (= op "retract") (= p "failed_by")
                           (str/starts-with? (str l) "@cmd:")
                           (contains? @addrs (rf port l "target")))
                      (do (when react? (react-pending! port uuid @addrs)) (flush))

                      ;; (d) watched-thread activity
                      (and (= op "assert") (contains? @watched l))
                      (do (println (format "◆  THREAD %s  %s = %s" l p r)) (flush)
                          (when once? (stop-listener!)))

                      ;; (e) observer-driven followed-thread activity. Replay
                      ;; matching stable principals into durable notifications.
                      ;; This path NEVER consumes --once; only direct attention
                      ;; and the legacy explicit watch contract may do that.
                      (and (#{"assert" "retract"} op)
                           (contains? (:followed @attention-scope) l))
                      (let [principals
                            (attention-principals-for @attention-scope l)]
                        (catch-up-attention! port principals)
                        (reset! attention-scope
                                (listener-attention-scope
                                 port @attention-principals)))))))
              ;; --scoped re-scope: break the inner read-loop so the outer reconnects with the new filter
              (if @reconnect?
                (do (println "  ↳ re-scoping subscription (addr/watch changed)…") (flush))
                (recur)))))))))
            (if @reconnect?
              {:reason :rescope}
              {:reason :closed
               :message "coordinator subscription stream closed"})
            (catch Exception error
              (listener-pass-failure error)))))
   (fn [delay-ms] (Thread/sleep delay-ms))
   (fn [result delay-ms]
     (binding [*out* *err*]
       (println
        (str "north listen: " (:message result)
             "; reconnecting in " delay-ms "ms"))
       (flush))))))
