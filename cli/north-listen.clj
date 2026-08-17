;; north-listen.clj <port> <uuid> [--once] [--ack] [--react]
;;
;; Fact-native pub/sub, client-side. An agent is @agent:<uuid> (an opaque address). Its SCOPE is:
;;   self-channel : a direct commit to {uuid} ∪ {roles it HOLDS}, or a broadcast
;;                  whose finite send-time audience contains uuid
;;   watched thread: a commit whose SUBJECT is a thread it watches                  — that thread moved
;; You ADDRESS a role (e.g. `to beagle-store`) and it routes to the current holder — agents are
;; fungible, roles are the stable address. holds/watches are facts (@agent:<uuid> holds @role:…
;; / watches @thread), so assign/unassign/watch/unwatch updates the scope on the
;; next bounded occurrence poll without reconnecting.
;;
;; --once : exit after the first ping — the interactive bridge (run as a bg task; completion == "you have mail").
;; --ack  : auto-assert acked_by <uuid> on each delivered message.
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

;; Shared canonical coordination substrate.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(def append! north.coord/append!)
(def rf      north.coord/resolved)
(def rmany   north.coord/many)
(defn role-slug [r] (when (and (string? r) (>= (count r) 6) (= "@role:" (subs r 0 6))) (subs r 6)))

(defn ack! [port me id] (append! port id "acked_by" me))   ; acked_by is multi — append (coexist)

(defn listener-kind-projection [port node]
  (let [kinds (vec (distinct (north.coord/many port node "kind")))]
    (when (> (count kinds) 1)
      (throw
       (ex-info "listener identity has an ambiguous kind"
                {:type :ambiguous-listener-kind
                 :node node
                 :values kinds})))
    (first kinds)))

(defn listener-node-projection [port node]
  (let [by-predicate
        (reduce
         (fn [index [predicate value]]
           (update index predicate (fnil conj []) value))
         {}
         (north.coord/show-rows port node))
        kinds (vec (distinct (get by-predicate "kind" [])))]
    (when (> (count kinds) 1)
      (throw
       (ex-info "listener identity has an ambiguous kind"
                {:type :ambiguous-listener-kind
                 :node node
                 :values kinds})))
    {:kind (first kinds)
     :holds (vec (distinct (get by-predicate "holds" [])))
     :watches (vec (distinct (get by-predicate "watches" [])))}))

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
(def listener-poll-ms
  (positive-env-ms "NORTH_FRAMRPC_LISTENER_POLL_MS" 250))
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
    (when (= :held (:reject response))
      (throw
       (ex-info "another listener generation already owns this identity"
                {:type :listener-generation-held
                 :holder (:holder response)
                 :expiry expiry})))
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
  (when (= :fence-lost (:reject response))
    (throw
     (ex-info "listener generation was superseded"
              {:type :listener-generation-superseded
               :operation operation})))
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
        (north.coord/acquire-lease!
         port resource holder listener-lease-ttl-ms)
        grant (require-listener-lease-grant! holder response)
        generation {:port port
                    :node node
                    :resource resource
                    :holder holder
                    :epoch (atom (:epoch grant))
                    :active? (atom true)
                    :stop-renewal? (atom false)
                    :transport (atom nil)
                    :renewal-error (atom nil)}]
    (try
      ;; A predecessor killed without cleanup may leave durable `armed` behind.
      ;; Publish a false boundary under this new fence before expensive scope
      ;; projection. Capturing the poll baseline makes armed the last route write.
      (fenced-listener-state! generation "frozen")
      (fenced-replace-listener-value!
       generation "live_input_epoch" holder)
      generation
      (catch Exception error
        (try
          (north.coord/release-lease! port (listener-fence generation))
          (catch Exception _ nil))
        (throw error)))))

(defn arm-listener-generation! [generation]
  (when generation
    (fenced-listener-state! generation "armed")))

