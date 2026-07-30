(ns north.attention
  "Durable observer subscriptions and replayable semantic notifications."
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.set :as set]
            [clojure.string :as str]))

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def default-event-filters
  #{"progress" "outcome" "dependency" "evidence" "activity" "changed"})
(def delivery-values #{"inbox" "notify"})
(def max-event-key-bytes 4096)
(def max-address-bytes 1024)
(def max-reference-bytes 4096)
(def max-attention-kind-bytes 256)
(def max-notification-subject-bytes 4096)
(def max-notification-body-bytes 65536)
(def replay-buffer-bytes 65536)
;; Fram's canonical flat-log reader rejects records above 1 MiB. Replay streams
;; an arbitrarily large cursor gap but retains this one-record safety bound.
(def max-log-record-bytes (* 1024 1024))
(def max-event-group-bytes (* 8 1024 1024))
(def max-event-group-records 65536)
(def max-replay-notification-id-sample 64)
(def notification-mutable-predicates #{"read_by" "read_at"})

(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

(defn utf8-prefix [value max-bytes]
  (let [value (str value)
        length (.length value)]
    (loop [index 0
           used 0]
      (if (= index length)
        value
        (let [code-point (.codePointAt value index)
              next-index (+ index (java.lang.Character/charCount code-point))
              piece (.substring value index next-index)
              piece-bytes (utf8-bytes piece)]
          (if (> (+ used piece-bytes) max-bytes)
            (.substring value 0 index)
            (recur next-index (+ used piece-bytes))))))))

(defn contains-control-character? [value]
  (boolean
   (some #(java.lang.Character/isISOControl (int %)) (str value))))

(defn fail! [message data]
  (throw (ex-info message data)))

(defn ref-value? [value]
  (and (string? value)
       (> (count value) 1)
       (str/starts-with? value "@")
       (<= (utf8-bytes value) max-reference-bytes)
       (not (re-find #"\s" value))
       (not (contains-control-character? value))))

(defn sha256 [domain value]
  (let [digest
        (.digest
         (java.security.MessageDigest/getInstance "SHA-256")
         (.getBytes (str domain "\u0000" value)
                    java.nio.charset.StandardCharsets/UTF_8))]
    (apply str (map #(format "%02x" (bit-and (int %) 0xff)) digest))))

(defn canonical-tuple [values]
  ;; EDN string escaping and vector boundaries make this unambiguous even when
  ;; caller-controlled values contain punctuation used by older key formats.
  (pr-str (vec values)))

(def prefix-anchor-window-bytes 65536)

(defn read-file-range! [path offset length]
  (let [bytes (byte-array (int length))]
    (with-open [raf (java.io.RandomAccessFile. (str path) "r")]
      (when (> (+ (long offset) (long length)) (.length raf))
        (fail! "attention log was truncated during replay"
               {:type :attention-log-truncated
                :offset offset :length length :path (str path)}))
      (.seek raf (long offset))
      (.readFully raf bytes))
    bytes))

(defn complete-log-offset [path]
  (let [file (io/file path)
        length (.length file)]
    (if (zero? length)
      0
      (with-open [raf (java.io.RandomAccessFile. file "r")]
        (.seek raf (dec length))
        (if (= 10 (.read raf))
          length
          (loop [at (- length 2)]
            (cond
              (neg? at) 0
              :else
              (do
                (.seek raf at)
                (if (= 10 (.read raf))
                  (inc at)
                  (recur (dec at)))))))))))

(defn prefix-anchor [path offset]
  (let [file (io/file path)
        offset (long offset)
        length (.length file)]
    (when-not (<= 0 offset length)
      (fail! "attention cursor is outside the physical log"
             {:type :attention-cursor-outside-log
              :offset offset :length length}))
    (when (and (pos? offset)
               (not= 10 (aget (read-file-range! path (dec offset) 1) 0)))
      (fail! "attention cursor is not at a complete log-line boundary"
             {:type :attention-cursor-not-line-boundary :offset offset}))
    (let [start (max 0 (- offset prefix-anchor-window-bytes))
          boundary (read-file-range! path start (- offset start))
          head (read-file-range! path 0 (min offset 4096))
          file-key
          (str
           (.fileKey
            (java.nio.file.Files/readAttributes
             (.toPath file)
             java.nio.file.attribute.BasicFileAttributes
             (make-array java.nio.file.LinkOption 0))))]
      (sha256
       "north-attention-prefix-anchor-v1"
       (str offset "\u0000" file-key "\u0000"
            (.encodeToString (java.util.Base64/getEncoder) head)
            "\u0000"
            (.encodeToString (java.util.Base64/getEncoder) boundary))))))

(defn physical-log-position []
  (let [path (north.coord/expected-log)
        offset (complete-log-offset path)]
    {:offset offset :anchor (prefix-anchor path offset)}))

(defn notification-subject [event-key recipient]
  (str "@notification:"
       (sha256 "north-attention-notification-v3"
               (canonical-tuple [event-key recipient]))))

(defn fresh-subscription-subject []
  (str "@subscription:" (java.util.UUID/randomUUID)))

(defn exact-facts [port subject]
  (->> (north.coord/query-rows
        port
        {:find "attention_fact"
         :rules
         [{:head {:rel "attention_fact" :args [{:var "p"} {:var "r"}]}
           :body [{:rel "triple" :args [subject {:var "p"} {:var "r"}]}]}]})
       (mapv (fn [[predicate value]] [predicate value]))
       set))

(defn entity-exists? [port subject]
  (boolean (seq (exact-facts port subject))))

(defn normalize-ref [value]
  (let [value (str value)]
    (if (str/starts-with? value "@") value (str "@" value))))

(defn valid-address? [value]
  (and (string? value)
       (not (str/blank? value))
       (<= (utf8-bytes value) max-address-bytes)
       (not (re-find #"\s" value))
       (not (contains-control-character? value))))

(defn attention-address
  "Map an ownership ref to the listener's existing literal routing address.
   Ownership and read state remain on recipient/subscriber refs; this value is
   only the scoped-stream wake edge."
  [principal]
  (let [principal (normalize-ref principal)
        address
        (cond
          (str/starts-with? principal "@agent:") (subs principal 7)
          (str/starts-with? principal "@role:") (subs principal 6)
          :else (subs principal 1))]
    (when-not (valid-address? address)
      (fail! "attention principal has no valid listener address"
             {:type :invalid-attention-address :principal principal}))
    address))

(defn require-principal! [port principal]
  (let [principal (normalize-ref principal)]
    (when-not (ref-value? principal)
      (fail! "attention principal must be an entity reference"
             {:type :invalid-attention-principal :principal principal}))
    (cond
      (str/starts-with? principal "@role:")
      (when-not (entity-exists? port principal)
        (fail! "attention role principal does not exist"
               {:type :unknown-attention-principal :principal principal}))

      (str/starts-with? principal "@agent:")
      (when-not (entity-exists? port principal)
        (fail! "attention agent principal does not exist"
               {:type :unknown-attention-principal :principal principal}))

      :else
      (when (str/blank? (north.coord/resolved port principal "display_name"))
        (fail! "attention person principal must have display_name"
               {:type :unknown-attention-principal :principal principal})))
    principal))

(defn require-thread! [port thread]
  (let [thread (normalize-ref thread)
        title (north.coord/resolved port thread "title")
        kind (north.coord/resolved port thread "kind")]
    (when-not (and (ref-value? thread)
                   (not (str/blank? title))
                   (or (nil? kind) (= "thread" kind)))
      (fail! "attention follow target must be a title-bearing thread"
             {:type :invalid-attention-thread :thread thread :kind kind}))
    thread))

(defn role-principals [port agent-id]
  (->> (north.coord/many port (str "@agent:" agent-id) "holds")
       (filter #(and (string? %) (str/starts-with? % "@role:")))
       distinct
       sort
       vec))

(defn default-principal [port]
  (if-let [agent-id (not-empty (System/getenv "AGENT_ID"))]
    (let [roles (role-principals port agent-id)]
      (if (= 1 (count roles))
        (first roles)
        (fail! "AGENT_ID does not hold exactly one durable role; pass --as"
               {:type :ambiguous-attention-principal
                :agent-id agent-id
                :roles roles})))
    (if-let [author (not-empty (System/getenv "NORTH_AUTHOR"))]
      (normalize-ref author)
      (fail! "attention principal requires --as, AGENT_ID, or NORTH_AUTHOR"
             {:type :missing-attention-principal}))))

(defn active-subscription-ids
  ([port principal]
   (active-subscription-ids port principal nil))
  ([port principal about]
   (let [candidate-body
         (cond-> [{:rel "triple"
                   :args [{:var "subscription"} "kind" "subscription"]}
                  {:rel "triple"
                   :args [{:var "subscription"} "subscriber" principal]}]
           about
           (conj {:rel "triple"
                  :args [{:var "subscription"} "about" about]}))
         query
         {:find "active_subscription"
          :strata
          [[{:head {:rel "subscription_candidate"
                    :args [{:var "subscription"}]}
             :body candidate-body}
            {:head {:rel "subscription_ended"
                    :args [{:var "subscription"}]}
             :body [{:rel "triple"
                     :args [{:var "subscription"} "ended_at" {:var "ended"}]}]}]
           [{:head {:rel "active_subscription"
                    :args [{:var "subscription"}]}
             :body [{:rel "subscription_candidate"
                     :args [{:var "subscription"}]}
                    {:rel "subscription_ended"
                     :args [{:var "subscription"}]
                     :neg true}]}]]}]
     (->> (north.coord/query-rows port query)
          (map first)
          distinct
          sort
          vec))))

(defn subscription-row [port subscription]
  {:id subscription
   :subscriber (north.coord/resolved port subscription "subscriber")
   :about (north.coord/resolved port subscription "about")
   :event-filters (set (north.coord/many port subscription "event_filter"))
   :delivery (north.coord/resolved port subscription "delivery")
   :start-version (north.coord/resolved port subscription "start_version")
   :cursor-version (north.coord/resolved port subscription "cursor_version")
   :start-offset (north.coord/resolved port subscription "start_offset")
   :cursor-offset (north.coord/resolved port subscription "cursor_offset")
   :cursor-anchor (north.coord/resolved port subscription "cursor_anchor")
   :created-at (north.coord/resolved port subscription "created_at")
   :ended-at (north.coord/resolved port subscription "ended_at")
   :end-version (north.coord/resolved port subscription "end_version")
   :end-offset (north.coord/resolved port subscription "end_offset")
   :end-anchor (north.coord/resolved port subscription "end_anchor")})

(defn require-event-filters! [event-filters]
  (let [event-filters (set event-filters)
        unknown (set/difference event-filters default-event-filters)]
    (when (empty? event-filters)
      (fail! "attention follow requires at least one event filter"
             {:type :empty-attention-event-filter}))
    (when (seq unknown)
      (fail! "attention follow has unsupported event filters"
             {:type :invalid-attention-event-filter :unknown (sort unknown)}))
    event-filters))

(defn require-delivery! [delivery]
  (when-not (contains? delivery-values delivery)
    (fail! "attention delivery must be inbox or notify"
           {:type :invalid-attention-delivery :delivery delivery}))
  delivery)

(defn follow!
  [port {:keys [principal about event-filters delivery]
         :or {event-filters default-event-filters delivery "inbox"}}]
  (let [principal (require-principal! port principal)
        about (require-thread! port about)
        event-filters (require-event-filters! event-filters)
        delivery (require-delivery! delivery)
        candidate (fresh-subscription-subject)
        result
        (north.coord/assert-batch-after-read!
         port candidate
         (fn []
           (let [active (active-subscription-ids port principal about)]
             (when (> (count active) 1)
               (fail! "multiple active subscriptions exist for one principal/thread"
                      {:type :ambiguous-active-subscription
                       :principal principal :about about :subscriptions active}))
             (if-let [existing (first active)]
               (let [row (subscription-row port existing)]
                 (when-not (and (= event-filters (:event-filters row))
                                (= delivery (:delivery row)))
                   (fail! "active follow has different filters or delivery; unfollow it first"
                          {:type :active-subscription-conflict
                           :subscription existing}))
                 {:done existing})
               (let [start-version (north.coord/cur-ver port)
                     {:keys [offset anchor]} (physical-log-position)
                     created-at (str (java.time.Instant/now))]
                 {:facts
                  (vec
                   (concat
                    [{:p "subscriber" :r principal}
                     {:p "about" :r about}
                     {:p "delivery" :r delivery}
                     {:p "start_version" :r (str start-version)}
                     {:p "cursor_version" :r (str start-version)}
                     {:p "start_offset" :r (str offset)}
                     {:p "cursor_offset" :r (str offset)}
                     {:p "cursor_anchor" :r anchor}
                     {:p "created_at" :r created-at}]
                    (map (fn [event-filter]
                           {:p "event_filter" :r event-filter})
                         (sort event-filters))
                    [{:p "kind" :r "subscription"}
                     {:p "entity_kind" :r "subscription"}
                     ;; Routing-only activation edge for an already-armed
                     ;; scoped listener. Ownership remains on subscriber.
                     {:p "target" :r (attention-address principal)}]))}))))
         Integer/MAX_VALUE
         (north.coord/retry-deadline-ns))]
    (cond
      (:done result) (:done result)
      (:ok result) candidate
      :else
      (fail! "attention follow publication failed"
             {:type :attention-follow-publication-failed :result result}))))

(defn ended-subscriptions-with-target
  [port principal about address]
  (->> (north.coord/query-rows
        port
        {:find "ended_subscription_with_target"
         :rules
         [{:head {:rel "ended_subscription_with_target"
                  :args [{:var "subscription"}]}
           :body [{:rel "triple"
                   :args [{:var "subscription"} "kind" "subscription"]}
                  {:rel "triple"
                   :args [{:var "subscription"} "subscriber" principal]}
                  {:rel "triple"
                   :args [{:var "subscription"} "about" about]}
                  {:rel "triple"
                   :args [{:var "subscription"} "ended_at" {:var "ended"}]}
                  {:rel "triple"
                   :args [{:var "subscription"} "target" address]}]}]})
       (map first)
       distinct
       sort
       vec))

(defn retract-subscription-target! [port subscription address]
  (let [result
        (north.coord/retract! port subscription "target" address)]
    (when (:reject result)
      (fail! "attention subscription target retraction failed"
             {:type :attention-subscription-target-retraction-failed
              :subscription subscription
              :result result}))
    (when (= address
             (north.coord/resolved port subscription "target"))
      (fail! "attention subscription target remained after retraction"
             {:type :attention-subscription-target-retraction-mismatch
              :subscription subscription
              :address address}))
    subscription))

(defn heal-ended-subscription-targets! [port principal about address]
  (let [subscriptions
        (ended-subscriptions-with-target port principal about address)]
    (doseq [subscription subscriptions]
      (retract-subscription-target! port subscription address))
    subscriptions))

(defn unfollow! [port {:keys [principal about]}]
  (let [principal (require-principal! port principal)
        about (require-thread! port about)
        address (attention-address principal)
        active (active-subscription-ids port principal about)]
    (when (> (count active) 1)
      (fail! "multiple active subscriptions exist for one principal/thread"
             {:type :ambiguous-active-subscription
              :principal principal :about about :subscriptions active}))
    (if-let [subscription (first active)]
      (let [result
            (north.coord/assert-batch-after-read!
             port subscription
             (fn []
               (when-not (contains?
                          (set (active-subscription-ids port principal about))
                          subscription)
                 (fail! "attention subscription ended concurrently"
                        {:type :attention-unfollow-conflict
                         :subscription subscription}))
               (let [{:keys [offset anchor]} (physical-log-position)]
                 {:facts
                 [{:p "end_offset" :r (str offset)}
                   {:p "end_anchor" :r anchor}
                   {:p "end_version" :r (str (north.coord/cur-ver port))}
                   {:p "ended_at" :r (str (java.time.Instant/now))}]})))]
        (when-not (:ok result)
          (fail! "attention unfollow publication failed"
                 {:type :attention-unfollow-publication-failed :result result}))
        ;; End authority lands first. The explicit target retract is only a wake/
        ;; transport-scope cleanup edge; a retry heals it after a lost response
        ;; or a process death between these two durable commits.
        (retract-subscription-target! port subscription address)
        subscription)
      (do
        (heal-ended-subscription-targets! port principal about address)
        nil))))

(defn following [port principal]
  (mapv #(subscription-row port %)
        (active-subscription-ids port (require-principal! port principal))))

(defn require-optional-ref! [label value]
  (when (and value (not (ref-value? value)))
    (fail! (str label " must be an entity reference")
           {:type :invalid-attention-reference :field label :value value}))
  value)

(defn canonical-notification-spec [spec]
  (let [allowed
        #{:event-key :to :about :attention-kind :subject :body :source-version
          :source-concerns :subscription :delivery}
        unknown (set/difference (set (keys spec)) allowed)
        event-key (:event-key spec)
        recipient (:to spec)
        delivery (or (:delivery spec) "inbox")
        attention-kind (or (:attention-kind spec) "changed")
        subject (or (:subject spec) attention-kind)
        body (or (:body spec) subject)
        source-concerns (->> (or (:source-concerns spec) []) set sort vec)]
    (when (seq unknown)
      (fail! "notification spec has unsupported keys"
             {:type :invalid-notification-spec :unknown (sort unknown)}))
    (when-not (and (string? event-key)
                   (not (str/blank? event-key))
                   (<= (utf8-bytes event-key) max-event-key-bytes)
                   (not (contains-control-character? event-key)))
      (fail! "notification event-key is missing, too large, or contains control characters"
             {:type :invalid-notification-event-key}))
    (when-not (ref-value? recipient)
      (fail! "notification recipient must be an entity reference"
             {:type :invalid-notification-recipient :recipient recipient}))
    (require-delivery! delivery)
    (doseq [[label value] [["about" (:about spec)]
                           ["subscription" (:subscription spec)]]]
      (require-optional-ref! label value))
    (doseq [source-concern source-concerns]
      (require-optional-ref! "source-concern" source-concern))
    (when-not (and (string? attention-kind)
                   (not (str/blank? attention-kind))
                   (<= (utf8-bytes attention-kind)
                       max-attention-kind-bytes)
                   (not (re-find #"\s" attention-kind))
                   (not (contains-control-character? attention-kind))
                   (string? subject) (not (str/blank? subject))
                   (string? body) (not (str/blank? body))
                   (<= (utf8-bytes subject)
                       max-notification-subject-bytes)
                   (<= (utf8-bytes body)
                       max-notification-body-bytes))
      (fail! "notification kind, subject, and body must be nonblank and within attention content bounds"
             {:type :invalid-notification-content}))
    (when (and (some? (:source-version spec))
               (or (not (integer? (:source-version spec)))
                   (neg? (:source-version spec))))
      (fail! "notification source-version must be a nonnegative integer"
             {:type :invalid-notification-source-version
              :source-version (:source-version spec)}))
    {:event-key event-key
     :recipient recipient
     :delivery delivery
     :about (:about spec)
     :attention-kind attention-kind
     :subject subject
     :body body
     :source-version (:source-version spec)
     :source-concerns source-concerns
     :subscription (:subscription spec)}))

(defn notification-fixed-facts
  [{:keys [event-key recipient delivery about attention-kind subject body
           source-version source-concerns subscription]}]
  (vec
   (concat
    [["from" "north"]
     ["subject" subject]
     ["body" body]
     ["event_key" event-key]
     ["attention_kind" attention-kind]
     ["delivery" delivery]]
    (when about [["about" about]])
    (when (some? source-version)
      [["source_version" (str source-version)]])
    (when subscription [["subscription" subscription]])
    (map (fn [source-concern] ["source_concern" source-concern])
         source-concerns)
    [["kind" "notification"]
     ["entity_kind" "notification"]
     ["recipient" recipient]
     ;; Routing-only activation edge. Durable ownership remains on recipient;
     ;; `target` wakes attention-aware listeners without entering ordinary mail.
     ["target" (attention-address recipient)]])))

(defn existing-notification-compatible?
  [facts fixed-facts]
  (let [immutable
        (set (remove (fn [[predicate _]]
                       (or (= "sent_at" predicate)
                           (contains? notification-mutable-predicates predicate)))
                     facts))
        sent-at (set (for [[predicate value] facts :when (= "sent_at" predicate)]
                       value))]
    (and (= (set fixed-facts) immutable)
         (= 1 (count sent-at))
         (not (str/blank? (first sent-at))))))

(defn publish-notification!
  "Publish one deterministic notification. Identical EVENT-KEY/spec calls are
   idempotent; a collision carrying different immutable facts fails closed."
  [port spec]
  (let [spec (canonical-notification-spec spec)
        subject (notification-subject (:event-key spec) (:recipient spec))
        fixed-facts (notification-fixed-facts spec)
        result
        (north.coord/assert-batch-after-read!
         port subject
         (fn []
           (let [existing (exact-facts port subject)]
             (cond
               (empty? existing)
               {:facts
                (mapv (fn [[predicate value]] {:p predicate :r value})
                      (vec
                       (concat
                        (butlast fixed-facts)
                        [["sent_at" (str (java.time.Instant/now))]
                         (last fixed-facts)])))}

               (existing-notification-compatible? existing fixed-facts)
               {:done subject}

               :else
               (fail! "notification event-key collides with different facts"
                      {:type :notification-event-key-collision
                       :notification subject})))))]
    (cond
      (:done result) (:done result)
      (:ok result) subject
      :else
      (fail! "notification publication failed"
             {:type :notification-publication-failed :result result}))))

(defn parse-position! [label value]
  (let [parsed (when (and (string? value) (re-matches #"[0-9]+" value))
                 (parse-long value))]
    (when-not (and (integer? parsed) (not (neg? parsed)))
      (fail! (str label " is not a nonnegative integer")
             {:type :invalid-attention-position :field label :value value}))
    parsed))

(defn semantic-event-kind [predicate]
  (cond
    (= "progress" predicate) "progress"
    (#{"outcome" "abandoned"} predicate) "outcome"
    (= "depends_on" predicate) "dependency"
    (#{"done_when" "bar_evidence" "bar_evidence_unreserved"} predicate) "evidence"
    (= "driver" predicate) "activity"
    :else "changed"))

(defn decode-log-utf8! [bytes line-start]
  (try
    (let [decoder
          (doto (.newDecoder java.nio.charset.StandardCharsets/UTF_8)
            (.onMalformedInput java.nio.charset.CodingErrorAction/REPORT)
            (.onUnmappableCharacter java.nio.charset.CodingErrorAction/REPORT))]
      (str (.decode decoder (java.nio.ByteBuffer/wrap bytes))))
    (catch java.nio.charset.CharacterCodingException error
      (throw
       (ex-info "attention replay encountered non-UTF-8 log data"
                {:type :malformed-attention-log
                 :line-start-offset line-start}
                error)))))

(defn parse-log-record! [bytes line-start line-end]
  (let [text (decode-log-utf8! bytes line-start)]
    (when-not (str/blank? text)
      (try
        (with-open
          [reader
           (java.io.PushbackReader. (java.io.StringReader. text))]
          (let [eof (Object.)
                record (edn/read {:eof eof} reader)
                trailing (edn/read {:eof eof} reader)]
            (when (or (identical? eof record)
                      (not (identical? eof trailing))
                      (not (map? record)))
              (fail! "attention replay encountered malformed log data"
                     {:type :malformed-attention-log
                      :line-start-offset line-start}))
            {:record record
             :start-offset line-start
             :end-offset line-end}))
        (catch clojure.lang.ExceptionInfo error
          (throw error))
        (catch Exception error
          (throw
           (ex-info "attention replay encountered malformed log data"
                    {:type :malformed-attention-log
                     :line-start-offset line-start}
                    error)))))))

(defn append-log-line-bytes! [output buffer start length line-start]
  (let [next-size (+ (.size output) length)]
    (when (> next-size max-log-record-bytes)
      (fail! "attention replay log record exceeds the canonical record bound"
             {:type :attention-log-record-too-large
              :line-start-offset line-start
              :max-bytes max-log-record-bytes}))
    (.write output buffer start length)))

(defn stream-log-records!
  "Stream every complete physical log record in [AFTER-OFFSET, THROUGH-OFFSET)
   through CONSUME!. Memory is bounded by one canonical record plus one fixed
   read buffer, independent of the total disconnected replay gap."
  [path after-offset through-offset consume!]
  (let [after-offset (long after-offset)
        through-offset (long through-offset)
        length (- through-offset after-offset)
        file (io/file path)]
    (when (neg? length)
      (fail! "attention replay boundary precedes its cursor"
             {:type :attention-replay-boundary-before-cursor
              :cursor after-offset :boundary through-offset}))
    (when (> through-offset (.length file))
      (fail! "attention log was truncated during replay"
             {:type :attention-log-truncated
              :offset after-offset :length length :path (str path)}))
    (when (pos? length)
      (with-open [raf (java.io.RandomAccessFile. file "r")]
        (.seek raf after-offset)
        (let [buffer (byte-array replay-buffer-bytes)
              output (java.io.ByteArrayOutputStream.)
              latin1 java.nio.charset.StandardCharsets/ISO_8859_1]
          (loop [position after-offset
                 line-start after-offset]
            (if (= position through-offset)
              (when (pos? (.size output))
                (fail! "attention replay boundary ended inside a log record"
                       {:type :attention-log-boundary-not-record-boundary
                        :boundary through-offset
                        :line-start-offset line-start}))
              (let [wanted
                    (int (min (long replay-buffer-bytes)
                              (- through-offset position)))
                    read-count (.read raf buffer 0 wanted)]
                (when (neg? read-count)
                  (fail! "attention log was truncated during replay"
                         {:type :attention-log-truncated
                          :offset position :path (str path)}))
                (if (zero? read-count)
                  (recur position line-start)
                  (let [segment (String. buffer 0 read-count latin1)
                        next-line-start
                        (loop [cursor 0
                               current-line-start line-start]
                          (if (= cursor read-count)
                            current-line-start
                            (let [newline (.indexOf segment "\n" cursor)
                                  stop (if (neg? newline) read-count newline)
                                  take-count (- stop cursor)]
                              (append-log-line-bytes!
                               output buffer cursor take-count current-line-start)
                              (if (neg? newline)
                                current-line-start
                                (let [line-end (+ position newline 1)
                                      parsed
                                      (parse-log-record!
                                       (.toByteArray output)
                                       current-line-start
                                       line-end)]
                                  (.reset output)
                                  (when parsed (consume! parsed))
                                  (recur (inc newline) line-end))))))]
                    (recur (+ position read-count) next-line-start)))))))))))

(defn normalized-event-digest [about event-kind occurrence events]
  (sha256
   "north-attention-semantic-event-v2"
   (pr-str
    {:about about
     :kind event-kind
     :occurrence occurrence
     :facts
     (->> events
          (map #(select-keys % [:op :l :p :r]))
          (sort-by pr-str)
          vec)})))

(defn replay-internal-record? [{:keys [l frame]}]
  (or (= "merge" frame)
      (and (string? l)
           (or (str/starts-with? l "@subscription:")
               (str/starts-with? l "@notification:")))))

(defn event-group->event [about {:keys [start-offset end-offset events]}]
  (let [events (vec events)
        event-kind (semantic-event-kind (:p (first events)))
        source-version (:tx (first events))
        occurrence [start-offset end-offset]]
    {:source-version
     (when (and (integer? source-version) (not (neg? source-version)))
       source-version)
     :event-kind event-kind
     :occurrence occurrence
     :digest (normalized-event-digest about event-kind occurrence events)
     :events (sort-by (juxt :p :op :r) events)}))

(defn event-group
  [group-key record start-offset end-offset]
  {:key group-key
   :start-offset start-offset
   :end-offset end-offset
   :byte-count (- end-offset start-offset)
   :record-count 1
   :events [record]})

(defn extend-event-group! [group record start-offset end-offset]
  (let [byte-count (+ (:byte-count group) (- end-offset start-offset))
        record-count (inc (:record-count group))]
    (when (or (> byte-count max-event-group-bytes)
              (> record-count max-event-group-records))
      (fail! "attention semantic event group exceeds its canonical transaction bound"
             {:type :attention-event-group-too-large
              :start-offset (:start-offset group)
              :bytes byte-count
              :records record-count
              :max-bytes max-event-group-bytes
              :max-records max-event-group-records}))
    (-> group
        (assoc :end-offset end-offset
               :byte-count byte-count
               :record-count record-count)
        (update :events conj record))))

(defn stream-log-events!
  "Stream semantic events for ABOUT while preserving the old transaction/kind
   partitioning across read-buffer boundaries. CONSUME! completes durably before
   the next event is read. Returns whether the slice contained any non-attention
   record, which controls the self-write cursor tail."
  [path about after-offset through-offset consume!]
  (let [current-group (volatile! nil)
        saw-domain-record? (volatile! false)
        flush-group!
        (fn []
          (when-let [group @current-group]
            (consume! (event-group->event about group))
            (vreset! current-group nil)))]
    (stream-log-records!
     path after-offset through-offset
     (fn [{:keys [record start-offset end-offset]}]
       (when-not (replay-internal-record? record)
         (vreset! saw-domain-record? true))
       (let [{:keys [op l frame tx p]} record]
         (if (and (not= "merge" frame)
                  (= about l)
                  (#{"assert" "retract"} op))
           (let [group-key [l tx (semantic-event-kind p)]
                 group @current-group]
             (if (= group-key (:key group))
               (vreset!
                current-group
                (extend-event-group!
                 group record start-offset end-offset))
               (do
                 (flush-group!)
                 (vreset!
                  current-group
                  (event-group
                   group-key record start-offset end-offset)))))
           ;; A transaction group is a physically contiguous run. Imported or
           ;; repaired tails may reuse :tx; an intervening record still starts a
           ;; new occurrence and must force a deterministic flush.
           (flush-group!)))))
    (flush-group!)
    {:saw-domain-record? @saw-domain-record?}))

(defn event-line [{:keys [op p r]}]
  (str op " " p " = " r))

(defn body-summary [omitted total digest]
  (str "\n… truncated; omitted-events="
       omitted
       "; total-events="
       total
       "; event-digest="
       digest))

(defn event-body [{:keys [events digest]}]
  (let [lines (mapv event-line events)
        full-size
        (+ (reduce + 0 (map utf8-bytes lines))
           (max 0 (dec (count lines))))]
    (if (<= full-size max-notification-body-bytes)
      (str/join "\n" lines)
      (let [total (count lines)
            largest-summary (body-summary total total digest)
            content-budget
            (- max-notification-body-bytes
               (utf8-bytes largest-summary))
            output (StringBuilder.)]
        (loop [index 0
               used 0]
          (let [line (nth lines index)
                piece (str (when (pos? index) "\n") line)
                piece-size (utf8-bytes piece)]
            (if (<= (+ used piece-size) content-budget)
              (do
                (.append output piece)
                (recur (inc index) (+ used piece-size)))
              (let [remaining (- content-budget used)
                    prefix (utf8-prefix piece remaining)]
                (.append output prefix)
                (.append output
                         (body-summary (- total index) total digest))
                (str output)))))))))

(defn event-subject [event-kind thread-title]
  (let [subject (str event-kind ": " thread-title)]
    (if (<= (utf8-bytes subject) max-notification-subject-bytes)
      subject
      (let [digest (subs (sha256 "north-attention-subject-v1" subject) 0 16)
            suffix (str "…[" digest "]")
            budget (- max-notification-subject-bytes (utf8-bytes suffix))]
        (str (utf8-prefix subject budget) suffix)))))

(defn advance-cursor!
  [port subscription expected-offset expected-anchor through-offset through-anchor
   source-high-water]
  (let [result
        (north.coord/assert-batch-after-read!
         port subscription
         (fn []
           (let [current-offset
                 (parse-position!
                  "subscription cursor_offset"
                  (north.coord/resolved port subscription "cursor_offset"))
                 current-anchor
                 (north.coord/resolved port subscription "cursor_anchor")]
             (cond
               (>= current-offset through-offset) {:done current-offset}
               (or (not= expected-offset current-offset)
                   (not= expected-anchor current-anchor))
               (fail! "attention cursor changed concurrently"
                      {:type :attention-cursor-conflict
                       :subscription subscription})
               :else
               {:facts
                [{:p "cursor_offset" :r (str through-offset)}
                 {:p "cursor_anchor" :r through-anchor}
                 {:p "cursor_version" :r (str source-high-water)}]}))))]
    (when-not (or (:ok result) (contains? result :done))
      (fail! "attention cursor advance failed"
             {:type :attention-cursor-advance-failed :result result}))
    through-offset))

(defn sync-subscription! [port subscription high-position source-high-water]
  (let [{:keys [subscriber about event-filters delivery cursor-offset cursor-anchor
                ended-at end-version end-offset end-anchor]}
        (subscription-row port subscription)
        cursor (parse-position! "subscription cursor_offset" cursor-offset)
        path (north.coord/expected-log)
        observed-anchor (prefix-anchor path cursor)
        _ (when-not (= cursor-anchor observed-anchor)
            (fail! "attention log prefix changed before the replay cursor"
                   {:type :attention-log-prefix-changed
                    :subscription subscription :cursor cursor}))
        boundary
        (if ended-at
          (parse-position! "subscription end_offset" end-offset)
          (:offset high-position))
        boundary-anchor
        (if ended-at
          end-anchor
          (:anchor high-position))
        boundary-high-water
        (if ended-at
          (parse-position! "subscription end_version" end-version)
          source-high-water)
        _ (when-not (= boundary-anchor (prefix-anchor path boundary))
            (fail! "attention log prefix changed at the replay boundary"
                   {:type :attention-log-boundary-changed
                    :subscription subscription :boundary boundary}))
        thread-title (or (north.coord/resolved port about "title") about)
        notification-count (volatile! 0)
        notification-id-sample (volatile! [])
        stream-result
        (stream-log-events!
         path about cursor boundary
         (fn [{:keys [source-version event-kind digest] :as event}]
           (when (contains? event-filters event-kind)
             (let [notification
                   (publish-notification!
                    port
                    (cond->
                     {:event-key
                      (canonical-tuple
                       [subscription about event-kind digest])
                      :to subscriber
                      :about about
                      :attention-kind event-kind
                      :delivery delivery
                      :subject (event-subject event-kind thread-title)
                      :body (event-body event)
                      :subscription subscription}
                      (some? source-version)
                      (assoc :source-version source-version)))]
               (vswap! notification-count inc)
               (when (< (count @notification-id-sample)
                        max-replay-notification-id-sample)
                 (vswap! notification-id-sample conj notification))))))
        should-advance?
        (and (< cursor boundary)
             (or ended-at
                 (:saw-domain-record? stream-result)))]
    (when should-advance?
      (advance-cursor! port subscription cursor cursor-anchor boundary
                       boundary-anchor boundary-high-water))
    {:notification-count @notification-count
     :notification-ids @notification-id-sample}))

(defn replayable-subscription-ids [port principal]
  (let [principal (require-principal! port principal)
        ids
        (->> (north.coord/query-rows
              port
              {:find "subscription"
               :rules
               [{:head {:rel "subscription" :args [{:var "subscription"}]}
                 :body [{:rel "triple"
                         :args [{:var "subscription"} "kind" "subscription"]}
                        {:rel "triple"
                         :args [{:var "subscription"} "subscriber" principal]}]}]})
             (map first)
             distinct
             sort)]
    (->> ids
         (filter
          (fn [subscription]
            (let [{:keys [ended-at cursor-offset end-offset]}
                  (subscription-row port subscription)]
              (or (nil? ended-at)
                  (< (parse-position! "subscription cursor_offset" cursor-offset)
                     (parse-position! "subscription end_offset" end-offset))))))
         vec)))

(defn sync-principal! [port principal]
  (let [principal (require-principal! port principal)
        high-water (north.coord/cur-ver port)
        high-position (physical-log-position)
        subscriptions (replayable-subscription-ids port principal)
        replay
        (reduce
         (fn [{:keys [notification-count notification-ids]} subscription]
           (let [result
                 (sync-subscription!
                  port subscription high-position high-water)
                 remaining
                 (- max-replay-notification-id-sample
                    (count notification-ids))]
             {:notification-count
              (+ notification-count (:notification-count result))
              :notification-ids
              (if (pos? remaining)
                (into notification-ids
                      (take remaining (:notification-ids result)))
                notification-ids)}))
         {:notification-count 0 :notification-ids []}
         subscriptions)]
    {:principal principal
     :high-water high-water
     :high-offset (:offset high-position)
     :subscriptions subscriptions
     :notification-count (:notification-count replay)
     :notification-ids (:notification-ids replay)}))

(defn notification-ids [port principal include-read?]
  (let [principal (require-principal! port principal)
        rows
        (north.coord/query-rows
         port
         {:find "notification"
          :rules
          [{:head {:rel "notification" :args [{:var "notification"}]}
            :body [{:rel "triple"
                    :args [{:var "notification"} "kind" "notification"]}
                   {:rel "triple"
                    :args [{:var "notification"} "recipient" principal]}]}]})]
    (->> rows
         (map first)
         distinct
         (filter
          (fn [notification]
            (or include-read?
                (not (contains?
                      (set (north.coord/many port notification "read_by"))
                      principal)))))
         sort
         vec)))

(defn notification-row [port notification principal]
  {:id notification
   :from (north.coord/resolved port notification "from")
   :recipient (north.coord/resolved port notification "recipient")
   :about (north.coord/resolved port notification "about")
   :attention-kind (north.coord/resolved port notification "attention_kind")
   :delivery (north.coord/resolved port notification "delivery")
   :subject (north.coord/resolved port notification "subject")
   :body (north.coord/resolved port notification "body")
   :sent-at (north.coord/resolved port notification "sent_at")
   :source-version (north.coord/resolved port notification "source_version")
   :read? (contains? (set (north.coord/many port notification "read_by"))
                     principal)})

(defn mark-notification-read! [port notification principal]
  (let [result
        (north.coord/assert-batch-after-read!
         port notification
         (fn []
           (let [facts (exact-facts port notification)
                 recipient
                 (north.coord/resolved port notification "recipient")
                 kind (north.coord/resolved port notification "kind")
                 already-read? (contains? facts ["read_by" principal])
                 read-at (north.coord/resolved port notification "read_at")]
             (when-not (and (= "notification" kind)
                            (= principal recipient))
               (fail! "notification read acknowledgement changed ownership"
                      {:type :notification-read-ownership-conflict
                       :notification notification
                       :principal principal
                       :recipient recipient
                       :kind kind}))
             (if (and already-read? (not (str/blank? read-at)))
               {:done notification}
               {:facts
                (vec
                 (concat
                  (when-not already-read?
                    [{:p "read_by" :r principal}])
                  (when (str/blank? read-at)
                    [{:p "read_at"
                      :r (str (java.time.Instant/now))}])))}))))]
    (when-not (or (:ok result) (contains? result :done))
      (fail! "notification mark-read publication failed"
             {:type :notification-mark-read-publication-failed
              :notification notification
              :result result}))
    notification))

(defn notifications
  [port principal {:keys [include-read? mark-read?]}]
  (let [principal (require-principal! port principal)
        _ (sync-principal! port principal)
        ids (notification-ids port principal include-read?)
        rows (mapv #(notification-row port % principal) ids)]
    (when mark-read?
      (doseq [notification ids]
        (mark-notification-read! port notification principal)))
    rows))
