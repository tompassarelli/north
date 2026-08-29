#!/usr/bin/env bb
;; Durable machine bridge from North mail facts to one managed SDK input channel.
;;
;; Protocol (north-live-feed-v1), one canonical JSON object per line:
;;   stdout: {"protocol":"north-live-feed-v1","type":"ready",...}
;;   stdin:  {"type":"start"}
;;   stdout: {"protocol":"north-live-feed-v1","type":"caught_up",...}
;;   stdout: {"protocol":"north-live-feed-v1","type":"mail",...}
;;   stdin:  {"type":"ack","id":"@msg:..."} | {"type":"nack","id":"@msg:..."}
;;   stdin:  {"type":"drain","epoch":"<frozen-route-uuid>"}
;;   stdout: {"protocol":"north-live-feed-v1","type":"drain_progress",...}
;;   stdout: {"protocol":"north-live-feed-v1","type":"drained",...}
;;
;; A successful version read establishes the poll cursor before `ready`, and
;; pending mail is replayed only after the host answers `start`. Commits arriving
;; during replay remain above the sampled cursor for the next poll. A claim is
;; held until the host admits the message and answers `ack`; EOF, timeout, nack, or
;; a crash leaves the message unacknowledged and therefore replayable.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-contract.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/agent-provenance.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/lifecycle-projection.clj"))

(def protocol "north-live-feed-v1")
(def max-control-line-bytes 4096)
(def max-output-message-bytes (* 192 1024))
(def event-queue-capacity 1024)
(def control-queue-capacity 32)
(def default-ack-timeout-ms 10000)
(def default-poll-ms 250)
(def target-identity-manifest-predicate "target_identity_manifest_sha256")
(def max-live-route-run-candidates 128)

(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn exact-keys? [value expected]
  (and (map? value) (= expected (set (keys value)))))

(defn bounded-positive [label raw fallback maximum]
  (let [value (if raw
                (try (Long/parseLong raw)
                     (catch Exception _ nil))
                fallback)]
    (when-not (and (integer? value) (pos? value) (<= value maximum))
      (throw (ex-info (str label " must be an integer in [1," maximum "]")
                      {:type :invalid-live-feed-option :option label})))
    value))

(defn poll-ms []
  (bounded-positive
   "NORTH_STORE_LISTENER_POLL_MS"
   (System/getenv "NORTH_STORE_LISTENER_POLL_MS")
   default-poll-ms 999999))

(defn flag-value [flags flag]
  (let [index (.indexOf ^java.util.List flags flag)]
    (when (>= index 0)
      (nth flags (inc index)))))

(defn validate-flags! [flags]
  (when (odd? (count flags))
    (throw (ex-info "live-feed options require flag/value pairs"
                    {:type :invalid-live-feed-option})))
  (doseq [[flag _] (partition 2 flags)]
    (when-not (contains? #{"--claim-ttl-ms" "--ack-timeout-ms"
                           "--settlement-only" "--deferred-start"} flag)
      (throw (ex-info (str "unknown live-feed option: " flag)
                      {:type :invalid-live-feed-option :option flag}))))
  (let [names (take-nth 2 flags)]
    (when-not (= (count names) (count (distinct names)))
      (throw (ex-info "live-feed options must not repeat"
                      {:type :invalid-live-feed-option})))))

