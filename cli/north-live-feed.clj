#!/usr/bin/env bb
;; Durable machine bridge from North mail facts to one managed SDK input channel.
;;
;; Protocol (north-live-feed-v1), one canonical JSON object per line:
;;   stdout: {"protocol":"north-live-feed-v1","type":"ready",...}
;;   stdin:  {"type":"start"}
;;   stdout: {"protocol":"north-live-feed-v1","type":"mail",...}
;;   stdin:  {"type":"ack","id":"@msg:..."} | {"type":"nack","id":"@msg:..."}
;;   stdin:  {"type":"drain","epoch":"<frozen-route-uuid>"}
;;   stdout: {"protocol":"north-live-feed-v1","type":"drain_progress",...}
;;   stdout: {"protocol":"north-live-feed-v1","type":"drained",...}
;;
;; The coordinator subscription is armed before `ready`, and pending mail is
;; replayed only after the host answers `start`. Live commits arriving during
;; replay remain queued on the already-armed subscription. A delivery claim is
;; held until the host admits the frame and answers `ack`; EOF, timeout, nack, or
;; a crash leaves the message unacknowledged and therefore replayable.
(require '[cheshire.core :as json]
         '[clojure.set :as set]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-audience.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-contract.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/agent-provenance.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))

(def protocol "north-live-feed-v1")
(def max-control-line-bytes 4096)
(def max-output-frame-bytes (* 192 1024))
(def event-queue-capacity 1024)
(def control-queue-capacity 32)
(def default-ack-timeout-ms 10000)
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
                           "--settlement-only"} flag)
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
                      (throw (ex-info "control frame is not valid JSON"
                                      {:type :invalid-control-frame} error))))
        frame
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
          (throw (ex-info "control frame has an invalid shape"
                          {:type :invalid-control-frame})))]
    (when-not (= line (json/generate-string frame))
      (throw (ex-info "control frame is not canonical JSON"
                      {:type :noncanonical-control-frame})))
    frame))

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
            (throw (ex-info "control stream closed during a frame"
                            {:type :truncated-control-frame})))

          (= 10 value)
          (decode-utf8! (.toByteArray output))

          (>= (.size output) max-control-line-bytes)
          (throw (ex-info "control frame exceeds its byte bound"
                          {:type :control-frame-too-large
                           :max-bytes max-control-line-bytes}))

          :else
          (do (.write output value) (recur)))))))

(defn start-control-reader! [control-queue event-queue]
  (future
    (try
      (loop []
        (if-let [line (read-control-line! System/in)]
          (let [frame (canonical-control line)]
            ;; Drain is an event-loop command, never an acknowledgement. Keeping
            ;; it out of CONTROL-QUEUE makes it safe to request while one
            ;; admission is still unwinding its ack/nack boundary.
            (.put (if (= "drain" (get frame "type"))
                    event-queue
                    control-queue)
                  (if (= "drain" (get frame "type"))
                    {:kind :drain :epoch (get frame "epoch")}
                    {:kind :frame :frame frame}))
              (recur))
          (.put control-queue {:kind :eof})))
      (catch Exception error
        (.put control-queue
              {:kind :error
               :error-type
               (or (:type (ex-data error)) :control-reader-failed)})))))

(defn emit! [frame]
  (let [line (json/generate-string frame)
        size (utf8-bytes line)]
    (when (> size max-output-frame-bytes)
      (throw (ex-info "live-feed output frame exceeds its byte bound"
                      {:type :output-frame-too-large
                       :max-bytes max-output-frame-bytes
                       :frame-type (get frame "type")})))
    (println line)
    (flush)))

(defn emit-ready! [recipient subscribed]
  (emit! (array-map "protocol" protocol
                    "type" "ready"
                    "recipient" recipient
                    "subscribed" subscribed)))

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

(defn emit-mail! [id from subject body]
  (emit! (array-map "protocol" protocol
                    "type" "mail"
                    "id" id
                    "from" from
                    "subject" subject
                    "body" body)))