(defn renew-listener-generation!
  [generation]
  (locking generation
    (when @(:active? generation)
      (let [response
            (north.coord/renew-lease!
             (:port generation) (listener-fence generation)
             listener-lease-ttl-ms)
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
         (north.coord/release-lease!
          (:port generation) (listener-fence generation)))
        (catch Exception error
          (binding [*out* *err*]
            (println
             (str "north listen: listener lease release skipped: "
                  (or (.getMessage error) (.getName (class error)))))
            (flush)))))))

(defn start-listener-renewer!
  [generation]
  (future
    (loop []
      (Thread/sleep listener-renew-interval-ms)
      (when-not @(:stop-renewal? generation)
        (if-let [error
                 (try
                   (renew-listener-generation! generation)
                   nil
                   (catch Exception error error))]
          (do
            (reset! (:renewal-error generation) error)
            (when-let [socket @(:transport generation)]
              (try (.close ^java.net.Socket socket)
                   (catch Exception _ nil))))
          (recur))))))

(defn ensure-listener-generation-current! [generation]
  (when-let [error (some-> generation :renewal-error deref)]
    (throw
     (ex-info "listener generation renewal failed"
              {:type :listener-generation-superseded}
              error))))

(defn with-native-listener-generation!
  "Fence a native listener before its scope projection. BODY receives the
   generation or nil; managed lanes retain SDK-only route authority."
  [port node agent-id kind body]
  (if-not (= "session" kind)
    (body nil)
    (let [generation (acquire-listener-generation! port node agent-id)
          renewer (start-listener-renewer! generation)
          shutdown-hook
          (Thread.
           (fn [] (finish-listener-generation! generation))
           (str "north-listener-cleanup-" agent-id))]
      (.addShutdownHook (Runtime/getRuntime) shutdown-hook)
      (try
        (let [result
              (try
                (body generation)
                (catch Exception error
                  (if-let [renewal-error @(:renewal-error generation)]
                    (throw
                     (ex-info "listener generation renewal failed"
                              {:type :listener-generation-superseded}
                              renewal-error))
                    (throw error))))]
          (if-let [renewal-error @(:renewal-error generation)]
            (throw
             (ex-info "listener generation renewal failed"
                      {:type :listener-generation-superseded}
                      renewal-error))
            result))
        (finally
          (reset! (:stop-renewal? generation) true)
          (future-cancel renewer)
          (finish-listener-generation! generation)
          (try
            (.removeShutdownHook (Runtime/getRuntime) shutdown-hook)
            (catch IllegalStateException _ nil)))))))

(defn validate-listener-corpus!
  "Fail before scope reads unless the configured SpaceId is ready."
  [port]
  (let [status (north.coord/status port)]
    (when-not (= :ready (:state status))
      (throw
       (ex-info
        "configured coordination SpaceId is not ready"
        {:type :listener-space-unavailable :status status})))
    status))

(defn stop-listener! []
  (throw (ex-info "listener one-shot complete" {:type :listener-stop})))

(defn listener-pass-failure [error]
  (let [type (:type (ex-data error))]
   (cond
    (= :listener-stop type)
    {:reason :stop}

    (contains? #{:listener-generation-held
                 :listener-generation-superseded}
               type)
    {:reason :superseded
     :message (or (.getMessage error) "listener generation superseded")}

    (= :rpc/space-mismatch type)
    {:reason :fatal
     :message (or (.getMessage error) (.getName (class error)))
     :error error}

    :else
    {:reason :unavailable
     :message (or (.getMessage error) (.getName (class error)))})))

(defn run-with-reconnect!
  "Drive transient poll passes until a test/embedding pass returns :stop."
  [pass! sleep! notice!]
  (loop [failure-attempt 0]
    (let [result (pass!)]
      (case (:reason result)
        :stop result
        :superseded result
        :rescope (recur 0)
        :fatal
        (throw
         (or (:error result)
             (ex-info (or (:message result) "listener poll failed")
                      {:type :listener-fatal})))
        (let [delay-ms (reconnect-backoff-ms failure-attempt)]
          (notice! result delay-ms)
          (sleep! delay-ms)
          (recur (inc failure-attempt)))))))

