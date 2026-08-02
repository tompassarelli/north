#!/usr/bin/env bb
(ns north.framrpc-client
  (:require [clojure.string :as str]
            [coord-daemon-wire :as wire]
            [fram.types :as t])
  (:import [java.io IOException]
           [java.net InetSocketAddress Socket SocketTimeoutException]
           [java.nio ByteBuffer ByteOrder]
           [java.util.concurrent ThreadLocalRandom]
           [java.util.concurrent.atomic AtomicLong]))

(def max-body-bytes 1048576)
(def effective-page-limit 200)
(def retryable-error-codes
  #{:rpc/conflict :rpc/cancelled :query-cancelled :query-time-limit
    :query-work-limit :durability-ambiguous})
(def subscription-operation :rpc/subscribe)

(def ^:private mutation-operations
  #{:rpc/assert :rpc/retract :rpc/batch :rpc/lease-acquire
    :rpc/lease-renew :rpc/lease-release})
(def ^:private ambiguous-error-codes #{:durability-ambiguous})
(def ^:private request-sequence (AtomicLong. 0))

(defrecord Client
  [host port space-id connect-timeout-ms read-timeout-ms max-attempts
   retry-delay-ms jitter-ms closed])

(defn- positive-integer! [label value]
  (when-not (and (integer? value) (pos? value))
    (throw (ex-info (str label " must be a positive integer")
                    {:type :rpc/invalid-client-option
                     :option label :value value})))
  value)

(defn- nonnegative-integer! [label value]
  (when-not (and (integer? value) (not (neg? value)))
    (throw (ex-info (str label " must be a non-negative integer")
                    {:type :rpc/invalid-client-option
                     :option label :value value})))
  value)

(defn- next-request-id []
  (let [value (.incrementAndGet request-sequence)]
    (if (pos? value)
      value
      (do (.set request-sequence 1) 1))))

(defn- read-exact! [input bytes offset length]
  (loop [position offset remaining length]
    (if (zero? remaining)
      true
      (let [read-count (.read input bytes position remaining)]
        (if (neg? read-count)
          false
          (recur (+ position read-count) (- remaining read-count)))))))

(defn- header-body-length! [header]
  (dotimes [index 8]
    (when-not (= (bit-and 255 (int (aget header index)))
                 (bit-and 255 (int (aget wire/rpc-v1-magic index))))
      (throw (ex-info "FRAMRPC response magic does not match"
                      {:type :rpc-invalid-magic}))))
  (let [buffer (doto (ByteBuffer/wrap header) (.order ByteOrder/LITTLE_ENDIAN))]
    (.position buffer 8)
    (let [major (Short/toUnsignedInt (.getShort buffer))
          minor (Short/toUnsignedInt (.getShort buffer))
          kind (bit-and 255 (int (.get buffer)))
          flags (bit-and 255 (int (.get buffer)))
          body-length (Integer/toUnsignedLong (.getInt buffer))]
      (when-not (and (= major wire/rpc-v1-major)
                     (= minor wire/rpc-v1-minor))
        (throw (ex-info "FRAMRPC response version is unsupported"
                        {:type :rpc-unsupported-version
                         :major major :minor minor})))
      (when-not (contains? #{2 4} kind)
        (throw (ex-info "FRAMRPC client expected a response or event frame"
                        {:type :rpc-invalid-kind :kind kind})))
      (when-not (zero? flags)
        (throw (ex-info "FRAMRPC response flags must be zero"
                        {:type :rpc-invalid-flags :flags flags})))
      (when (> body-length max-body-bytes)
        (throw (ex-info "FRAMRPC response body exceeds the 1 MiB limit"
                        {:type :rpc-frame-too-large
                         :body-length body-length
                         :limit max-body-bytes})))
      (int body-length))))

(defn read-frame!
  "Read one bounded FRAMRPC response/event frame without allocating an
   untrusted declared body."
  [input]
  (let [header (byte-array wire/rpc-v1-header-bytes)]
    (when-not (read-exact! input header 0 wire/rpc-v1-header-bytes)
      (throw (ex-info "FRAMRPC response ended inside its header"
                      {:type :rpc-truncated})))
    (let [body-length (header-body-length! header)
          body (byte-array body-length)
          frame (byte-array (+ wire/rpc-v1-header-bytes body-length))]
      (when-not (read-exact! input body 0 body-length)
        (throw (ex-info "FRAMRPC response ended inside its body"
                        {:type :rpc-truncated})))
      (System/arraycopy header 0 frame 0 wire/rpc-v1-header-bytes)
      (System/arraycopy body 0 frame wire/rpc-v1-header-bytes body-length)
      (wire/decode-rpc-frame-v1! frame))))

(defn encode-request-frame!
  "Encode one request through TermCodecV1 and enforce the shared body limit."
  [request-id request]
  (wire/encode-rpc-frame-v1! (wire/rpc-request-frame request-id request)))

(defn transport-round-trip!
  "Perform one unary socket exchange. The daemon owns one request per socket."
  [client request]
  (let [request-id (next-request-id)
        sent? (atom false)]
    (try
      (let [bytes (encode-request-frame! request-id request)]
        (with-open [socket (Socket.)]
          (.connect socket
                    (InetSocketAddress. ^String (:host client) (int (:port client)))
                    (int (:connect-timeout-ms client)))
          (.setSoTimeout socket (int (max (:read-timeout-ms client)
                                          (+ 1000 (or (t/rpcrequest-timeout-ms request)
                                                      0)))))
          (let [output (.getOutputStream socket)]
            (reset! sent? true)
            (.write output bytes)
            (.flush output))
          (let [frame (read-frame! (.getInputStream socket))
                response (t/rpcframev1-response frame)]
            (when-not (= :response (t/rpcframev1-kind frame))
              (throw (ex-info "FRAMRPC unary request received a non-response frame"
                              {:type :rpc-invalid-kind})))
            (when-not (= request-id (t/rpcframev1-request-id frame))
              (throw (ex-info "FRAMRPC response request-id does not match"
                              {:type :rpc-request-id-mismatch})))
            (when-not (and (= (t/rpcrequest-space request)
                              (t/rpcresponse-space response))
                           (= (t/rpcrequest-op request)
                              (t/rpcresponse-op response)))
              (throw (ex-info "FRAMRPC response identity does not match its request"
                              {:type :rpc-response-mismatch
                               :expected-space (t/rpcrequest-space request)
                               :actual-space (t/rpcresponse-space response)
                               :expected-op (t/rpcrequest-op request)
                               :actual-op (t/rpcresponse-op response)})))
            response)))
      (catch Throwable error
        (if (contains? (ex-data error) :request-sent?)
          (throw error)
          (throw (ex-info (or (.getMessage error) "FRAMRPC transport failed")
                          (assoc (or (ex-data error) {})
                                 :request-sent? @sent?)
                          error)))))))

(def ^:dynamic *round-trip!* transport-round-trip!)

(defn- open! [client]
  (when @(:closed client)
    (throw (ex-info "FRAMRPC client is closed" {:type :rpc/client-closed})))
  client)

(defn close! [client]
  (reset! (:closed client) true)
  nil)

(defn closed? [client] @(:closed client))

(defn- typed-response-error [response attempts]
  (let [error (t/rpcresponse-error response)
        code (t/rpcerror-code error)]
    (ex-info (t/rpcerror-message error)
             {:type code
              :code code
              :retryable (t/rpcerror-retryable error)
              :attempts attempts
              :served-version (t/rpcresponse-served-version response)
              :detail (t/rpc-error-detail-value error)})))

(defn- transport-error? [error]
  (or (instance? IOException error)
      (instance? SocketTimeoutException error)
      (contains? #{:rpc-truncated :rpc-invalid-magic :rpc-unsupported-version
                   :rpc-invalid-kind :rpc-invalid-flags :rpc-frame-too-large
                   :rpc-request-id-mismatch :rpc-response-mismatch}
                 (:type (ex-data error)))))

(defn- retry-pause! [client attempt]
  (let [base (* attempt (:retry-delay-ms client))
        jitter (if (pos? (:jitter-ms client))
                 (.nextLong (ThreadLocalRandom/current)
                            (long (inc (:jitter-ms client))))
                 0)]
    (Thread/sleep (+ base jitter))))

(defn- resolve-ambiguous! [resolver client request error attempt]
  (if-not resolver
    (throw (ex-info "FRAMRPC mutation outcome is ambiguous and no exact projection resolver was supplied"
                    {:type :rpc/ambiguous-write
                     :operation (t/rpcrequest-op request)
                     :attempts attempt}
                    error))
    (let [{:keys [resolution] :as decision}
          (resolver client request error)]
      (case resolution
        :committed {:resolved decision :attempts attempt}
        :retry :retry
        (throw (ex-info "FRAMRPC mutation could not be resolved from its exact projection"
                        {:type :rpc/ambiguous-write
                         :operation (t/rpcrequest-op request)
                         :attempts attempt
                         :resolution resolution}
                        error))))))

(defn- invoke!
  [client request {:keys [ambiguity-resolver]}]
  (open! client)
  (let [mutation? (contains? mutation-operations (t/rpcrequest-op request))]
    (loop [attempt 1]
      (let [result
            (try
              {:response (*round-trip!* client request)}
              (catch Throwable error {:transport-error error}))]
        (if-let [error (:transport-error result)]
          (if-not (transport-error? error)
            (throw error)
            (if (and mutation? (not= false (:request-sent? (ex-data error))))
              (let [decision (resolve-ambiguous!
                              ambiguity-resolver client request error attempt)]
                (if (= :retry decision)
                  (if (< attempt (:max-attempts client))
                    (do (retry-pause! client attempt) (recur (inc attempt)))
                    (throw (ex-info "FRAMRPC same-question retry budget exhausted"
                                    {:type :rpc/retry-exhausted
                                     :operation (t/rpcrequest-op request)
                                     :attempts attempt}
                                    error)))
                  decision))
              (if (< attempt (:max-attempts client))
                (do (retry-pause! client attempt) (recur (inc attempt)))
                (if (= false (:request-sent? (ex-data error)))
                  (throw error)
                  (throw (ex-info "FRAMRPC same-question retry budget exhausted"
                                  {:type :rpc/retry-exhausted
                                   :operation (t/rpcrequest-op request)
                                   :attempts attempt}
                                  error))))))
          (let [response (:response result)
                rpc-error (t/rpcresponse-error response)]
            (if-not rpc-error
              {:response response :attempts attempt}
              (let [code (t/rpcerror-code rpc-error)
                    retryable? (and (t/rpcerror-retryable rpc-error)
                                    (contains? retryable-error-codes code))]
                (cond
                  (and mutation? (contains? ambiguous-error-codes code))
                  (let [decision (resolve-ambiguous!
                                  ambiguity-resolver client request
                                  (typed-response-error response attempt) attempt)]
                    (if (= :retry decision)
                      (if (< attempt (:max-attempts client))
                        (do (retry-pause! client attempt) (recur (inc attempt)))
                        (throw (typed-response-error response attempt)))
                      decision))

                  (and retryable?
                       (not= code :rpc/conflict)
                       (< attempt (:max-attempts client)))
                  (do (retry-pause! client attempt) (recur (inc attempt)))

                  :else (throw (typed-response-error response attempt)))))))))))

(defn- request!
  [client operation payload options]
  (let [request (wire/rpc-request!
                 (:space-id client) operation
                 (:expected-version options) (:page options)
                 (:timeout-ms options) payload)]
    (invoke! client request options)))

(defn- result-map [{:keys [response attempts resolved]}]
  (if resolved
    {:resolved resolved :attempts attempts}
    {:served-version (t/rpcresponse-served-version response)
     :payload (t/rpc-response-payload-value response)
     :page (t/rpcresponse-page response)
     :attempts attempts}))

(defn- record-fields! [value tag count-value]
  (wire/rpc-record-fields! value tag count-value))

(defn- list-values! [value] (wire/rpc-list-values! value))

(defn- page-map [page]
  (when page
    {:ordinal (t/rpcpageresponse-ordinal page)
     :cursor (t/rpc-page-response-cursor-value page)
     :done? (t/rpcpageresponse-done page)}))

(defn- mutation-result [result]
  (let [{:keys [payload] :as base} (result-map result)]
    (if (:resolved base)
      base
      (let [[encoded-results] (record-fields! payload :rpc/mutation-result 1)]
        (assoc (dissoc base :payload :page)
               :results
               (mapv (fn [encoded]
                       (let [[input-index changed occurrences]
                             (record-fields! encoded :rpc/action-result 3)]
                         {:input-index input-index
                          :changed? changed
                          :occurrences (list-values! occurrences)}))
                     (list-values! encoded-results)))))))

(defn version! [client]
  (let [result (result-map (request! client :rpc/version wire/rpc-unit {}))]
    (dissoc result :payload :page)))

(defn status! [client]
  (let [{:keys [payload] :as result}
        (result-map (request! client :rpc/status wire/rpc-unit {}))
        [state live-count engine] (record-fields! payload :rpc/status 3)]
    (assoc (dissoc result :payload :page)
           :space-id (:space-id client)
           :state state :live-count live-count :engine engine)))

(declare connect)

(defn connect
  ([host port space-id] (connect host port space-id {}))
  ([host port space-id options]
   (when (str/blank? host)
     (throw (ex-info "FRAMRPC host must be nonblank"
                     {:type :rpc/invalid-client-option :option :host})))
   (when (str/blank? space-id)
     (throw (ex-info "FRAMRPC SpaceId must be nonblank"
                     {:type :rpc/invalid-client-option :option :space-id})))
   (let [client (->Client host
                          (positive-integer! :port port)
                          space-id
                          (positive-integer!
                           :connect-timeout-ms
                           (get options :connect-timeout-ms 2000))
                          (positive-integer!
                           :read-timeout-ms
                           (get options :read-timeout-ms 15000))
                          (positive-integer!
                           :max-attempts
                           (get options :max-attempts 3))
                          (nonnegative-integer!
                           :retry-delay-ms
                           (get options :retry-delay-ms 10))
                          (nonnegative-integer!
                           :jitter-ms
                           (get options :jitter-ms 25))
                          (atom false))]
     (try
       (status! client)
       client
       (catch Throwable error
         (close! client)
         (throw error))))))

(defn assert!
  ([client proposition] (assert! client proposition {}))
  ([client proposition options]
   (mutation-result
    (request! client :rpc/assert
              (wire/rpc-write! proposition
                               (get options :policy wire/rpc-subject-any)
                               (:fence options))
              options))))

(defn retract!
  ([client proposition] (retract! client proposition {}))
  ([client proposition options]
   (mutation-result
    (request! client :rpc/retract
              (wire/rpc-write! proposition
                               (get options :policy wire/rpc-subject-any)
                               (:fence options))
              options))))

(defn- action-term [action]
  (if (t/triple? action)
    action
    (wire/rpc-action! (:op action) (:proposition action)
                      (get action :policy wire/rpc-subject-any))))

(defn batch!
  ([client actions] (batch! client actions {}))
  ([client actions options]
   (mutation-result
    (request! client :rpc/batch
              (wire/rpc-batch! (mapv action-term actions) (:fence options))
              options))))

(defn scan!
  ([client slot0 slot1 slot2] (scan! client slot0 slot1 slot2 {}))
  ([client slot0 slot1 slot2 options]
   (let [{:keys [payload page] :as result}
         (result-map
          (request! client :rpc/scan
                    (wire/rpc-triple-pattern! slot0 slot1 slot2) options))
         [rows] (record-fields! payload :rpc/triples 1)]
     (assoc (dissoc result :payload :page)
            :rows (list-values! rows) :page (page-map page)))))

(defn query!
  ([client query-request] (query! client query-request {}))
  ([client query-request options]
   (let [{:keys [payload page] :as result}
         (result-map (request! client :rpc/query query-request options))
         [encoded-rows] (record-fields! payload :query/rows 1)]
     (assoc (dissoc result :payload :page)
            :rows
            (mapv (fn [row]
                    (let [[values] (record-fields! row :query/row 1)]
                      (list-values! values)))
                  (list-values! encoded-rows))
            :page (page-map page)))))

(defn occurrences!
  ([client] (occurrences! client {}))
  ([client options]
   (let [{:keys [payload page] :as result}
         (result-map (request! client :rpc/occurrences wire/rpc-unit options))
         [rows] (record-fields! payload :rpc/occurrences 1)]
     (assoc (dissoc result :payload :page)
            :rows (list-values! rows) :page (page-map page)))))

(defn- checked-page-size [options]
  (let [page-size (get options :page-size effective-page-limit)]
    (positive-integer! :page-size page-size)
    (when (> page-size effective-page-limit)
      (throw (ex-info "FRAMRPC page size exceeds the current TermCodec-safe limit"
                      {:type :rpc/page-size-unsafe
                       :page-size page-size
                       :effective-limit effective-page-limit})))
    page-size))

(defn- drain-pages! [page-fn options]
  (let [page-size (checked-page-size options)
        base-options (dissoc options :page-size)]
    (loop [cursor nil rows [] pages 0 snapshot nil attempts 0]
      (let [response (page-fn (assoc base-options
                                     :page (wire/rpc-page-request!
                                            page-size cursor)))
            served (:served-version response)
            page (:page response)
            snapshot (or snapshot served)]
        (when-not page
          (throw (ex-info "FRAMRPC paged operation omitted page metadata"
                          {:type :rpc/missing-page})))
        (when-not (= snapshot served)
          (throw (ex-info "FRAMRPC page drain changed snapshot"
                          {:type :rpc/page-snapshot-changed
                           :expected snapshot :actual served})))
        (let [all-rows (into rows (:rows response))
              page-count (inc pages)
              attempt-count (+ attempts (:attempts response))]
          (if (:done? page)
            {:rows all-rows :served-version snapshot
             :pages page-count :attempts attempt-count}
            (recur (:cursor page) all-rows page-count snapshot attempt-count)))))))

(defn scan-all!
  ([client slot0 slot1 slot2] (scan-all! client slot0 slot1 slot2 {}))
  ([client slot0 slot1 slot2 options]
   (drain-pages! #(scan! client slot0 slot1 slot2 %) options)))

(defn query-all!
  ([client query-request] (query-all! client query-request {}))
  ([client query-request options]
   (drain-pages! #(query! client query-request %) options)))

(defn occurrences-all!
  ([client] (occurrences-all! client {}))
  ([client options]
   (drain-pages! #(occurrences! client %) options)))

(defn lease-acquire!
  ([client resource holder ttl-ms]
   (lease-acquire! client resource holder ttl-ms {}))
  ([client resource holder ttl-ms options]
   (let [{:keys [payload] :as result}
         (result-map
          (request! client :rpc/lease-acquire
                    (wire/rpc-lease-acquire! resource holder ttl-ms) options))
         [fence expires] (record-fields! payload :lease/grant 2)]
     (assoc (dissoc result :payload :page) :fence fence :expires expires))))

(defn lease-renew!
  ([client fence ttl-ms] (lease-renew! client fence ttl-ms {}))
  ([client fence ttl-ms options]
   (let [{:keys [payload] :as result}
         (result-map
          (request! client :rpc/lease-renew
                    (wire/rpc-lease-renew! fence ttl-ms) options))
         [next-fence expires] (record-fields! payload :lease/grant 2)]
     (assoc (dissoc result :payload :page)
            :fence next-fence :expires expires))))

(defn lease-release!
  ([client fence] (lease-release! client fence {}))
  ([client fence options]
   (let [{:keys [payload] :as result}
         (result-map (request! client :rpc/lease-release fence options))
         [released] (record-fields! payload :lease/released 1)]
     (assoc (dissoc result :payload :page) :released? released))))

(defn lease-check!
  ([client fence] (lease-check! client fence {}))
  ([client fence options]
   (let [{:keys [payload] :as result}
         (result-map (request! client :rpc/lease-check fence options))
         [valid expires-option] (record-fields! payload :lease/check 2)]
     (assoc (dissoc result :payload :page)
            :valid? valid
            :expires (when (wire/rpc-option-present?! expires-option)
                       (wire/rpc-option-value! expires-option))))))

(defn validate!
  ([client] (validate! client {}))
  ([client options]
   (let [{:keys [payload] :as result}
         (result-map (request! client :rpc/validate wire/rpc-unit options))
         [valid violations] (record-fields! payload :rpc/validation 2)]
     (assoc (dissoc result :payload :page)
            :valid? valid :violations (list-values! violations)))))

(defn- projection! [client subject predicate]
  (let [result (scan-all! client subject predicate nil)]
    {:served-version (:served-version result)
     :triples (:rows result)
     :values (mapv t/triple-slot2 (:rows result))}))

(defn profile-write!
  "Set one subject/predicate projection with North set semantics. CARDINALITY is
   :one or :many. OCC and ambiguity resolution use the same exact projection."
  ([client subject predicate desired-values]
   (profile-write! client subject predicate desired-values {}))
  ([client subject predicate desired-values options]
   (let [cardinality (get options :cardinality :many)
         desired (vec (distinct desired-values))]
     (when-not (contains? #{:one :many} cardinality)
       (throw (ex-info "profile cardinality must be :one or :many"
                       {:type :rpc/invalid-profile
                        :cardinality cardinality})))
     (when (and (= cardinality :one) (> (count desired) 1))
       (throw (ex-info "single-valued profile accepts at most one value"
                       {:type :rpc/cardinality-violation
                        :subject subject :predicate predicate})))
     (let [{:keys [served-version values]} (projection! client subject predicate)
           before-frequencies (frequencies values)
           desired-frequencies (frequencies desired)
           retract-values
           (mapcat (fn [[value count-value]]
                     (repeat (max 0 (- count-value
                                       (get desired-frequencies value 0)))
                             value))
                   before-frequencies)
           assert-values
           (mapcat (fn [[value count-value]]
                     (repeat (max 0 (- count-value
                                       (get before-frequencies value 0)))
                             value))
                   desired-frequencies)
           actions
           (vec
            (concat
             (map (fn [value]
                    {:op :rpc/retract
                     :proposition (t/triple subject predicate value)})
                  retract-values)
             (map (fn [value]
                    {:op :rpc/assert
                     :proposition (t/triple subject predicate value)})
                  assert-values)))]
       (if (empty? actions)
         {:changed? false :served-version served-version
          :values desired :results [] :attempts 0}
         (let [base-version (get options :expected-version served-version)
               resolver
               (fn [resolver-client _ _]
                 (let [{after-version :served-version after-values :values}
                       (projection! resolver-client subject predicate)
                       after-frequencies (frequencies after-values)]
                   (cond
                     (= desired-frequencies after-frequencies)
                     {:resolution :committed
                      :served-version after-version :values desired}

                     (and (= before-frequencies after-frequencies)
                          (= base-version after-version))
                     {:resolution :retry}

                     :else
                     {:resolution :indeterminate
                      :served-version after-version
                      :before values :after after-values
                      :desired desired})))
               result
               (batch! client actions
                       (assoc options
                              :expected-version base-version
                              :ambiguity-resolver resolver))]
           (if-let [resolved (:resolved result)]
             {:changed? true
              :served-version (:served-version resolved)
              :values (:values resolved)
              :resolved-ambiguity? true
              :results [] :attempts (:attempts result)}
             (assoc result :changed? true :values desired))))))))

(defn subscribe! [& _]
  (throw (ex-info "FRAMRPC subscription is reserved for the daemon-side v2 operation"
                  {:type :rpc/subscription-unavailable
                   :operation subscription-operation
                   :stage :post-stage-1})))