(defn safe-control-id? [value]
  (and (string? value)
       (<= (utf8-bytes value)
           north.message-contract/max-message-id-bytes)
       (boolean (re-matches #"^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn safe-route-epoch? [value]
  (and (string? value)
       (boolean
        (re-matches
         #"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
         value))))

(defn canonical-control [line]
  ;; Cheshire's ordinary parser is last-key-wins. Requiring the byte-exact
  ;; canonical encoding after parsing rejects duplicate keys, surplus
  ;; whitespace/trailing forms, and alternate authority-bearing shapes.
  (let [parsed (try (json/parse-string-strict line)
                    (catch Exception error
                      (throw (ex-info "control message is not valid JSON"
                                      {:type :invalid-control-message} error))))
        message
        (cond
          (and (exact-keys? parsed #{"type"})
               (= "start" (get parsed "type")))
          (array-map "type" "start")

          (and (exact-keys? parsed #{"type" "epoch"})
               (= "drain" (get parsed "type"))
               (safe-route-epoch? (get parsed "epoch")))
          (array-map "type" "drain" "epoch" (get parsed "epoch"))

          (and (exact-keys? parsed #{"type" "id"})
               (contains? #{"ack" "nack"} (get parsed "type"))
               (safe-control-id? (get parsed "id")))
          (array-map "type" (get parsed "type") "id" (get parsed "id"))

          :else
          (throw (ex-info "control message has an invalid shape"
                          {:type :invalid-control-message})))]
    (when-not (= line (json/generate-string message))
      (throw (ex-info "control message is not canonical JSON"
                      {:type :noncanonical-control-message})))
    message))

(defn decode-utf8! [^bytes bytes]
  (let [decoder (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
                  (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
                  (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
    (str (.decode decoder (java.nio.ByteBuffer/wrap bytes)))))

(defn read-control-line! [^java.io.InputStream input]
  (let [output (java.io.ByteArrayOutputStream.)]
    (loop []
      (let [value (.read input)]
        (cond
          (= -1 value)
          (if (zero? (.size output))
            nil
            (throw (ex-info "control stream closed during a message"
                            {:type :truncated-control-message})))

          (= 10 value)
          (decode-utf8! (.toByteArray output))

          (>= (.size output) max-control-line-bytes)
          (throw (ex-info "control message exceeds its byte bound"
                          {:type :control-message-too-large
                           :max-bytes max-control-line-bytes}))

          :else
          (do (.write output value) (recur)))))))

(defn start-control-reader! [control-queue event-queue]
  (future
    (try
      (loop []
        (if-let [line (read-control-line! System/in)]
          (let [message (canonical-control line)]
            ;; Drain is an event-loop command, never an acknowledgement. Keeping
            ;; it out of CONTROL-QUEUE makes it safe to request while one
            ;; admission is still unwinding its ack/nack boundary.
            (.put (if (= "drain" (get message "type"))
                    event-queue
                    control-queue)
                  (if (= "drain" (get message "type"))
                    {:kind :drain :epoch (get message "epoch")}
                    {:kind :message :message message}))
              (recur))
          (.put control-queue {:kind :eof})))
      (catch Exception error
        (.put control-queue
              {:kind :error
               :error-type
               (or (:type (ex-data error)) :control-reader-failed)})))))

(defn emit! [message]
  (let [line (json/generate-string message)
        size (utf8-bytes line)]
    (when (> size max-output-message-bytes)
      (throw (ex-info "live-feed output message exceeds its byte bound"
                      {:type :output-message-too-large
                       :max-bytes max-output-message-bytes
                       :message-type (get message "type")})))
    (println line)
    (flush)))

(defn emit-ready! [recipient cursor]
  (emit! (array-map "protocol" protocol
                    "type" "ready"
                    "recipient" recipient
                    "cursor" cursor)))

(defn emit-caught-up! [recipient]
  (emit! (array-map "protocol" protocol
                    "type" "caught_up"
                    "recipient" recipient)))

(defn emit-drain-progress! [recipient epoch settled]
  (emit! (array-map "protocol" protocol
                    "type" "drain_progress"
                    "recipient" recipient
                    "epoch" epoch
                    "settled" settled)))

(defn emit-drained! [recipient epoch]
  (emit! (array-map "protocol" protocol
                    "type" "drained"
                    "recipient" recipient
                    "epoch" epoch)))

(defn emit-mail! [id from subject body wake-attempt]
  (emit! (array-map "protocol" protocol
                    "type" "mail"
                    "id" id
                    "from" from
                    "subject" subject
                    "body" body
                    "wakeAttempt" wake-attempt)))

(defn emit-error! [code id]
  (emit! (cond-> (array-map "protocol" protocol
                            "type" "error"
                            "code" code)
           id (assoc "id" id))))

(defn await-control! [queue expected-type expected-id timeout-ms]
  (let [event (if (some? timeout-ms)
                (.poll queue timeout-ms java.util.concurrent.TimeUnit/MILLISECONDS)
                (.take queue))]
    (cond
      (nil? event)
      (throw (ex-info "host control acknowledgement timed out"
                      {:type :control-timeout}))

      (= :eof (:kind event))
      (throw (ex-info "host control stream closed"
                      {:type :control-eof}))

      (= :error (:kind event))
      (throw (ex-info "host control stream failed"
                      {:type (:error-type event)}))

      (and (= :message (:kind event))
           (or (nil? expected-type)
               (= expected-type (get-in event [:message "type"])))
           (or (some? expected-type)
               (contains? #{"ack" "nack"}
                          (get-in event [:message "type"])))
           (or (nil? expected-id)
               (= expected-id (get-in event [:message "id"]))))
      (:message event)

      :else
      (throw (ex-info "host control message is out of sequence"
                      {:type :unexpected-control-message})))))

(defn role-slug [role]
  (when (and (string? role) (str/starts-with? role "@role:"))
    (subs role (count "@role:"))))

(defn current-direct-addresses [port recipient]
  (into #{recipient}
        (keep role-slug
              (north.coord/many port (str "@agent:" recipient) "holds"))))

(defn currently-deliverable? [port recipient message to]
  ;; Role authority is re-read after the delivery claim and immediately before
  ;; output. A stale startup snapshot can discover a candidate, but it cannot
  ;; authorize mail after the role has been retracted.
  (and (string? to)
       (north.message-audience/deliverable?
        port
        message
        to
        recipient
        (if (or (= to recipient)
                (= to north.message-audience/broadcast-address))
          #{recipient}
          (current-direct-addresses port recipient)))))

(defn agent-facts [port control]
  (try
    (north.lifecycle-projection/folded-agent-point-facts
     (fn [subject predicate] (north.coord/many port subject predicate))
     (str "@agent:" control))
    (catch Exception _ nil)))

(defn route-guard-facts [port control]
  (try
    (north.lifecycle-projection/raw-point-facts
     (fn [subject predicate] (north.coord/many port subject predicate))
     (str "@agent:" control)
     north.lifecycle-projection/route-guard-predicates)
    (catch Exception _ nil)))

(defn agent-run-entries [port control]
  (try
    (let [response
          (north.coord/query-page
           port
           {:find "live_route_run_candidate"
            :rules
            [{:head {:rel "live_route_run_candidate"
                     :args [{:var "e"}]}
              :body [{:rel "triple"
                      :args [{:var "e"} "agent" control]}]}]}
           max-live-route-run-candidates nil)
          rows (:rows response)]
      (when (and (map? response)
                 (vector? rows)
                 (true? (:done? response))
                 (<= (count rows) max-live-route-run-candidates)
                 (every? #(and (vector? %) (= 1 (count %))
                               (every? string? %))
                         rows))
        (->> rows
             (map first)
             (filter north.terminal-projection/valid-run-entity?)
             distinct
             sort
             (mapv
              (fn [subject]
                {:subject subject
                 :facts
                 (into {}
                       (keep
                        (fn [predicate]
                          (let [values
                                (set (north.coord/many port subject predicate))]
                            (when (seq values) [predicate values]))))
                       north.terminal-projection/run-resolution-predicates)})))))
    (catch Exception _ nil)))

(defn route-resolution [port control facts]
  (let [runs (agent-run-entries port control)]
    (if (and (map? facts) (vector? runs))
      (north.terminal-projection/lane-resolution control facts runs)
      {:status :indeterminate :reason :lifecycle-projection-unavailable})))

(defn require-open-lane! [port control]
  (let [facts (agent-facts port control)
        resolution (route-resolution port control facts)]
    (case (:status resolution)
      :unresolved true
      :resolved
      (throw (ex-info "live input target is terminal"
                      {:type :terminal-live-input-target
                       :recipient control}))
      (throw (ex-info "live input target lifecycle is inconsistent"
                      {:type :indeterminate-live-input-target
                       :recipient control
                       :reason (:reason resolution)})))))

(defn require-frozen-route-epoch! [port recipient epoch]
  (let [facts (agent-facts port recipient)
        resolution (route-resolution port recipient facts)]
    (when-not
     (and (safe-route-epoch? epoch)
          (map? facts)
          (north.agent-provenance/managed-valid? facts)
          (= "streaming" (get facts "live_input"))
          (= "frozen" (get facts "live_input_state"))
          (= epoch (get facts "live_input_epoch"))
          (= :unresolved (:status resolution)))
      (throw
       (ex-info
        "terminal drain does not match the current frozen route generation"
        {:type :terminal-msg-drain-route-mismatch
         :recipient recipient
         :epoch epoch})))
    facts))

(defn msg-route-status-from-facts
  [port message to subject facts resolution]
  (let [msg-shaped-subject?
        (= "msg" (some-> subject str str/trim str/lower-case))
        canonical-msg-subject? (= "msg" subject)
        expected
        (north.coord/resolved
         port message target-identity-manifest-predicate)
        managed-msg? (some? expected)
        observed
        (when (map? facts)
          (north.terminal-projection/singleton-value
           facts "identity_manifest_sha256"))
        live-input
        (when (map? facts)
          (north.terminal-projection/singleton-value facts "live_input"))
        live-input-state
        (when (map? facts)
          (north.terminal-projection/singleton-value facts "live_input_state"))
        live-input-epoch
        (when (map? facts)
          (north.terminal-projection/singleton-value facts "live_input_epoch"))
        wake-attempt (north.coord/resolved port message "wake_attempt_id")
        wake-epoch (north.coord/resolved port message "wake_listener_epoch")
        wake-manifest
        (north.coord/resolved port message "wake_listener_manifest_sha256")]
    (if-not (or msg-shaped-subject? managed-msg?)
      {:valid? true}
      (cond
        (and managed-msg? (not canonical-msg-subject?))
        {:valid? false :reason "msg_type_invalid"
         :expected-manifest
         (when (and (string? expected)
                    (re-matches #"^[0-9a-f]{64}$" expected))
           expected)
         :observed-manifest
         (when (and (string? observed)
                    (re-matches #"^[0-9a-f]{64}$" observed))
           observed)}

        (not (and (string? expected)
                  (re-matches #"^[0-9a-f]{64}$" expected)))
        {:valid? false :reason "msg_manifest_missing"}

        ;; A supported route mutation withdraws the full identity marker before
        ;; changing any route axis and recommits it last. Point-reading that
        ;; marker plus the complete route guard therefore detects every supported
        ;; torn generation without reloading the whole identity per message.
        (or (not (map? facts))
            (not (and (string? observed)
                      (re-matches #"^[0-9a-f]{64}$" observed)))
            (not (contains? #{"streaming" "turn-messages" "unsupported"} live-input))
            (not (contains? #{"pending" "armed" "frozen"}
                            live-input-state))
            (not (safe-route-epoch? live-input-epoch)))
        {:valid? false :reason "msg_route_invalid"
         :expected-manifest expected
         :observed-manifest
         (when (and (string? observed)
                    (re-matches #"^[0-9a-f]{64}$" observed))
           observed)}

        (not= expected observed)
        {:valid? false :reason "msg_route_stale"
         :expected-manifest expected :observed-manifest observed}

        (= :resolved (:status resolution))
        {:valid? false :reason "msg_route_not_armed"
         :expected-manifest expected :observed-manifest observed}

        (= :indeterminate (:status resolution))
        {:valid? false :reason "msg_route_not_armed"
         :expected-manifest expected :observed-manifest observed}

        (or (not (contains? #{"streaming" "turn-messages"} live-input))
            (not= "armed" live-input-state))
        {:valid? false :reason "msg_route_not_armed"
         :expected-manifest expected :observed-manifest observed}

        (and (= "turn-messages" live-input)
             (not (and (string? wake-attempt)
                       (re-matches #"^wake:[0-9a-f]{64}$" wake-attempt)
                       (= live-input-epoch wake-epoch)
                       (= expected wake-manifest))))
        {:valid? false :reason "wake_identity_invalid"
         :expected-manifest expected :observed-manifest observed}

        :else
        {:valid? true
         :live-input live-input
         :live-input-epoch live-input-epoch
         :expected-manifest expected :observed-manifest observed}))))

(defn msg-route-status [port message to subject]
  (let [facts (route-guard-facts port to)]
    (msg-route-status-from-facts
     port message to subject facts (route-resolution port to facts))))

(defn current-msg-route? [port message to subject]
  (:valid? (msg-route-status port message to subject)))

(defn message-problem [id from subject body wake-attempt]
  (or
   (when-not (safe-control-id? id) "invalid_message_id")
   (when-not (or (nil? wake-attempt)
                 (and (string? wake-attempt)
                      (re-matches #"^wake:[0-9a-f]{64}$" wake-attempt)))
     "invalid_wake_attempt")
   (north.message-contract/sender-problem from)
   (north.message-contract/subject-problem subject)
   (north.message-contract/body-problem body)
   (when
       (> (utf8-bytes
           (json/generate-string
            (array-map "protocol" protocol
                       "type" "mail"
                       "id" id
                       "from" from
                       "subject" subject
                       "body" body
                       "wakeAttempt" wake-attempt)))
          max-output-message-bytes)
     "message_too_large")))

(defn deliver-message!
  ([port recipient message control-queue claim-ttl-ms ack-timeout-ms]
   (deliver-message!
    port recipient message control-queue claim-ttl-ms ack-timeout-ms
    (fn [candidate to subject]
      (msg-route-status port candidate to subject))))
  ([port recipient message control-queue claim-ttl-ms ack-timeout-ms
    route-status]
   (when-let [claim
              (north.message-audience/claim-delivery!
               port message recipient claim-ttl-ms)]
     (let [to (north.coord/resolved port message "to")
           from (north.coord/resolved port message "from")
           subject (north.coord/resolved port message "subject")
           body (north.coord/resolved port message "body")
           wake-attempt (north.coord/resolved port message "wake_attempt_id")
           problem (message-problem message from subject body wake-attempt)
           msg-status (route-status message to subject)]
       (cond
         (not (currently-deliverable? port recipient message to))
         (do
           (north.message-audience/release-delivery-claim! port claim)
           :skipped)

         problem
         (do
           (north.message-audience/reject-delivery!
            port message recipient claim {:reason problem})
           (emit-error! problem (when (safe-control-id? message) message))
           :rejected)

         (not (:valid? msg-status))
         (do
           (north.message-audience/reject-delivery!
            port message recipient claim msg-status)
           (emit-error! (:reason msg-status)
                        (when (safe-control-id? message) message))
           :rejected)

         :else
         (try
           (emit-mail! message from subject body wake-attempt)
           (let [control (await-control!
                          control-queue nil message ack-timeout-ms)]
             (if (= "ack" (get control "type"))
               (do
                 (north.message-audience/complete-delivery!
                  port message recipient claim)
                 :acked)
               (do
                 (north.message-audience/release-delivery-claim! port claim)
                 :restart)))
           (catch Exception error
             (north.message-audience/release-delivery-claim! port claim)
             (throw error))))))))

(defn replay-pending!
  [port recipient direct-addresses control-queue claim-ttl-ms ack-timeout-ms]
  ;; Settle one bounded first page, then query the first page again. Ack/rejection
  ;; writes change the pending relation, so no cross-mutation cursor is needed
  ;; and a backlog of any size drains with constant client/wire memory.
  (loop []
    (let [page
          (north.message-audience/pending-message-page
           port recipient direct-addresses)
          messages (:messages page)]
      (if (seq messages)
        (let [results
              (mapv
               #(deliver-message!
                 port recipient % control-queue
                 claim-ttl-ms ack-timeout-ms)
               messages)]
          (cond
            ;; The whole page left the pending relation; query the bounded first
            ;; page again until empty.
            (every? #{:acked :rejected} results)
            (recur)

            ;; A nil claim or host nack stays pending. Claim expiry emits no
            ;; coordinator commit, so the caller must arm one timed replay.
            (some #(or (nil? %) (= :restart %)) results)
            :blocked

            ;; :skipped means address authority changed after the page snapshot.
            ;; A later holds/to commit is the correct wake edge.
            :else
            :idle))
        :idle))))

(defn settle-terminal-msgs!
  "After the managed route is durably frozen, terminally reject every
   producer-admitted msg that remains undelivered. The final `to` producer CAS
   orders every accepted msg before the freeze commit, so an empty query is a
   teardown barrier. A foreign delivery claim can outlive its consumer without
   emitting a commit; retry through one full claim TTL, bounded and backoff-led."
  [port recipient direct-addresses control-queue
   claim-ttl-ms ack-timeout-ms epoch]
  (let [idle-bound-ms (+ claim-ttl-ms ack-timeout-ms 1000)]
    (loop [settled 0 blocked-since nil backoff-ms 25]
      (let [frozen-facts (require-frozen-route-epoch! port recipient epoch)
            messages
            (:messages
             (north.message-audience/pending-msg-page
              port recipient direct-addresses))
            frozen-route-status
            (fn [message to subject]
              (msg-route-status-from-facts
               port message to subject frozen-facts
               {:status :unresolved :reason :frozen-route-validated}))]
        (if (empty? messages)
          (do
            ;; The receipt is generation-specific: re-read after observing the
            ;; empty relation so re-arm/fallback cannot race a stale `drained`.
            (require-frozen-route-epoch! port recipient epoch)
            settled)
          (let [page-result
                (reduce
                 (fn [{:keys [settled blocked? progress?] :as state} message]
                   (let [result
                         (deliver-message!
                          port recipient message control-queue
                          claim-ttl-ms ack-timeout-ms frozen-route-status)]
                     (case result
                       :rejected
                       (let [next-settled (inc settled)]
                         ;; Each durable settlement is a watchdog heartbeat.
                         ;; A backlog of any size may take time, but it cannot
                         ;; look indistinguishable from a wedged claim.
                         (emit-drain-progress!
                          recipient epoch next-settled)
                         (assoc state
                                :settled next-settled
                                :progress? true))

                       :acked
                       (throw
                        (ex-info
                         "frozen terminal drain acknowledged a msg"
                         {:type :terminal-msg-drain-contradiction
                          :message message}))

                       ;; nil foreign claim, nack/restart, or address change:
                       ;; leave it pending and continue settling the rest of the
                       ;; bounded page before one backoff-led retry.
                       (assoc state :blocked? true))))
                 {:settled settled :blocked? false :progress? false}
                 messages)
                next-settled (:settled page-result)]
            (if-not (:blocked? page-result)
              (recur next-settled nil 25)
              (let [now (System/currentTimeMillis)
                    since (if (:progress? page-result)
                            now
                            (or blocked-since now))]
                (when (>= (- now since) idle-bound-ms)
                  (throw
                   (ex-info
                    "terminal msg drain made no progress through one claim bound"
                    {:type :terminal-msg-drain-timeout
                     :recipient recipient
                     :epoch epoch})))
                (Thread/sleep backoff-ms)
                (recur next-settled since
                       (min 250 (* 2 backoff-ms)))))))))))

(defn run-poll-feed!
  [port recipient settlement-only? deferred-start? claim-ttl-ms ack-timeout-ms
   control-queue event-queue]
  (let [interval (poll-ms)
        baseline (north.coord/cur-ver port)]
    ;; The successful pinned version read establishes the poller before
    ;; readiness is observable. Durable replay closes the ready/start gap.
    (require-open-lane! port recipient)
    (emit-ready! recipient baseline)
    (await-control! control-queue "start" nil
                    (when-not deferred-start? ack-timeout-ms))
    (let [addrs (atom (current-direct-addresses port recipient))
          initial
          (when-not settlement-only?
            (replay-pending!
             port recipient @addrs control-queue
             claim-ttl-ms ack-timeout-ms))]
      ;; The first replay attempt is the host's terminal-boundary barrier. A
      ;; delivered message keeps replay-pending! blocked through host admission
      ;; and durable graph acknowledgement; a nacked or unavailable claim stays
      ;; graph-pending but no longer delays this provider session.
      (when deferred-start?
        (emit-caught-up! recipient))
      (loop [cursor baseline
             retry-at
             (when (= :blocked initial)
               (+ (System/currentTimeMillis) claim-ttl-ms))]
      (let [now (System/currentTimeMillis)
            until-retry (when retry-at (max 1 (- retry-at now)))
            wait-ms (if until-retry
                      (min interval until-retry)
                      interval)
            item (.poll event-queue wait-ms
                        java.util.concurrent.TimeUnit/MILLISECONDS)]
        (cond
          (= :drain (:kind item))
          (let [epoch (:epoch item)]
            (reset! addrs (current-direct-addresses port recipient))
            (settle-terminal-msgs!
             port recipient @addrs control-queue
             claim-ttl-ms ack-timeout-ms epoch)
            (emit-drained! recipient epoch)
            (recur cursor nil))

          (some? item)
          (throw
           (ex-info "Store RPC live-feed received an invalid local event"
                    {:type :invalid-store-rpc-live-feed-event
                     :event item}))

          :else
          (let [head (north.coord/cur-ver port)
                now (System/currentTimeMillis)
                changed? (not= cursor head)
                retry? (and retry-at (<= retry-at now))
                result
                (when (and (not settlement-only?)
                           (or changed? retry?))
                  (reset! addrs (current-direct-addresses port recipient))
                  (replay-pending!
                   port recipient @addrs control-queue
                   claim-ttl-ms ack-timeout-ms))]
            ;; HEAD was sampled before replay. A concurrent later commit
            ;; remains greater than this cursor and wakes the next poll.
            (recur head
                   (cond
                     (= :blocked result) (+ now claim-ttl-ms)
                     (and (nil? result) (not retry?)) retry-at
                     :else nil)))))))))

(defn run-feed! [port recipient flags]
  (validate-flags! flags)
  (let [claim-ttl-ms
        (bounded-positive
         "--claim-ttl-ms"
         (flag-value flags "--claim-ttl-ms")
         north.message-audience/delivery-claim-ttl-ms
         north.message-audience/delivery-claim-ttl-ms)
        ack-timeout-ms
        (bounded-positive
         "--ack-timeout-ms"
         (flag-value flags "--ack-timeout-ms")
         default-ack-timeout-ms
         claim-ttl-ms)
        settlement-only-raw (flag-value flags "--settlement-only")
        _ (when-not (or (nil? settlement-only-raw)
                        (= "true" settlement-only-raw))
            (throw
             (ex-info "--settlement-only accepts only true"
                      {:type :invalid-live-feed-option})))
        settlement-only? (= "true" settlement-only-raw)
        deferred-start-raw (flag-value flags "--deferred-start")
        _ (when-not (or (nil? deferred-start-raw)
                        (= "true" deferred-start-raw))
            (throw
             (ex-info "--deferred-start accepts only true"
                      {:type :invalid-live-feed-option})))
        deferred-start? (= "true" deferred-start-raw)
        _ (when (>= ack-timeout-ms claim-ttl-ms)
            (throw (ex-info "--ack-timeout-ms must be smaller than --claim-ttl-ms"
                            {:type :invalid-live-feed-option})))
        recipient (north.message-audience/bare-handle recipient)
        _ (when-not (and
                     (<= (utf8-bytes recipient)
                         north.message-contract/max-target-bytes)
                     (boolean
                      (re-matches #"^[A-Za-z0-9][A-Za-z0-9._:-]*$" recipient)))
            (throw (ex-info "recipient is malformed"
                            {:type :invalid-live-feed-recipient})))
        control-queue (java.util.concurrent.LinkedBlockingQueue.
                       control-queue-capacity)
        event-queue (java.util.concurrent.LinkedBlockingQueue.
                     event-queue-capacity)]
    (start-control-reader! control-queue event-queue)
    (run-poll-feed!
     port recipient settlement-only? deferred-start? claim-ttl-ms ack-timeout-ms
     control-queue event-queue)))

(when-not (= "1" (System/getProperty "north.live-feed.lib"))
  (let [[port recipient & flags] *command-line-args*]
    (try
      (when (or (str/blank? port) (str/blank? recipient))
        (throw (ex-info
                "usage: north-live-feed.clj <port> <recipient> [--claim-ttl-ms N] [--ack-timeout-ms N] [--settlement-only true] [--deferred-start true]"
                {:type :usage})))
      (run-feed! (Integer/parseInt port) recipient (vec flags))
      (catch Exception error
        (binding [*out* *err*]
          (println (str "north-live-feed: "
                        (or (some-> error ex-data :type name)
                            "failed"))))
        (System/exit 1)))))
