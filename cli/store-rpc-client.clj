#!/usr/bin/env bb
(ns north.store-rpc-client
  (:require [clojure.string :as str]
            [store.rpc :as wire]
            [store.types :as t])
  (:import [java.io IOException]
           [java.net InetSocketAddress Socket SocketTimeoutException]
           [java.nio ByteBuffer ByteOrder]
           [java.util.concurrent ThreadLocalRandom]
           [java.util.concurrent.atomic AtomicLong]))

(def max-body-bytes wire/rpc-v2-max-body-bytes)
(def effective-page-limit 200)
;; Must stay equal to the Beagle Store server's own retryable set;
;; a code the server marks retryable but the client omits fails a caller that
;; the server expected to ask again.
(def retryable-error-codes
  #{:rpc/conflict :rpc/cancelled :query-cancelled :query-time-limit
    :query-work-limit :query/archive-unavailable :durability-ambiguous})
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
                 (bit-and 255 (int (aget wire/rpc-v2-magic index))))
      (throw (ex-info "Store RPC response magic does not match"
                      {:type :rpc-invalid-magic}))))
  (let [buffer (doto (ByteBuffer/wrap header) (.order ByteOrder/LITTLE_ENDIAN))]
    (.position buffer 8)
    (let [major (Short/toUnsignedInt (.getShort buffer))
          minor (Short/toUnsignedInt (.getShort buffer))
          kind (bit-and 255 (int (.get buffer)))
          flags (bit-and 255 (int (.get buffer)))
          body-length (Integer/toUnsignedLong (.getInt buffer))]
      (when-not (and (= major wire/rpc-v2-major)
                     (= minor wire/rpc-v2-minor))
        (throw (ex-info "Store RPC response version is unsupported"
                        {:type :rpc-unsupported-version
                         :major major :minor minor})))
      (when-not (contains? #{2 4} kind)
        (throw (ex-info "Store RPC client expected a response or event packet"
                        {:type :rpc-invalid-kind :kind kind})))
      (when-not (zero? flags)
        (throw (ex-info "Store RPC response flags must be zero"
                        {:type :rpc-invalid-flags :flags flags})))
      (when (> body-length max-body-bytes)
        (throw (ex-info "Store RPC response body exceeds the 1 MiB limit"
                        {:type :rpc-packet-too-large
                         :body-length body-length
                         :limit max-body-bytes})))
      (int body-length))))

(defn read-packet!
  "Read one bounded Store RPC response/event packet without allocating an
   untrusted declared body."
  [input]
  (let [header (byte-array wire/rpc-v2-header-bytes)]
    (when-not (read-exact! input header 0 wire/rpc-v2-header-bytes)
      (throw (ex-info "Store RPC response ended inside its header"
                      {:type :rpc-truncated})))
    (let [body-length (header-body-length! header)
          body (byte-array body-length)
          packet (byte-array (+ wire/rpc-v2-header-bytes body-length))]
      (when-not (read-exact! input body 0 body-length)
        (throw (ex-info "Store RPC response ended inside its body"
                        {:type :rpc-truncated})))
      (System/arraycopy header 0 packet 0 wire/rpc-v2-header-bytes)
      (System/arraycopy body 0 packet wire/rpc-v2-header-bytes body-length)
      (wire/decode-rpc-frame-v2! packet))))

(defn encode-request-packet!
  "Encode one request through TermCodecV1 and enforce the shared body limit."
  [request-id request]
  (wire/encode-rpc-frame-v2!
   (wire/rpc-request-frame request-id request)))

(defn transport-round-trip!
  "Perform one unary socket exchange. The daemon owns one request per socket."
  [client request]
  (let [request-id (next-request-id)
        sent? (atom false)]
    (try
      (let [bytes (encode-request-packet! request-id request)]
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
          (let [packet (read-packet! (.getInputStream socket))
                response (t/rpcframev2-response packet)]
            (when-not (= :response (t/rpcframev2-kind packet))
              (throw (ex-info "Store RPC unary request received a non-response packet"
                              {:type :rpc-invalid-kind})))
            (when-not (= request-id (t/rpcframev2-request-id packet))
              (throw (ex-info "Store RPC response request-id does not match"
                              {:type :rpc-request-id-mismatch})))
            (when-not (and (= (t/rpcrequest-space request)
                              (t/rpcresponse-space response))
                           (= (t/rpcrequest-op request)
                              (t/rpcresponse-op response)))
              (throw (ex-info "Store RPC response identity does not match its request"
                              {:type :rpc-response-mismatch
                               :expected-space (t/rpcrequest-space request)
                               :actual-space (t/rpcresponse-space response)
                               :expected-op (t/rpcrequest-op request)
                               :actual-op (t/rpcresponse-op response)})))
            response)))
      (catch Throwable error
        (if (contains? (ex-data error) :request-sent?)
          (throw error)
          (throw (ex-info (or (.getMessage error) "Store RPC transport failed")
                          (assoc (or (ex-data error) {})
                                 :request-sent? @sent?)
                          error)))))))