(defn emit-error! [code id]
  (emit! (cond-> (array-map "protocol" protocol
                            "type" "error"
                            "code" code)
           id (assoc "id" id))))

(defn await-control! [queue expected-type expected-id timeout-ms]
  (let [event (.poll queue timeout-ms java.util.concurrent.TimeUnit/MILLISECONDS)]
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

      (and (= :frame (:kind event))
           (or (nil? expected-type)
               (= expected-type (get-in event [:frame "type"])))
           (or (some? expected-type)
               (contains? #{"ack" "nack"}
                          (get-in event [:frame "type"])))
           (or (nil? expected-id)
               (= expected-id (get-in event [:frame "id"]))))
      (:frame event)

      :else
      (throw (ex-info "host control frame is out of sequence"
                      {:type :unexpected-control-frame})))))

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
  (let [response
        (north.coord/send-op
         port
         {:op :query
          :query
          {:find "live_route_fact"
           :rules
           [{:head {:rel "live_route_fact"
                    :args [{:var "p"} {:var "r"}]}
             :body [{:rel "triple"
                     :args [(str "@agent:" control) {:var "p"} {:var "r"}]}]}]}})
        rows (:ok response)]
    (when (and (map? response)
               (vector? rows)
               (<= (count rows) 256)
               (every? #(and (vector? %) (= 2 (count %))
                             (every? string? %))
                       rows))
      (reduce (fn [facts [predicate value]]
                (north.agent-provenance/fold-fact facts predicate value))
              {}
              rows))))

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
          rows (:ok response)]
      (when (and (map? response)
                 (vector? rows)
                 (false? (:more response))
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
        {:type :terminal-steer-drain-route-mismatch
         :recipient recipient
         :epoch epoch})))
    facts))

(defn steer-route-status [port message to subject]
  (let [steer-shaped-subject?
        (= "steer" (some-> subject str str/trim str/lower-case))
        canonical-steer-subject? (= "steer" subject)
        expected
        (north.coord/resolved
         port message target-identity-manifest-predicate)
        managed-steer? (some? expected)]
    (if-not (or steer-shaped-subject? managed-steer?)
      {:valid? true}
      (let [facts (agent-facts port to)
            resolution (route-resolution port to facts)
            observed (when (map? facts)
                       (get facts "identity_manifest_sha256"))]
        (cond
        (and managed-steer? (not canonical-steer-subject?))
        {:valid? false :reason "steer_type_invalid"
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
        {:valid? false :reason "steer_manifest_missing"}

        (or (not (map? facts))
            (not (north.agent-provenance/managed-valid? facts)))
        {:valid? false :reason "steer_route_invalid"
         :expected-manifest expected
         :observed-manifest
         (when (and (string? observed)
                    (re-matches #"^[0-9a-f]{64}$" observed))
           observed)}

        (not= expected observed)
        {:valid? false :reason "steer_route_stale"
         :expected-manifest expected :observed-manifest observed}

        (= :resolved (:status resolution))
        {:valid? false :reason "steer_route_not_armed"
         :expected-manifest expected :observed-manifest observed}

        (= :indeterminate (:status resolution))
        {:valid? false :reason "steer_route_not_armed"
         :expected-manifest expected :observed-manifest observed}

        (or (not= "streaming" (get facts "live_input"))
            (not= "armed" (get facts "live_input_state")))
        {:valid? false :reason "steer_route_not_armed"
         :expected-manifest expected :observed-manifest observed}

        :else
          {:valid? true
           :expected-manifest expected :observed-manifest observed})))))

(defn current-steer-route? [port message to subject]
  (:valid? (steer-route-status port message to subject)))

(defn message-problem [id from subject body]
  (or
   (when-not (safe-control-id? id) "invalid_message_id")
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
                       "body" body)))
          max-output-frame-bytes)
     "message_frame_too_large")))