;; --- Phase 1: command consumption — a forward-chaining rule over facts -------
;; The consumer never string-parses a command envelope (the parse-envelope copy that
;; "MUST stay in sync" with msg-cli is DELETED). A command is FACTS on @cmd:<id>; the
;; consumer matches PENDING ones (op+target, NOT acked_by) via the shared Datalog rule
;; (coord/pending-cmds) and reads each arg as a fact with rf — no parsing, no settle sleep.
;; Execute only operations whose effect is safely repeatable across listener
;; crash/replay and rival generations. Peer spawn/dispatch require an atomic
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
    "acquire"
    {:ok false :retryable false
     :message "peer acquire is not a listener operation; use North dispatch"}
    {:ok false :retryable false :message (str "op " op " not wired in the command consumer")}))

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

(defn process-listener-event!
  [port uuid node once? ack? react? addrs watched
   {:keys [operation subject predicate value]}]
  (let [op (name operation)
        l subject
        p predicate
        r value]
    (cond
      (and (= l node) (= p "holds"))
      (when-let [slug (role-slug r)]
        (swap! addrs (if (= op "assert") conj disj) slug)
        (println
         (format "  ↳ addrs: %s %s (now %s)"
                 (if (= op "assert") "+role" "-role")
                 slug (pr-str (sort @addrs))))
        (flush))

      (and (= l node) (= p "watches"))
      (do
        (swap! watched (if (= op "assert") conj disj) r)
        (println
         (format "  ↳ scope: %s %s (now %d watched)"
                 (if (= op "assert") "watch" "unwatch")
                 r (count @watched)))
        (flush))

      (and (= op "assert") (= p "to")
           (not (#{"notification" "subscription"} (rf port l "kind")))
           (north.message-audience/deliverable? port l r uuid @addrs))
      (let [claim (when ack?
                    (north.message-audience/claim-delivery! port l uuid))]
        (when (or (not ack?) claim)
          (println
           (format "✉  MAIL %s  (to %s)\n   from:    %s\n   subject: %s\n   body:    %s"
                   l r (rf port l "from") (rf port l "subject")
                   (rf port l "body")))
          (flush)
          (when ack?
            (north.message-audience/complete-delivery! port l uuid claim)
            (println (str "   ↳ acked_by " uuid))
            (flush))
          (when once? (stop-listener!))))

      (and (= op "assert") (= p "target")
           (not (#{"notification" "subscription"} (rf port l "kind")))
           (contains? @addrs r))
      (do
        (if react?
          (react-pending! port uuid @addrs)
          (println
           (format "⌘  COMMAND %s  op=%s  (target %s)"
                   l (rf port l "op") r)))
        (flush)
        (when once? (stop-listener!)))

      (and (= op "retract") (= p "failed_by")
           (str/starts-with? (str l) "@cmd:")
           (contains? @addrs (rf port l "target")))
      (do
        (when react? (react-pending! port uuid @addrs))
        (flush))

      (and (= op "assert") (contains? @watched l))
      (do
        (println (format "◆  THREAD %s  %s = %s" l p r))
        (flush)
        (when once? (stop-listener!))))))

(defn replay-listener-mail!
  [port uuid once? ack? addrs]
  (when ack?
    (loop []
      (let [messages
            (:messages
             (north.message-audience/pending-message-page
              port uuid @addrs))
            delivered
            (reduce
             (fn [count-value message]
               (let [to (rf port message "to")]
                 (if-not
                  (and (string? to)
                       (north.message-audience/deliverable?
                        port message to uuid @addrs))
                   count-value
                   (if-let [claim
                            (north.message-audience/claim-delivery!
                             port message uuid)]
                     (do
                       (println
                        (format
                         "✉  MAIL %s  (to %s)\n   from:    %s\n   subject: %s\n   body:    %s"
                         message to (rf port message "from")
                         (rf port message "subject")
                         (rf port message "body")))
                       (flush)
                       (north.message-audience/complete-delivery!
                        port message uuid claim)
                       (println (str "   ↳ acked_by " uuid))
                       (flush)
                       (when once? (stop-listener!))
                       (inc count-value))
                     count-value))))
             0 messages)]
        (when (and (seq messages) (= delivered (count messages)))
          (recur))))))

(def listener-window 256)

(defn run-listener-pass!
  [port uuid node once? ack? react? addrs watched]
  (try
    (validate-listener-corpus! port)
    (let [kind (listener-kind-projection port node)]
      (with-native-listener-generation!
       port node uuid kind
       (fn [generation]
         ;; The exact kind read decides whether North owns the route. Native
         ;; sessions fence and freeze before any mutable scope projection.
         (let [baseline (north.coord/cur-ver port)
               projection (listener-node-projection port node)
               _ (when-not (= kind (:kind projection))
                   (throw
                    (ex-info "listener kind changed during fenced startup"
                             {:type :listener-scope-changed})))
               roles
               (filterv #(and (string? %)
                              (str/starts-with? % "@role:"))
                        (:holds projection))]
           (reset! addrs (into #{uuid} (keep role-slug roles)))
           (reset! watched (set (:watches projection)))
           (arm-listener-generation! generation)
           (println
            (format
             "● @agent:%s listening [FRAMRPC poll] — addrs %s + %d watched%s"
             uuid (pr-str (sort @addrs)) (count @watched)
             (if once? "  [--once]" "")))
           (flush)
           (when react? (react-pending! port uuid @addrs))
           (replay-listener-mail! port uuid once? ack? addrs)
           (loop [cursor baseline
                  next-replay-at
                  (+ (System/currentTimeMillis)
                     north.message-audience/delivery-claim-ttl-ms)]
             (ensure-listener-generation-current! generation)
             (let [head (north.coord/cur-ver port)
                   upper (min head (+ cursor listener-window))]
               (when (< cursor upper)
                 (north.coord/poll-occurrence-window!
                  port cursor upper
                  #(process-listener-event!
                    port uuid node once? ack? react? addrs watched %)))
               (let [now (System/currentTimeMillis)
                     changed? (< cursor upper)
                     retry? (<= next-replay-at now)]
                 (when (or changed? retry?)
                   (replay-listener-mail! port uuid once? ack? addrs))
                 (Thread/sleep listener-poll-ms)
                 (recur upper
                        (if retry?
                          (+ now
                             north.message-audience/delivery-claim-ttl-ms)
                          next-replay-at)))))))))
    (catch Exception error
      (listener-pass-failure error))))

(when-not (= "1" (System/getenv "NORTH_LISTEN_LIB"))
  (let [[ps uuid & flags] *command-line-args*
        port (Integer/parseInt ps)
        node (str "@agent:" uuid)
        once? (boolean (some #{"--once"} flags))
        ack? (boolean (some #{"--ack"} flags))
        react? (boolean (some #{"--react"} flags))
        _ (when-let [problem
                     (and react?
                          (north.topology-authority/authority-problem
                           "listen --react"))]
            (binding [*out* *err*] (println problem))
            (System/exit 1))
        addrs (atom #{uuid})
        watched (atom #{})]
    (run-with-reconnect!
     #(run-listener-pass! port uuid node once? ack? react? addrs watched)
     (fn [delay-ms] (Thread/sleep delay-ms))
     (fn [result delay-ms]
       (binding [*out* *err*]
         (println
          (str "north listen: " (:message result)
               "; reconnecting in " delay-ms "ms"))
         (flush))))))