(def ^:dynamic *round-trip!* transport-round-trip!)

(defn- open! [client]
  (when @(:closed client)
    (throw (ex-info "Store RPC client is closed" {:type :rpc/client-closed})))
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
                   :rpc-invalid-kind :rpc-invalid-flags :rpc-packet-too-large
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
    (throw (ex-info "Store RPC mutation outcome is ambiguous and no exact projection resolver was supplied"
                    {:type :rpc/ambiguous-write
                     :operation (t/rpcrequest-op request)
                     :attempts attempt}
                    error))
    (let [{:keys [resolution] :as decision}
          (resolver client request error)]
      (case resolution
        :committed {:resolved decision :attempts attempt}
        :retry :retry
        (throw (ex-info "Store RPC mutation could not be resolved from its exact projection"
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
                    (throw (ex-info "Store RPC same-question retry budget exhausted"
                                    {:type :rpc/retry-exhausted
                                     :operation (t/rpcrequest-op request)
                                     :attempts attempt}
                                    error)))
                  decision))
              (if (< attempt (:max-attempts client))
                (do (retry-pause! client attempt) (recur (inc attempt)))
                (if (= false (:request-sent? (ex-data error)))
                  (throw error)
                  (throw (ex-info "Store RPC same-question retry budget exhausted"
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
                       (let [[input-index changed occurrence]
                             (record-fields! encoded :rpc/action-result 3)]
                         (when-not (t/occurrence-coordinate? occurrence)
                           (throw
                            (ex-info
                             "Store RPC action result requires one occurrence coordinate"
                             {:type :rpc-invalid-occurrence
                              :occurrence occurrence})))
                         {:input-index input-index
                          :changed? changed
                          :occurrence occurrence}))
                     (list-values! encoded-results)))))))

(defn version! [client]
  (let [result (result-map (request! client :rpc/version wire/rpc-unit {}))]
    (dissoc result :payload :page)))

(defn status! [client]
  (let [{:keys [payload] :as result}
        (result-map (request! client :rpc/status wire/rpc-unit {}))
        [state live-count engine encoded-cache]
        (record-fields! payload :rpc/status 4)
        [hits misses bytes evictions]
        (record-fields! encoded-cache :rpc/result-cache 4)]
    (assoc (dissoc result :payload :page)
           :space-id (:space-id client)
           :state state :live-count live-count :engine engine
           :cache {:hits hits :misses misses :bytes bytes
                   :evictions evictions})))

(declare connect)