(defn deliver-message!
  [port recipient message control-queue claim-ttl-ms ack-timeout-ms]
  (when-let [claim
             (north.message-audience/claim-delivery!
              port message recipient claim-ttl-ms)]
    (let [to (north.coord/resolved port message "to")
          from (north.coord/resolved port message "from")
          subject (north.coord/resolved port message "subject")
          body (north.coord/resolved port message "body")
          problem (message-problem message from subject body)
          steer-status (steer-route-status port message to subject)]
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

        (not (:valid? steer-status))
        (do
          (north.message-audience/reject-delivery!
           port message recipient claim steer-status)
          (emit-error! (:reason steer-status)
                       (when (safe-control-id? message) message))
          :rejected)

        :else
        (try
          (emit-mail! message from subject body)
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
            (throw error)))))))

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

(defn settle-terminal-steers!
  "After the managed route is durably frozen, terminally reject every
   producer-admitted steer that remains undelivered. The final `to` producer CAS
   orders every accepted steer before the freeze commit, so an empty query is a
   teardown barrier. A foreign delivery claim can outlive its consumer without
   emitting a commit; retry through one full claim TTL, bounded and backoff-led."
  [port recipient direct-addresses control-queue
   claim-ttl-ms ack-timeout-ms epoch]
  (let [idle-bound-ms (+ claim-ttl-ms ack-timeout-ms 1000)]
    (loop [settled 0 blocked-since nil backoff-ms 25]
      (require-frozen-route-epoch! port recipient epoch)
      (let [messages
            (:messages
             (north.message-audience/pending-steer-page
              port recipient direct-addresses))]
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
                          claim-ttl-ms ack-timeout-ms)]
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
                         "frozen terminal drain acknowledged a steer"
                         {:type :terminal-steer-drain-contradiction
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
                    "terminal steer drain made no progress through one claim bound"
                    {:type :terminal-steer-drain-timeout
                     :recipient recipient
                     :epoch epoch})))
                (Thread/sleep backoff-ms)
                (recur next-settled since
                       (min 250 (* 2 backoff-ms)))))))))))

(defn start-event-reader! [reader queue]
  (future
    (try
      (loop []
        (if-let [line (north.coord/read-stream-line-bounded! reader)]
          (do
            (.put queue {:kind :event
                         :event (try (edn/read-string line)
                                     (catch Exception error
                                       (throw (ex-info "coordinator event is malformed"
                                                       {:type :malformed-event}
                                                       error))))})
            (recur))
          (.put queue {:kind :eof})))
      (catch Exception error
        (.put queue {:kind :error
                     :error-type (or (:type (ex-data error)) :event-reader-failed)})))))

(defn process-event!
  [port recipient node addrs event control-queue claim-ttl-ms ack-timeout-ms]
  (when (and (map? event) (= :commit (:event event)))
    (let [{:keys [op l p r]} event]
      (cond
        (and (= l node) (= p "holds"))
        (let [before @addrs
              after (current-direct-addresses port recipient)]
          (reset! addrs after)
          ;; Acquiring a role makes already-committed role mail eligible. The
          ;; stream's holds commit is the wake edge; replay supplies the past.
          (when (seq (set/difference after before))
            (replay-pending!
             port recipient after control-queue claim-ttl-ms ack-timeout-ms)))

        (and (= op "assert")
             (= p "to")
             (north.message-audience/deliverable?
              port l r recipient @addrs))
        (let [result
              (deliver-message!
               port recipient l control-queue claim-ttl-ms ack-timeout-ms)]
          (when (or (nil? result) (= :restart result)) :blocked))))))

