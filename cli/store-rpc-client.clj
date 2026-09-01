(ns north.store-rpc-client
  (:require [clojure.string :as str]
            [store.rpc :as wire]
            [store.types :as t])
  (:import [java.io IOException]
           [java.lang ProcessHandle]
           [java.net InetSocketAddress]
           [java.net Socket]
           [java.net SocketTimeoutException]
           [java.nio ByteBuffer]
           [java.nio ByteOrder]
           [java.util.concurrent ThreadLocalRandom]
           [java.util.concurrent.atomic AtomicLong]))

(def max-body-bytes wire/rpc-v2-max-body-bytes)

(def effective-page-limit 200)

(def retryable-error-codes #{:rpc/conflict :rpc/cancelled :query-cancelled :query-time-limit :query-work-limit :query/archive-unavailable :durability-ambiguous})

(def subscription-operation :rpc/subscribe)

(def ^:private mutation-operations #{:rpc/assert :rpc/retract :rpc/batch :rpc/lease-acquire :rpc/lease-renew :rpc/lease-release})

(def ^:private ambiguous-error-codes #{:durability-ambiguous})

(def ^:private request-sequence (AtomicLong. 0))

(def ^:private request-sequence-limit 4294967295)

(def ^:private request-id-stride 4294967296)

(def ^:private request-process-id (long (.pid (ProcessHandle/current))))

(defrecord Client [host port space-id connect-timeout-ms read-timeout-ms max-attempts retry-delay-ms jitter-ms closed])

(defn client-host [r] (:host r))

(defn client-port [r] (:port r))

(defn client-space-id [r] (:space-id r))

(defn client-connect-timeout-ms [r] (:connect-timeout-ms r))

(defn client-read-timeout-ms [r] (:read-timeout-ms r))

(defn client-max-attempts [r] (:max-attempts r))

(defn client-retry-delay-ms [r] (:retry-delay-ms r))

(defn client-jitter-ms [r] (:jitter-ms r))

(defn client-closed [r] (:closed r))

(defn- positive-integer [label value]
  (if (not (and (integer? value) (pos? value))) (do
  (throw (ex-info (str label " must be a positive integer") {:type :rpc/invalid-client-option :option label :value value}))))
  value)

(defn- nonnegative-integer [label value]
  (if (not (and (integer? value) (not (neg? value)))) (do
  (throw (ex-info (str label " must be a non-negative integer") {:type :rpc/invalid-client-option :option label :value value}))))
  value)

(defn- request-id-for-process [process-id sequence]
  (if (not (and (pos? process-id) (<= process-id 2147483647))) (do
  (throw (ex-info "Store RPC process id exceeds the request-id namespace" {:type :rpc/request-id-process-invalid :process-id process-id}))))
  (if (not (and (pos? sequence) (<= sequence request-sequence-limit))) (do
  (throw (ex-info "Store RPC process request-id sequence is exhausted" {:type :rpc/request-id-sequence-exhausted :sequence sequence}))))
  (+ (* process-id request-id-stride) sequence))

(defn- next-request-id []
  (let [value (.incrementAndGet request-sequence)]
  (request-id-for-process request-process-id value)))

(defn- ^Boolean read-exact! [input bytes offset length]
  (loop [position offset
   remaining length]
  (if (zero? remaining) true (let [read-count (int (.read input bytes position remaining))]
  (if (neg? read-count) false (recur (+ position read-count) (- remaining read-count)))))))

(defn- header-body-length! [header]
  (doseq [index (range 8)]
  (if (not (= (bit-and 255 (int (aget header index))) (bit-and 255 (int (aget wire/store-rpc-v2-magic index))))) (do
  (throw (ex-info "Store RPC response magic does not match" {:type :rpc-invalid-magic})))))
  (let [buffer (doto (ByteBuffer/wrap header)
  (.order ByteOrder/LITTLE_ENDIAN))]
  (.position buffer 8)
  (let [major (Short/toUnsignedInt (.getShort buffer))
   minor (Short/toUnsignedInt (.getShort buffer))
   kind (bit-and 255 (int (.get buffer)))
   flags (bit-and 255 (int (.get buffer)))
   body-length (Integer/toUnsignedLong (.getInt buffer))]
  (if (not (and (= major wire/rpc-v2-major) (= minor wire/rpc-v2-minor))) (do
  (throw (ex-info "Store RPC response version is unsupported" {:type :rpc-unsupported-version :major major :minor minor}))))
  (if (not (contains? #{2 4} kind)) (do
  (throw (ex-info "Store RPC client expected a response or event packet" {:type :rpc-invalid-kind :kind kind}))))
  (if (not (zero? flags)) (do
  (throw (ex-info "Store RPC response flags must be zero" {:type :rpc-invalid-flags :flags flags}))))
  (if (> body-length max-body-bytes) (do
  (throw (ex-info "Store RPC response body exceeds the 1 MiB limit" {:type :rpc-packet-too-large :body-length body-length :limit max-body-bytes}))))
  (int body-length))))

(defn read-packet!
  "Read one bounded Store RPC response/event packet without allocating an\n   untrusted declared body." [input]
  (let [header (byte-array wire/rpc-v2-header-bytes)]
  (if (not (read-exact! input header 0 wire/rpc-v2-header-bytes)) (do
  (throw (ex-info "Store RPC response ended inside its header" {:type :rpc-truncated}))))
  (let [body-length (header-body-length! header)
   body (byte-array body-length)
   packet (byte-array (+ wire/rpc-v2-header-bytes body-length))]
  (if (not (read-exact! input body 0 body-length)) (do
  (throw (ex-info "Store RPC response ended inside its body" {:type :rpc-truncated}))))
  (System/arraycopy header 0 packet 0 wire/rpc-v2-header-bytes)
  (System/arraycopy body 0 packet wire/rpc-v2-header-bytes body-length)
  (wire/store-rpc-decode-packet-v2! packet))))

(defn transport-round-trip!
  "Perform one unary socket exchange. The daemon owns one request per socket." [^Client client request]
  (let [request-id (next-request-id)
   sent? (atom false)]
  (try
  (let [bytes (wire/store-rpc-encode-packet-v2! (wire/store-rpc-request-packet request-id request))]
  (with-open [socket (Socket.)]
  (.connect socket (InetSocketAddress. ^String (:host client) (int (:port client))) (int (:connect-timeout-ms client)))
  (.setSoTimeout socket (int (max (:read-timeout-ms client) (+ 1000 (int (or (t/rpcrequest-timeout-ms request) 0))))))
  (let [output (.getOutputStream socket)]
  (reset! sent? true)
  (.write output bytes)
  (.flush output))
  (let [packet (read-packet! (.getInputStream socket))
   response (:response packet)]
  (if (not (= :response (:kind packet))) (do
  (throw (ex-info "Store RPC unary request received a non-response packet" {:type :rpc-invalid-kind}))))
  (if (not (= request-id (:request-id packet))) (do
  (throw (ex-info "Store RPC response request-id does not match" {:type :rpc-request-id-mismatch}))))
  (if (not (and (= (t/rpcrequest-space request) (t/rpcresponse-space response)) (= (t/rpcrequest-op request) (t/rpcresponse-op response)))) (do
  (throw (ex-info "Store RPC response identity does not match its request" {:type :rpc-response-mismatch :expected-space (t/rpcrequest-space request) :actual-space (t/rpcresponse-space response) :expected-op (t/rpcrequest-op request) :actual-op (t/rpcresponse-op response)}))))
  response)))
  (catch Throwable error
    (if (contains? (ex-data error) :request-sent?) (throw error) (throw (ex-info (or (.getMessage error) "Store RPC transport failed") (assoc (or (ex-data error) {}) :request-sent? (deref sent?)) error)))))))

(def ^:dynamic *round-trip!* transport-round-trip!)

(defn- ^Client open! [^Client client]
  (if (deref (:closed client)) (do
  (throw (ex-info "Store RPC client is closed" {:type :rpc/client-closed}))))
  client)

(defn close! [^Client client]
  (reset! (:closed client) true)
  nil)

(defn ^Boolean closed? [^Client client]
  (deref (:closed client)))

(defn- typed-response-error [response error attempts]
  (let [code (t/rpcerror-code error)]
  (ex-info (t/rpcerror-message error) {:type code :code code :retryable (t/rpcerror-retryable error) :attempts attempts :served-version (t/rpcresponse-served-version response) :detail (t/rpc-error-detail-value error)})))

(defn- ^Boolean transport-error? [error]
  (or (instance? IOException error) (instance? SocketTimeoutException error) (contains? #{:rpc-truncated :rpc-invalid-magic :rpc-unsupported-version :rpc-invalid-kind :rpc-invalid-flags :rpc-packet-too-large :rpc-request-id-mismatch :rpc-response-mismatch} (:type (ex-data error)))))

(defn- retry-pause! [^Client client attempt]
  (let [base (* attempt (:retry-delay-ms client))
   jitter (if (pos? (:jitter-ms client)) (long (.nextLong (ThreadLocalRandom/current) (long (inc (:jitter-ms client))))) 0)]
  (Thread/sleep (+ base jitter))))

(defn- resolve-ambiguous! [resolver ^Client client request error attempt]
  (if resolver (let [{:keys [resolution] :as decision} (resolver client request error)]
  (case resolution
    :committed {:resolved decision :attempts attempt}
    :retry :retry
    (throw (ex-info "Store RPC mutation could not be resolved from its exact projection" {:type :rpc/ambiguous-write :operation (t/rpcrequest-op request) :attempts attempt :resolution resolution} error)))) (throw (ex-info "Store RPC mutation outcome is ambiguous and no exact projection resolver was supplied" {:type :rpc/ambiguous-write :operation (t/rpcrequest-op request) :attempts attempt} error))))

(defn- invoke! [^Client client request {:keys [ambiguity-resolver]}]
  (open! client)
  (let [mutation? (contains? mutation-operations (t/rpcrequest-op request))]
  (loop [attempt 1]
  (let [result (try
  {:response (*round-trip!* client request)}
  (catch Throwable error
    {:transport-error error}))]
  (let [error (:transport-error result)]
  (if error (if (transport-error? error) (if (and mutation? (not= false (:request-sent? (ex-data error)))) (let [decision (resolve-ambiguous! ambiguity-resolver client request error attempt)]
  (if (= :retry decision) (if (< attempt (:max-attempts client)) (do
  (retry-pause! client attempt)
  (recur (inc attempt))) (throw (ex-info "Store RPC same-question retry budget exhausted" {:type :rpc/retry-exhausted :operation (t/rpcrequest-op request) :attempts attempt} error))) decision)) (if (< attempt (:max-attempts client)) (do
  (retry-pause! client attempt)
  (recur (inc attempt))) (if (= false (:request-sent? (ex-data error))) (throw error) (throw (ex-info "Store RPC same-question retry budget exhausted" {:type :rpc/retry-exhausted :operation (t/rpcrequest-op request) :attempts attempt} error))))) (throw error)) (let [response (:response result)
   rpc-error (t/rpcresponse-error response)]
  (if rpc-error (let [code (t/rpcerror-code rpc-error)
   retryable? (and (t/rpcerror-retryable rpc-error) (contains? retryable-error-codes code))]
  (cond
  (and mutation? (contains? ambiguous-error-codes code)) (let [decision (resolve-ambiguous! ambiguity-resolver client request (typed-response-error response rpc-error attempt) attempt)]
  (if (= :retry decision) (if (< attempt (:max-attempts client)) (do
  (retry-pause! client attempt)
  (recur (inc attempt))) (throw (typed-response-error response rpc-error attempt))) decision))
  (and retryable? (not= code :rpc/conflict) (< attempt (:max-attempts client))) (do
  (retry-pause! client attempt)
  (recur (inc attempt)))
  :else (throw (typed-response-error response rpc-error attempt)))) {:response response :attempts attempt}))))))))

(defn- request! [^Client client operation payload options]
  (let [request (wire/rpc-request! (:space-id client) operation (:expected-version options) (:page options) (:timeout-ms options) payload)]
  (invoke! client request options)))

(defn- result-map [{:keys [response attempts resolved]}]
  (if resolved {:resolved resolved :attempts attempts} {:served-version (t/rpcresponse-served-version response) :payload (t/rpc-response-payload-value response) :page (t/rpcresponse-page response) :attempts attempts}))

(defn- record-fields [value tag count-value]
  (wire/rpc-record-fields! value tag count-value))

(defn- list-values [value]
  (wire/rpc-list-values! value))

(defn- page-map [page]
  (if page (do
  {:ordinal (t/rpcpageresponse-ordinal page) :cursor (t/rpc-page-response-cursor-value page) :done? (t/rpcpageresponse-done page)})))

(defn- mutation-result [result]
  (let [{:keys [payload] :as base} (result-map result)]
  (if (:resolved base) base (let [[encoded-results] (record-fields payload :rpc/mutation-result 1)]
  (assoc (dissoc base :payload :page) :results (mapv (fn [encoded] (let [[input-index changed occurrence] (record-fields encoded :rpc/action-result 3)]
  (if (not (t/occurrence-coordinate? occurrence)) (do
  (throw (ex-info "Store RPC action result requires one occurrence coordinate" {:type :rpc-invalid-occurrence :occurrence occurrence}))))
  {:input-index input-index :changed? changed :occurrence occurrence})) (list-values encoded-results)))))))

(defn version! [client]
  (let [result (result-map (request! client :rpc/version wire/rpc-unit {}))]
  (dissoc result :payload :page)))

(defn status! [client]
  (let [{:keys [payload] :as result} (result-map (request! client :rpc/status wire/rpc-unit {}))
   [state live-count engine encoded-cache] (record-fields payload :rpc/status 4)
   [hits misses bytes evictions] (record-fields encoded-cache :rpc/result-cache 4)]
  (assoc (dissoc result :payload :page) :space-id (:space-id client) :state state :live-count live-count :engine engine :cache {:hits hits :misses misses :bytes bytes :evictions evictions})))

(declare connect!)

(defn client
  "Construct a canonical Store RPC client without an implicit probe. The client\n   owns configuration, not a persistent socket; each named operation performs\n   exactly its own bounded request. Use connect! when an eager status probe is\n   itself part of the caller's contract."
  ([host port space-id]
    (client host port space-id {}))
  ([host port space-id options]
    (if (str/blank? host) (do
  (throw (ex-info "Store RPC host must be nonblank" {:type :rpc/invalid-client-option :option :host}))))
    (if (str/blank? space-id) (do
  (throw (ex-info "Store RPC SpaceId must be nonblank" {:type :rpc/invalid-client-option :option :space-id}))))
    (let [client (->Client host (positive-integer :port port) space-id (positive-integer :connect-timeout-ms (:connect-timeout-ms options 2000)) (positive-integer :read-timeout-ms (:read-timeout-ms options 15000)) (positive-integer :max-attempts (:max-attempts options 3)) (nonnegative-integer :retry-delay-ms (:retry-delay-ms options 10)) (nonnegative-integer :jitter-ms (:jitter-ms options 25)) (atom false))]
  client)))

(defn connect!
  ([host port space-id]
    (connect! host port space-id {}))
  ([host port space-id options]
    (let [client (client host port space-id options)]
  (try
  (status! client)
  client
  (catch Throwable error
    (close! client)
    (throw error))))))

(defn assert!
  ([client proposition]
    (assert! client proposition {}))
  ([client proposition options]
    (mutation-result (request! client :rpc/assert (wire/rpc-write! proposition (:policy options wire/rpc-subject-any) (:fence options)) options))))

(defn retract!
  ([client proposition]
    (retract! client proposition {}))
  ([client proposition options]
    (mutation-result (request! client :rpc/retract (wire/rpc-write! proposition (:policy options wire/rpc-subject-any) (:fence options)) options))))

(defn- action-term [action]
  (if (t/triple? action) action (wire/rpc-action! (:op action) (:proposition action) (:policy action wire/rpc-subject-any))))

(defn batch!
  ([client actions]
    (batch! client actions {}))
  ([client actions options]
    (mutation-result (request! client :rpc/batch (wire/rpc-batch! (mapv action-term actions) (:fence options)) options))))

(defn scan!
  ([client t1 t2 t3]
    (scan! client t1 t2 t3 {}))
  ([client t1 t2 t3 options]
    (let [{:keys [payload page] :as result} (result-map (request! client :rpc/scan (wire/rpc-triple-pattern! t1 t2 t3) options))
   [rows] (record-fields payload :rpc/triples 1)]
  (assoc (dissoc result :payload :page) :rows (list-values rows) :page (page-map page)))))

(defn query!
  ([client query-request]
    (query! client query-request {}))
  ([client query-request options]
    (let [{:keys [payload page] :as result} (result-map (request! client :rpc/query query-request options))
   [encoded-rows] (record-fields payload :query/rows 1)]
  (assoc (dissoc result :payload :page) :rows (mapv (fn [row] (let [[values] (record-fields row :query/row 1)]
  (list-values values))) (list-values encoded-rows)) :page (page-map page)))))

(defn occurrences!
  ([client]
    (occurrences! client {}))
  ([client options]
    (let [{:keys [payload page] :as result} (result-map (request! client :rpc/occurrences wire/rpc-unit options))
   [rows] (record-fields payload :rpc/occurrences 1)]
  (assoc (dissoc result :payload :page) :rows (list-values rows) :page (page-map page)))))

(defn- checked-page-size [options]
  (let [page-size (:page-size options effective-page-limit)]
  (positive-integer :page-size page-size)
  (if (> page-size effective-page-limit) (do
  (throw (ex-info "Store RPC page size exceeds the current TermCodec-safe limit" {:type :rpc/page-size-unsafe :page-size page-size :effective-limit effective-page-limit}))))
  page-size))

(defn- drain-pages! [page-fn options]
  (let [page-size (checked-page-size options)
   base-options (dissoc options :page-size)]
  (loop [cursor nil
   rows []
   pages 0
   snapshot nil
   attempts 0]
  (let [response (page-fn (assoc base-options :page (wire/rpc-page-request! page-size cursor)))
   served (:served-version response)
   page (:page response)
   snapshot (or snapshot served)]
  (if (not page) (do
  (throw (ex-info "Store RPC paged operation omitted page metadata" {:type :rpc/missing-page}))))
  (if (not (= snapshot served)) (do
  (throw (ex-info "Store RPC page drain changed snapshot" {:type :rpc/page-snapshot-changed :expected snapshot :actual served}))))
  (let [all-rows (into rows (:rows response))
   page-count (inc pages)
   attempt-count (+ attempts (int (:attempts response)))]
  (if (:done? page) {:rows all-rows :served-version snapshot :pages page-count :attempts attempt-count} (recur (:cursor page) all-rows page-count snapshot attempt-count)))))))

(defn scan-all!
  ([client t1 t2 t3]
    (scan-all! client t1 t2 t3 {}))
  ([client t1 t2 t3 options]
    (drain-pages! (fn [%1] (scan! client t1 t2 t3 %1)) options)))

(defn query-all!
  ([client query-request]
    (query-all! client query-request {}))
  ([client query-request options]
    (drain-pages! (fn [%1] (query! client query-request %1)) options)))

(defn occurrences-all!
  ([client]
    (occurrences-all! client {}))
  ([client options]
    (drain-pages! (fn [%1] (occurrences! client %1)) options)))

(defn lease-acquire!
  ([client resource holder ttl-ms]
    (lease-acquire! client resource holder ttl-ms {}))
  ([client resource holder ttl-ms options]
    (let [{:keys [payload] :as result} (result-map (request! client :rpc/lease-acquire (wire/rpc-lease-acquire! resource holder ttl-ms) options))
   [fence expires] (record-fields payload :lease/grant 2)]
  (assoc (dissoc result :payload :page) :fence fence :expires expires))))

(defn lease-renew!
  ([client fence ttl-ms]
    (lease-renew! client fence ttl-ms {}))
  ([client fence ttl-ms options]
    (let [{:keys [payload] :as result} (result-map (request! client :rpc/lease-renew (wire/rpc-lease-renew! fence ttl-ms) options))
   [next-fence expires] (record-fields payload :lease/grant 2)]
  (assoc (dissoc result :payload :page) :fence next-fence :expires expires))))

(defn lease-release!
  ([client fence]
    (lease-release! client fence {}))
  ([client fence options]
    (let [{:keys [payload] :as result} (result-map (request! client :rpc/lease-release fence options))
   [released] (record-fields payload :lease/released 1)]
  (assoc (dissoc result :payload :page) :released? released))))

(defn lease-check!
  ([client fence]
    (lease-check! client fence {}))
  ([client fence options]
    (let [{:keys [payload] :as result} (result-map (request! client :rpc/lease-check fence options))
   [valid expires-option] (record-fields payload :lease/check 2)]
  (assoc (dissoc result :payload :page) :valid? valid :expires (if (wire/rpc-option-present?! expires-option) (do
  (wire/rpc-option-value! expires-option)))))))

(defn validate!
  ([client]
    (validate! client {}))
  ([client options]
    (let [{:keys [payload] :as result} (result-map (request! client :rpc/validate wire/rpc-unit options))
   [valid violations] (record-fields payload :rpc/validation 2)]
  (assoc (dissoc result :payload :page) :valid? valid :violations (list-values violations)))))

(defn- projection! [client subject predicate]
  (let [result (scan-all! client subject predicate nil)]
  {:served-version (:served-version result) :triples (:rows result) :values (mapv t/triple-t3 (:rows result))}))

(defn occurrence-map
  "Canonical occurrence map of TRIPLES: predicate -> value -> occurrence count.\n   The count is data on an occurrence-shaped wire, never noise: two equal\n   assertions are two occurrences, and an exact publication must reproduce the\n   frequency, not merely the value set." [triples]
  (reduce (fn [acc triple] (update-in acc [(t/triple-t2 triple) (t/triple-t3 triple)] (fnil inc 0))) {} triples))

(defn subject-projection!
  "ONE paged scan of the whole SUBJECT, producing its canonical occurrence map\n   at ONE served version. A multi-predicate write plans from this, never from a\n   scan per predicate: independent reads share no version to fence a batch with."
  ([client subject]
    (subject-projection! client subject {}))
  ([client subject options]
    (let [{:keys [rows served-version pages attempts]} (scan-all! client subject nil nil options)]
  {:subject subject :served-version served-version :triples rows :occurrences (occurrence-map rows) :pages pages :attempts attempts})))

(defn- multiset-excess
  "Values FROM holds beyond OTHER, one entry per surplus occurrence." [from other]
  (mapcat (fn [[value count-value]] (repeat (max 0 (- (long count-value) (long (get other value 0)))) value)) from))

(def ^:private action-phase {:rpc/retract 0 :rpc/assert 1})

(defn plan-subject-actions
  "Action vector driving SUBJECT's occurrence map BEFORE to DESIRED. Both are\n   predicate -> value -> count; a predicate DESIRED does not name is left\n   untouched, and one mapped to {} is emptied. Retracts precede asserts because\n   a retract withdraws the LATEST equal occurrence and would otherwise eat the\n   assert beside it. Inside a phase the order is (:rank action) then a canonical\n   key, so a caller needing one assert last supplies a rank and stays\n   deterministic; the plan never depends on map iteration order."
  ([subject before desired]
    (plan-subject-actions subject before desired {}))
  ([subject before desired {:keys [rank]}]
    (let [rank-fn (or rank (constantly 0))
   actions (for [predicate (keys desired)
   :let [current (get before predicate {})
   target (get desired predicate {})]
   [operation values] [[:rpc/retract (multiset-excess current target)] [:rpc/assert (multiset-excess target current)]]
   value values]
  {:op operation :proposition (t/triple subject predicate value)})]
  (vec (sort-by (fn [action] (let [proposition (:proposition action)]
  [(action-phase (:op action)) (rank-fn action) (pr-str [(t/triple-t2 proposition) (t/triple-t3 proposition)])])) actions)))))

(defn- reconcile-once! [^Client client subject predicate desired-fn options]
  (let [{:keys [served-version values]} (projection! client subject predicate)
   desired (vec (desired-fn values))
   before-frequencies (frequencies values)
   desired-frequencies (frequencies desired)
   actions (plan-subject-actions subject {predicate before-frequencies} {predicate desired-frequencies})]
  (if (empty? actions) {:changed? false :served-version served-version :values desired :results [] :attempts 0} (let [base-version (:expected-version options served-version)
   resolver (fn [resolver-client _resolver-request _resolver-error] (let [projection (projection! resolver-client subject predicate)
   after-version (:served-version projection)
   after-values (:values projection)
   after-frequencies (frequencies after-values)]
  (cond
  (= desired-frequencies after-frequencies) {:resolution :committed :served-version after-version :values desired}
  (and (= before-frequencies after-frequencies) (= base-version after-version)) {:resolution :retry}
  :else {:resolution :indeterminate :served-version after-version :before values :after after-values :desired desired})))
   result (batch! client actions (assoc options :expected-version base-version :ambiguity-resolver resolver))]
  (let [resolved (:resolved result)]
  (if resolved {:changed? true :served-version (:served-version resolved) :values (:values resolved) :resolved-ambiguity? true :results [] :attempts (:attempts result)} (assoc result :changed? true :values desired)))))))

(defn- reconcile-projection!
  "Drive one subject/predicate projection to (DESIRED-FN current-values) as one\n   atomic batch. The read and the write are fenced by the read's own served\n   version, so a losing race is a typed :rpc/conflict and never a silent clobber;\n   the re-ask re-reads the projection. A caller that PINS :expected-version owns\n   its own base, so its conflict is an answer and is never retried here." [^Client client subject predicate desired-fn options]
  (let [limit (if (contains? options :expected-version) 1 (:max-attempts client))]
  (loop [attempt 1]
  (let [outcome (try
  {:value (reconcile-once! client subject predicate desired-fn options)}
  (catch clojure.lang.ExceptionInfo error
    (if (and (= :rpc/conflict (:type (ex-data error))) (< attempt limit)) {:conflict error} (throw error))))]
  (if (:conflict outcome) (do
  (retry-pause! client attempt)
  (recur (inc attempt))) (:value outcome))))))

(defn profile-write!
  "Set one subject/predicate projection with North set semantics. CARDINALITY is\n   :one or :many. OCC and ambiguity resolution use the same exact projection."
  ([client subject predicate desired-values]
    (profile-write! client subject predicate desired-values {}))
  ([client subject predicate desired-values options]
    (let [cardinality (:cardinality options :many)
   desired (vec (distinct desired-values))]
  (if (not (contains? #{:one :many} cardinality)) (do
  (throw (ex-info "profile cardinality must be :one or :many" {:type :rpc/invalid-profile :cardinality cardinality}))))
  (if (and (= cardinality :one) (> (count desired) 1)) (do
  (throw (ex-info "single-valued profile accepts at most one value" {:type :rpc/cardinality-violation :subject subject :predicate predicate}))))
  (reconcile-projection! client subject predicate (constantly desired) options))))

(def ^:dynamic *env* (fn [%1] (System/getenv %1)))

(def ^:private cardinality-cache (atom {}))

(defn reset-cardinality-cache!
  "Cardinality is cached for the life of the process. Tests and any caller that\n   changes a declaration in-process must drop the cache explicitly." []
  (reset! cardinality-cache {})
  nil)

(defn- predicate-name [predicate]
  (if (keyword? predicate) (name predicate) (str predicate)))

(defn- env-single-valued []
  (into #{} (remove str/blank? (str/split (or (*env* "BEAGLE_STORE_SINGLE_VALUED") "") #"\s+"))))

(defn- declared-cardinality! [client predicate]
  (let [rows (:rows (scan-all! client (str "@" (predicate-name predicate)) "cardinality" nil))
   values (into #{} (map (comp str t/triple-t3)) rows)]
  (cond
  (contains? values "single") :one
  (contains? values "multi") :many
  :else nil)))

(defn cardinality-of!
  "North cardinality of PREDICATE — :one (declared single: an assertion\n   supersedes the live value) or :many (values coexist)." [client predicate]
  (let [cache-key [(:host client) (:port client) (:space-id client) (predicate-name predicate)]]
  (or (get (deref cardinality-cache) cache-key) (let [resolved (or (declared-cardinality! client predicate) (if (contains? (env-single-valued) (predicate-name predicate)) (do
  :one)) :many)]
  (swap! cardinality-cache assoc cache-key resolved)
  resolved))))

(defn assert-projected!
  "North assert semantics. A repeated identical assertion is idempotent, and a\n   declared-single predicate's assertion supersedes its live value as one batch.\n   This is the write verb a North caller uses; assert! is the raw\n   occurrence-appending primitive underneath it."
  ([client proposition]
    (assert-projected! client proposition {}))
  ([client proposition options]
    (let [subject (t/triple-t1 proposition)
   predicate (t/triple-t2 proposition)
   value (t/triple-t3 proposition)
   cardinality (or (:cardinality options) (cardinality-of! client predicate))
   desired-fn (if (= :one cardinality) (constantly [value]) (fn [current] (if (some (fn [%1] (= value %1)) current) (vec current) (conj (vec current) value))))]
  (assoc (reconcile-projection! client subject predicate desired-fn (dissoc options :cardinality)) :cardinality cardinality))))

(defn retract-projected!
  "North retract semantics: the value LEAVES the exact projection, every equal\n   occurrence with it. retract! withdraws only the latest equal occurrence, so a\n   value appended twice would otherwise survive its own retraction."
  ([client proposition]
    (retract-projected! client proposition {}))
  ([client proposition options]
    (let [subject (t/triple-t1 proposition)
   predicate (t/triple-t2 proposition)
   value (t/triple-t3 proposition)]
  (reconcile-projection! client subject predicate (fn [current] (vec (remove (fn [%1] (= value %1)) current))) (dissoc options :cardinality)))))

(def batch-outcomes "Every outcome fenced-batch! can report, with the three facts a caller decides\n   on. :applied? nil is genuinely unknown and only an exact readback resolves it.\n   :replan-safe? false means a fresh scan CANNOT be read as truth yet." {:no-op {:applied? false :zero-applied? true :retry-identical-safe? true :replan-safe? true} :applied {:applied? true :zero-applied? false :retry-identical-safe? false :replan-safe? true} :conflict {:applied? false :zero-applied? true :retry-identical-safe? false :replan-safe? true} :fence-mismatch {:applied? false :zero-applied? true :retry-identical-safe? false :replan-safe? true} :lease-held {:applied? false :zero-applied? true :retry-identical-safe? false :replan-safe? true} :rejected {:applied? false :zero-applied? true :retry-identical-safe? false :replan-safe? true} :not-sent {:applied? false :zero-applied? true :retry-identical-safe? true :replan-safe? true} :sent-ambiguous {:applied? nil :zero-applied? false :retry-identical-safe? true :replan-safe? true} :durability-ambiguous {:applied? nil :zero-applied? false :retry-identical-safe? false :replan-safe? false :restart-required? true}})

(defn- outcome-result [outcome extra]
  (merge {:outcome outcome} (get batch-outcomes outcome) extra))

(def ^:private mutation-failure-outcomes {:rpc/conflict :conflict :rpc/lease-fence-mismatch :fence-mismatch :rpc/lease-held :lease-held :durability-ambiguous :durability-ambiguous})

(defn- classify-mutation-failure
  "Classify ERROR as a mutation outcome, or rethrow it. A daemon answer carries\n   :code, a transport failure carries :request-sent?, and anything with neither\n   is a local/protocol fault that must not be dressed up as a write outcome." [error]
  (let [data (or (ex-data error) {})]
  (cond
  (= :rpc/ambiguous-write (:type data)) (let [cause (ex-cause error)
   code (:code (or (ex-data cause) {}))]
  (outcome-result (if (= :durability-ambiguous code) :durability-ambiguous :sent-ambiguous) {:code (or code :rpc/ambiguous-write) :error error}))
  (contains? data :code) (outcome-result (get mutation-failure-outcomes (:code data) :rejected) {:code (:code data) :error error :served-version (:served-version data)})
  (contains? data :request-sent?) (outcome-result (if (false? (:request-sent? data)) :not-sent :sent-ambiguous) {:code (:type data) :error error})
  :else (throw error))))

(defn- require-action-results! [actions result]
  (let [results (:results result)]
  (if (not (and (= (count results) (count actions)) (= (map (fn [action-result] (:input-index action-result)) results) (range (count actions))))) (do
  (throw (ex-info "Store RPC batch acknowledged an action-result shape that does not match its actions" {:type :rpc/unexpected-action-results :actions (count actions) :results (mapv (fn [action-result] (:input-index action-result)) results)})))))
  result)

(defn fenced-batch!
  "Submit ACTIONS as ONE fenced, expected-version transaction and classify its\n   outcome instead of retrying it. Pass :fence and :expected-version from the\n   same read that produced the plan. Returns an entry of batch-outcomes plus\n   :results/:changed?/:served-version on an acknowledged commit; an acknowledged\n   commit whose action results do not match its actions throws, because that is\n   a protocol disagreement and not a publication outcome."
  ([client actions]
    (fenced-batch! client actions {}))
  ([client actions options]
    (open! client)
    (if (empty? actions) (outcome-result :no-op {:results [] :changed? false :attempts 0}) (let [single (assoc client :max-attempts 1)
   request-options (dissoc options :ambiguity-resolver)
   outcome (try
  {:ok (batch! single actions request-options)}
  (catch Throwable error
    {:error error}))]
  (let [result (:ok outcome)]
  (if result (let [{:keys [results] :as acked} (require-action-results! actions result)]
  (outcome-result :applied {:results results :changed? (boolean (some :changed? results)) :served-version (:served-version acked) :attempts (:attempts acked)})) (classify-mutation-failure (:error outcome))))))))

(defn- occurrence-excess
  "Occurrences FROM holds beyond OTHER, as an occurrence map." [from other]
  (into {} (keep (fn [[predicate values]] (let [surplus (into {} (keep (fn [[value count-value]] (let [delta (- (long count-value) (long (get-in other [predicate value] 0)))]
  (if (pos? delta) (do
  [value delta]))))) values)]
  (if (seq surplus) (do
  [predicate surplus]))))) from))

(defn subject-readback!
  "Re-scan SUBJECT and decide what a fenced write actually did. DESIRED is an\n   occurrence map and the comparison is exact in BOTH directions — value set AND\n   occurrence frequency. Options: :before, the pre-write occurrence map, which is\n   what lets an unchanged projection be reported as proven-absent rather than\n   merely not-committed; :guard-predicates, predicates required to be absent;\n   :scope :subject to compare the WHOLE subject instead of only the predicates\n   DESIRED and the guards name.\n\n   States: :committed (exact), :absent (identical to :before, so the batch never\n   landed and a replan is safe), :foreign-writer (equal to neither, which this\n   atomic batch cannot have produced — another writer or corruption intervened),\n   :indeterminate (a mismatch with no baseline to attribute it to)."
  ([client subject desired]
    (subject-readback! client subject desired {}))
  ([client subject desired options]
    (let [{:keys [before scope guard-predicates]} options
   projection (subject-projection! client subject (dissoc options :before :scope :guard-predicates))
   actual (:occurrences projection)
   compared (if (= :subject scope) (into (set (keys actual)) (keys desired)) (into (set (keys desired)) guard-predicates))
   restrict (fn [occurrences] (into {} (keep (fn [predicate] (let [values (get occurrences predicate)]
  (if (seq values) (do
  [predicate values]))))) compared))
   actual-view (restrict actual)
   desired-view (restrict desired)
   state (cond
  (= actual-view desired-view) :committed
  (and before (= actual-view (restrict before))) :absent
  before :foreign-writer
  :else :indeterminate)]
  {:state state :committed? (= state :committed) :indeterminate? (contains? #{:foreign-writer :indeterminate} state) :served-version (:served-version projection) :occurrences actual :missing (occurrence-excess desired-view actual-view) :unexpected (occurrence-excess actual-view desired-view)})))

(defn fence-parts
  "[resource holder epoch] of a typed lease fence." [fence]
  (record-fields fence :rpc/fence 3))

(defn lease-acquire-at-version!
  "Acquire RESOURCE for HOLDER as the transaction immediately after\n   EXPECTED-VERSION. A commit's epoch is EXPECTED-VERSION+1, which is what makes\n   the fence RECONSTRUCTABLE when the acknowledgement is lost: :candidate-fence\n   names the lease this call would have taken, and lease-check! decides. OCC\n   admits at most one committing acquire, so an identical retry at the same\n   expected version is safe. On :conflict and :lease-held nothing committed."
  ([^Client client ^String resource ^String holder ttl-ms expected-version]
    (lease-acquire-at-version! client resource holder ttl-ms expected-version {}))
  ([^Client client ^String resource ^String holder ttl-ms expected-version options]
    (open! client)
    (let [candidate (wire/rpc-fence! resource holder (inc expected-version))
   outcome (try
  {:ok (lease-acquire! (assoc client :max-attempts 1) resource holder ttl-ms (assoc options :expected-version expected-version))}
  (catch Throwable error
    {:error error}))]
  (let [grant (:ok outcome)]
  (if grant (let [[_lease-subject _lease-token epoch] (fence-parts (:fence grant))]
  (outcome-result :applied {:acquired? true :fence (:fence grant) :epoch epoch :expected-epoch (inc expected-version) :expires (:expires grant) :served-version (:served-version grant) :attempts (:attempts grant)})) (let [classified (classify-mutation-failure (:error outcome))]
  (assoc classified :acquired? (:applied? classified) :candidate-fence candidate)))))))

(defn subscribe! [& $beagle$rest$host]
  (let [_args (vec $beagle$rest$host)]
  (throw (ex-info "Store RPC subscription is reserved for the daemon-side v2 operation" {:type :rpc/subscription-unavailable :operation subscription-operation :stage :post-stage-1}))))