(defn client
  "Construct a canonical Store RPC client without an implicit probe. The client
   owns configuration, not a persistent socket; each named operation performs
   exactly its own bounded request. Use connect when an eager status probe is
   itself part of the caller's contract."
  ([host port space-id] (client host port space-id {}))
  ([host port space-id options]
   (when (str/blank? host)
     (throw (ex-info "Store RPC host must be nonblank"
                     {:type :rpc/invalid-client-option :option :host})))
   (when (str/blank? space-id)
     (throw (ex-info "Store RPC SpaceId must be nonblank"
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
     client)))

(defn connect
  ([host port space-id] (connect host port space-id {}))
  ([host port space-id options]
   (let [client (client host port space-id options)]
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
  ([client t1 t2 t3] (scan! client t1 t2 t3 {}))
  ([client t1 t2 t3 options]
   (let [{:keys [payload page] :as result}
         (result-map
          (request! client :rpc/scan
                    (wire/rpc-triple-pattern! t1 t2 t3) options))
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
      (throw (ex-info "Store RPC page size exceeds the current TermCodec-safe limit"
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
          (throw (ex-info "Store RPC paged operation omitted page metadata"
                          {:type :rpc/missing-page})))
        (when-not (= snapshot served)
          (throw (ex-info "Store RPC page drain changed snapshot"
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
  ([client t1 t2 t3] (scan-all! client t1 t2 t3 {}))
  ([client t1 t2 t3 options]
   (drain-pages! #(scan! client t1 t2 t3 %) options)))

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
     :values (mapv t/triple-t3 (:rows result))}))

;; --- multi-predicate subject projection --------------------------------------

(defn occurrence-map
  "Canonical occurrence map of TRIPLES: predicate -> value -> occurrence count.
   The count is data on an occurrence-shaped wire, never noise: two equal
   assertions are two occurrences, and an exact publication must reproduce the
   frequency, not merely the value set."
  [triples]
  (reduce (fn [acc triple]
            (update-in acc [(t/triple-t2 triple) (t/triple-t3 triple)]
                       (fnil inc 0)))
          {}
          triples))

(defn subject-projection!
  "ONE paged scan of the whole SUBJECT, producing its canonical occurrence map
   at ONE served version. A multi-predicate write plans from this, never from a
   scan per predicate: independent reads share no version to fence a batch with."
  ([client subject] (subject-projection! client subject {}))
  ([client subject options]
   (let [{:keys [rows served-version pages attempts]}
         (scan-all! client subject nil nil options)]
     {:subject subject
      :served-version served-version
      :triples rows
      :occurrences (occurrence-map rows)
      :pages pages
      :attempts attempts})))

(defn- multiset-excess
  "Values FROM holds beyond OTHER, one entry per surplus occurrence."
  [from other]
  (mapcat (fn [[value count-value]]
            (repeat (max 0 (- count-value (get other value 0))) value))
          from))

(def ^:private action-phase {:rpc/retract 0 :rpc/assert 1})

(defn plan-subject-actions
  "Action vector driving SUBJECT's occurrence map BEFORE to DESIRED. Both are
   predicate -> value -> count; a predicate DESIRED does not name is left
   untouched, and one mapped to {} is emptied. Retracts precede asserts because
   a retract withdraws the LATEST equal occurrence and would otherwise eat the
   assert beside it. Inside a phase the order is (:rank action) then a canonical
   key, so a caller needing one assert last supplies a rank and stays
   deterministic; the plan never depends on map iteration order."
  ([subject before desired] (plan-subject-actions subject before desired {}))
  ([subject before desired {:keys [rank]}]
   (let [rank-fn (or rank (constantly 0))
         actions
         (for [predicate (keys desired)
               :let [current (get before predicate {})
                     target (get desired predicate {})]
               [operation values] [[:rpc/retract (multiset-excess current target)]
                                   [:rpc/assert (multiset-excess target current)]]
               value values]
           {:op operation :proposition (t/triple subject predicate value)})]
     (vec (sort-by (fn [action]
                     (let [proposition (:proposition action)]
                       [(action-phase (:op action))
                        (rank-fn action)
                        (pr-str [(t/triple-t2 proposition)
                                 (t/triple-t3 proposition)])]))
                   actions)))))

;; --- North projection semantics over the append-only head wire ---------------
;; The head wire is occurrence-shaped (assert appends unconditionally, retract
;; withdraws only the latest equal occurrence); North's contract is set-shaped
;; over the exact projection. Every North write reconciles: read the projection,
;; multiset-diff it, land the difference as ONE batch — an already-satisfied
;; projection issues no write, which is what makes a repeated assert idempotent.

(defn- reconcile-once!
  [client subject predicate desired-fn options]
  (let [{:keys [served-version values]} (projection! client subject predicate)
        desired (vec (desired-fn values))
        before-frequencies (frequencies values)
        desired-frequencies (frequencies desired)
        actions (plan-subject-actions subject
                                      {predicate before-frequencies}
                                      {predicate desired-frequencies})]
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
          (assoc result :changed? true :values desired))))))

(defn- reconcile-projection!
  "Drive one subject/predicate projection to (DESIRED-FN current-values) as one
   atomic batch. The read and the write are fenced by the read's own served
   version, so a losing race is a typed :rpc/conflict and never a silent clobber;
   the re-ask re-reads the projection. A caller that PINS :expected-version owns
   its own base, so its conflict is an answer and is never retried here."
  [client subject predicate desired-fn options]
  (let [limit (if (contains? options :expected-version) 1 (:max-attempts client))]
    (loop [attempt 1]
      (let [outcome
            (try
              {:value (reconcile-once! client subject predicate desired-fn options)}
              (catch clojure.lang.ExceptionInfo error
                (if (and (= :rpc/conflict (:type (ex-data error)))
                         (< attempt limit))
                  {:conflict error}
                  (throw error))))]
        (if (:conflict outcome)
          (do (retry-pause! client attempt) (recur (inc attempt)))
          (:value outcome))))))

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
     (reconcile-projection! client subject predicate (constantly desired)
                            options))))

;; --- predicate cardinality: which assertion supersedes -----------------------
;; Authority order is graph (`@<name> cardinality`, written by pred-cli) then
;; bin/north's BEAGLE_STORE_SINGLE_VALUED mirror, then Beagle Store's own default of multi.

(def ^:dynamic *env* #(System/getenv %))

(def ^:private cardinality-cache (atom {}))

(defn reset-cardinality-cache!
  "Cardinality is cached for the life of the process. Tests and any caller that
   changes a declaration in-process must drop the cache explicitly."
  []
  (reset! cardinality-cache {})
  nil)

(defn- predicate-name [predicate]
  (if (keyword? predicate) (name predicate) (str predicate)))

(defn- env-single-valued []
  (into #{}
        (remove str/blank?)
        (str/split (or (*env* "BEAGLE_STORE_SINGLE_VALUED") "") #"\s+")))

(defn- declared-cardinality [client predicate]
  (let [rows (:rows (scan-all! client (str "@" (predicate-name predicate))
                               "cardinality" nil))
        values (into #{} (map (comp str t/triple-t3)) rows)]
    (cond (contains? values "single") :one
          (contains? values "multi") :many
          :else nil)))

(defn cardinality-of
  "North cardinality of PREDICATE — :one (declared single: an assertion
   supersedes the live value) or :many (values coexist)."
  [client predicate]
  (let [cache-key [(:host client) (:port client) (:space-id client)
                   (predicate-name predicate)]]
    (or (get @cardinality-cache cache-key)
        (let [resolved (or (declared-cardinality client predicate)
                           (when (contains? (env-single-valued)
                                            (predicate-name predicate))
                             :one)
                           :many)]
          (swap! cardinality-cache assoc cache-key resolved)
          resolved))))

(defn assert-projected!
  "North assert semantics. A repeated identical assertion is idempotent, and a
   declared-single predicate's assertion supersedes its live value as one batch.
   This is the write verb a North caller uses; assert! is the raw
   occurrence-appending primitive underneath it."
  ([client proposition] (assert-projected! client proposition {}))
  ([client proposition options]
   (let [subject (t/triple-t1 proposition)
         predicate (t/triple-t2 proposition)
         value (t/triple-t3 proposition)
         cardinality (or (:cardinality options)
                         (cardinality-of client predicate))
         desired-fn (if (= :one cardinality)
                      (constantly [value])
                      ;; Multi keeps every rival value AND any pre-existing
                      ;; duplicate: an assert deduplicates itself, never the
                      ;; projection around it.
                      (fn [current]
                        (if (some #(= value %) current)
                          (vec current)
                          (conj (vec current) value))))]
     (assoc (reconcile-projection! client subject predicate desired-fn
                                   (dissoc options :cardinality))
            :cardinality cardinality))))

(defn retract-projected!
  "North retract semantics: the value LEAVES the exact projection, every equal
   occurrence with it. retract! withdraws only the latest equal occurrence, so a
   value appended twice would otherwise survive its own retraction."
  ([client proposition] (retract-projected! client proposition {}))
  ([client proposition options]
   (let [subject (t/triple-t1 proposition)
         predicate (t/triple-t2 proposition)
         value (t/triple-t3 proposition)]
     (reconcile-projection! client subject predicate
                            (fn [current] (vec (remove #(= value %) current)))
                            (dissoc options :cardinality)))))

;; --- fenced batch submission: classify the outcome, never guess it -----------
;; A batch applies whole or not at all: no outcome below is a partial prefix,
;; and none is retried here — the replan policy belongs to the caller.

(def batch-outcomes
  "Every outcome fenced-batch! can report, with the three facts a caller decides
   on. :applied? nil is genuinely unknown and only an exact readback resolves it.
   :replan-safe? false means a fresh scan CANNOT be read as truth yet."
  {:no-op                {:applied? false :zero-applied? true
                          :retry-identical-safe? true :replan-safe? true}
   :applied              {:applied? true :zero-applied? false
                          :retry-identical-safe? false :replan-safe? true}
   :conflict             {:applied? false :zero-applied? true
                          :retry-identical-safe? false :replan-safe? true}
   :fence-mismatch       {:applied? false :zero-applied? true
                          :retry-identical-safe? false :replan-safe? true}
   :lease-held           {:applied? false :zero-applied? true
                          :retry-identical-safe? false :replan-safe? true}
   :rejected             {:applied? false :zero-applied? true
                          :retry-identical-safe? false :replan-safe? true}
   :not-sent             {:applied? false :zero-applied? true
                          :retry-identical-safe? true :replan-safe? true}
   :sent-ambiguous       {:applied? nil :zero-applied? false
                          :retry-identical-safe? true :replan-safe? true}
   :durability-ambiguous {:applied? nil :zero-applied? false
                          :retry-identical-safe? false :replan-safe? false
                          :restart-required? true}})

(defn- outcome-result [outcome extra]
  (merge {:outcome outcome} (get batch-outcomes outcome) extra))

(def ^:private mutation-failure-outcomes
  {:rpc/conflict :conflict
   :rpc/lease-fence-mismatch :fence-mismatch
   :rpc/lease-held :lease-held
   :durability-ambiguous :durability-ambiguous})

(defn- classify-mutation-failure
  "Classify ERROR as a mutation outcome, or rethrow it. A daemon answer carries
   :code, a transport failure carries :request-sent?, and anything with neither
   is a local/protocol fault that must not be dressed up as a write outcome."
  [error]
  (let [data (or (ex-data error) {})]
    (cond
      ;; invoke! reaches :rpc/ambiguous-write only for a mutation whose request
      ;; was sent, so the ack — not the write — is what was lost.
      (= :rpc/ambiguous-write (:type data))
      (let [cause (ex-cause error)
            code (:code (or (ex-data cause) {}))]
        (outcome-result (if (= :durability-ambiguous code)
                          :durability-ambiguous
                          :sent-ambiguous)
                        {:code (or code :rpc/ambiguous-write) :error error}))

      (contains? data :code)
      (outcome-result (get mutation-failure-outcomes (:code data) :rejected)
                      {:code (:code data)
                       :error error
                       :served-version (:served-version data)})

      (contains? data :request-sent?)
      (outcome-result (if (false? (:request-sent? data)) :not-sent :sent-ambiguous)
                      {:code (:type data) :error error})

      :else (throw error))))

(defn- require-action-results! [actions result]
  (let [results (:results result)]
    (when-not (and (= (count results) (count actions))
                   (= (map :input-index results) (range (count actions))))
      (throw (ex-info "Store RPC batch acknowledged an action-result shape that does not match its actions"
                      {:type :rpc/unexpected-action-results
                       :actions (count actions)
                       :results (mapv :input-index results)}))))
  result)

(defn fenced-batch!
  "Submit ACTIONS as ONE fenced, expected-version transaction and classify its
   outcome instead of retrying it. Pass :fence and :expected-version from the
   same read that produced the plan. Returns an entry of batch-outcomes plus
   :results/:changed?/:served-version on an acknowledged commit; an acknowledged
   commit whose action results do not match its actions throws, because that is
   a protocol disagreement and not a publication outcome."
  ([client actions] (fenced-batch! client actions {}))
  ([client actions options]
   (open! client)
   (if (empty? actions)
     (outcome-result :no-op {:results [] :changed? false :attempts 0})
     ;; One attempt: every retry here is a caller decision (replan, identical
     ;; retry, or stop), and the generic client owns none of those policies.
     (let [single (assoc client :max-attempts 1)
           request-options (dissoc options :ambiguity-resolver)
           outcome (try
                     {:ok (batch! single actions request-options)}
                     (catch Throwable error {:error error}))]
       (if-let [result (:ok outcome)]
         (let [{:keys [results] :as acked} (require-action-results! actions result)]
           (outcome-result :applied
                           {:results results
                            :changed? (boolean (some :changed? results))
                            :served-version (:served-version acked)
                            :attempts (:attempts acked)}))
         (classify-mutation-failure (:error outcome)))))))

;; --- exact subject readback: what the graph actually holds now ----------------

(defn- occurrence-excess
  "Occurrences FROM holds beyond OTHER, as an occurrence map."
  [from other]
  (into {}
        (keep (fn [[predicate values]]
                (let [surplus
                      (into {}
                            (keep (fn [[value count-value]]
                                    (let [delta (- count-value
                                                   (get-in other [predicate value] 0))]
                                      (when (pos? delta) [value delta]))))
                            values)]
                  (when (seq surplus) [predicate surplus]))))
        from))

(defn subject-readback!
  "Re-scan SUBJECT and decide what a fenced write actually did. DESIRED is an
   occurrence map and the comparison is exact in BOTH directions — value set AND
   occurrence frequency. Options: :before, the pre-write occurrence map, which is
   what lets an unchanged projection be reported as proven-absent rather than
   merely not-committed; :guard-predicates, predicates required to be absent;
   :scope :subject to compare the WHOLE subject instead of only the predicates
   DESIRED and the guards name.

   States: :committed (exact), :absent (identical to :before, so the batch never
   landed and a replan is safe), :foreign-writer (equal to neither, which this
   atomic batch cannot have produced — another writer or corruption intervened),
   :indeterminate (a mismatch with no baseline to attribute it to)."
  ([client subject desired] (subject-readback! client subject desired {}))
  ([client subject desired options]
   (let [{:keys [before scope guard-predicates]} options
         projection (subject-projection!
                     client subject
                     (dissoc options :before :scope :guard-predicates))
         actual (:occurrences projection)
         compared (if (= :subject scope)
                    (into (set (keys actual)) (keys desired))
                    (into (set (keys desired)) guard-predicates))
         restrict (fn [occurrences]
                    (into {}
                          (keep (fn [predicate]
                                  (let [values (get occurrences predicate)]
                                    (when (seq values) [predicate values]))))
                          compared))
         actual-view (restrict actual)
         desired-view (restrict desired)
         state (cond
                 (= actual-view desired-view) :committed
                 (and before (= actual-view (restrict before))) :absent
                 before :foreign-writer
                 :else :indeterminate)]
     {:state state
      :committed? (= state :committed)
      :indeterminate? (contains? #{:foreign-writer :indeterminate} state)
      :served-version (:served-version projection)
      :occurrences actual
      :missing (occurrence-excess desired-view actual-view)
      :unexpected (occurrence-excess actual-view desired-view)})))

;; --- lease acquisition at an expected version --------------------------------

(defn fence-parts
  "[resource holder epoch] of a typed lease fence."
  [fence]
  (record-fields! fence :rpc/fence 3))

(defn lease-acquire-at-version!
  "Acquire RESOURCE for HOLDER as the transaction immediately after
   EXPECTED-VERSION. A commit's epoch is EXPECTED-VERSION+1, which is what makes
   the fence RECONSTRUCTABLE when the acknowledgement is lost: :candidate-fence
   names the lease this call would have taken, and lease-check! decides. OCC
   admits at most one committing acquire, so an identical retry at the same
   expected version is safe. On :conflict and :lease-held nothing committed."
  ([client resource holder ttl-ms expected-version]
   (lease-acquire-at-version! client resource holder ttl-ms expected-version {}))
  ([client resource holder ttl-ms expected-version options]
   (open! client)
   (let [candidate (wire/rpc-fence! resource holder (inc expected-version))
         outcome (try
                   {:ok (lease-acquire! (assoc client :max-attempts 1)
                                        resource holder ttl-ms
                                        (assoc options
                                               :expected-version expected-version))}
                   (catch Throwable error {:error error}))]
     (if-let [grant (:ok outcome)]
       (let [[_ _ epoch] (fence-parts (:fence grant))]
         (outcome-result :applied
                         {:acquired? true
                          :fence (:fence grant)
                          :epoch epoch
                          :expected-epoch (inc expected-version)
                          :expires (:expires grant)
                          :served-version (:served-version grant)
                          :attempts (:attempts grant)}))
       (let [classified (classify-mutation-failure (:error outcome))]
         (assoc classified
                :acquired? (:applied? classified)
                :candidate-fence candidate))))))

(defn subscribe! [& _]
  (throw (ex-info "Store RPC subscription is reserved for the daemon-side v2 operation"
                  {:type :rpc/subscription-unavailable
                   :operation subscription-operation
                   :stage :post-stage-1})))