(defn write-subscribe! [socket]
  (let [writer (.getOutputStream socket)
        envelope (north.coord/log-envelope {:op :subscribe})
        wire (.getBytes (str (pr-str envelope) "\n")
                        java.nio.charset.StandardCharsets/UTF_8)]
    (.write writer wire)
    (.flush writer)))

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
        node (str "@agent:" recipient)
        control-queue (java.util.concurrent.LinkedBlockingQueue.
                       control-queue-capacity)
        event-queue (java.util.concurrent.LinkedBlockingQueue.
                     event-queue-capacity)]
    (start-control-reader! control-queue event-queue)
    (with-open [socket (north.coord/connect-socket port)]
      (write-subscribe! socket)
      (let [reader (north.coord/coordinator-reader socket)
            handshake
            (north.coord/validate-subscription!
             (north.coord/read-line-bounded! reader))]
        ;; Start draining the armed stream before making readiness observable.
        (start-event-reader! reader event-queue)
        ;; A committed terminal run is execution truth even when a crashed
        ;; writer never copied it to @agent. Check after subscription arm and
        ;; immediately before readiness becomes observable.
        (require-open-lane! port recipient)
        (emit-ready! recipient (:subscribed handshake))
        (await-control! control-queue "start" nil ack-timeout-ms)
        ;; Role/address snapshot happens after arm. Any concurrent holds/to
        ;; commits are already queued above, and replay/live claim overlap is
        ;; idempotent.
        (let [addrs (atom (current-direct-addresses port recipient))
              initial
              (when-not settlement-only?
                (replay-pending!
                 port recipient @addrs control-queue
                 claim-ttl-ms ack-timeout-ms))]
          (loop [retry-at
                 (when (= :blocked initial)
                   (+ (System/currentTimeMillis) claim-ttl-ms))]
            (let [now (System/currentTimeMillis)]
              ;; A retry deadline is a clock edge independent of coordinator
              ;; traffic. Prioritize it once due; an unrelated commit storm must
              ;; not keep a nonempty event queue postponing claim-expiry replay.
              (if (and retry-at (<= retry-at now))
                (let [result
                      (replay-pending!
                       port recipient @addrs control-queue
                       claim-ttl-ms ack-timeout-ms)]
                  (recur
                   (when (= :blocked result)
                     (+ (System/currentTimeMillis) claim-ttl-ms))))
                (let [wait-ms (when retry-at (- retry-at now))
                      item (if wait-ms
                             (.poll event-queue wait-ms
                                    java.util.concurrent.TimeUnit/MILLISECONDS)
                             (.take event-queue))]
                  (cond
                    (nil? item)
                    ;; The bounded poll reached the retry deadline. Re-enter at
                    ;; the loop head so clock priority has one implementation.
                    (recur retry-at)

                    (= :drain (:kind item))
                    (let [epoch (:epoch item)]
                      (settle-terminal-steers!
                       port recipient @addrs control-queue
                       claim-ttl-ms ack-timeout-ms epoch)
                      (emit-drained! recipient epoch)
                      (recur nil))

                    (= :event (:kind item))
                    (let [result
                          (when-not settlement-only?
                            (process-event!
                             port recipient node addrs (:event item) control-queue
                             claim-ttl-ms ack-timeout-ms))
                          candidate
                          (when (= :blocked result)
                            (+ (System/currentTimeMillis) claim-ttl-ms))]
                      ;; Never move an existing retry later. Multiple blocked
                      ;; observations share the earliest bounded clock edge.
                      (recur
                       (cond
                         (nil? candidate) retry-at
                         (nil? retry-at) candidate
                         :else (min retry-at candidate))))

                    (= :eof (:kind item))
                    (throw (ex-info "coordinator subscription closed"
                                    {:type :subscription-eof}))

                    :else
                    (throw (ex-info "coordinator subscription failed"
                                    {:type (:error-type item)}))))))))))))

(when-not (= "1" (System/getProperty "north.live-feed.lib"))
  (let [[port recipient & flags] *command-line-args*]
    (try
      (when (or (str/blank? port) (str/blank? recipient))
        (throw (ex-info
                "usage: north-live-feed.clj <port> <recipient> [--claim-ttl-ms N] [--ack-timeout-ms N] [--settlement-only true]"
                {:type :usage})))
      (run-feed! (Integer/parseInt port) recipient (vec flags))
      (catch Exception error
        (binding [*out* *err*]
          (println (str "north-live-feed: "
                        (or (some-> error ex-data :type name)
                            "failed"))))
        (System/exit 1)))))
