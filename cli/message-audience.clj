;; Shared human-message audience semantics.
;;
;; Direct mail is addressed by its `to` fact. A broadcast keeps `to="*"` as the
;; subscription trigger, but authority to deliver comes only from the finite
;; `broadcast_to` facts snapshotted before that trigger lands. An audience-less
;; historical wildcard is therefore inert: no future session can receive or ack
;; it, and no time cutoff is needed.
(ns north.message-audience
  (:require [cheshire.core :as json]
            [clojure.set :as set]
            [clojure.string :as str]
            [north.coord :as coord]))

(def broadcast-address "*")
(def audience-predicate "broadcast_to")
(def audience-version-predicate "broadcast_audience_version")
(def audience-version "snapshot-v1")
(def delivery-claim-ttl-ms 30000)
(def rejection-predicate "delivery_rejection")
(def rejected-by-predicate "delivery_rejected_by")
(def msg-manifest-predicate "target_identity_manifest_sha256")
(def rejection-reasons
  #{"invalid_message_id" "missing_sender" "invalid_sender" "sender_too_large"
    "missing_subject" "invalid_subject" "subject_too_large"
    "missing_body" "invalid_body" "body_too_large"
    "message_frame_too_large" "msg_manifest_missing"
    "msg_type_invalid" "msg_route_invalid" "msg_route_stale"
    "msg_route_not_armed"})
(def max-rejection-recipient-bytes 512)
(def max-direct-addresses 256)
(def max-direct-address-bytes 512)
(def pending-page-limit 256)
(def manifest-sha256-bytes 64)
(def max-message-id-bytes 512)
(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))
(def max-rejection-evidence-bytes
  ;; Exact maximum canonical JSON encoding: a max-size safe direct address, the
  ;; longest closed-set reason, and both fixed-width SHA-256 route manifests.
  ;; Safe handles and every other field are ASCII, so JSON escaping adds no
  ;; value-dependent expansion.
  (utf8-bytes
   (json/generate-string
    (sorted-map
     "expectedManifest" (apply str (repeat manifest-sha256-bytes "a"))
     "observedManifest" (apply str (repeat manifest-sha256-bytes "b"))
     "reason" (apply max-key utf8-bytes rejection-reasons)
     "recipient" (apply str (repeat max-rejection-recipient-bytes "r"))))))

(defn bare-handle [handle]
  (-> (str handle)
      (str/replace-first #"^@agent:" "")
      (str/replace-first #"^@session:" "")))

(defn canonical-message-id?
  "Only canonical @msg subjects enter human-mail consumers. Routing predicates
   are shared by other coordination entities, so `to` alone is never proof that
   a subject is mail."
  [value]
  (and (string? value)
       (<= (utf8-bytes value) max-message-id-bytes)
       (boolean
        (re-matches #"^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn complete-message-envelope?
  "A canonical subject prefix is necessary but not sufficient: require the
   complete mail envelope that every production publisher writes before
   its routing edge."
  [port message]
  (and
   (canonical-message-id? message)
   (every?
    string?
    (map #(coord/resolved port message %)
         ["from" "subject" "body" "sent_at"]))))

(defn online-handles
  "Finite session audience at one database observation. Liveness uses the
   same unexpired renewable-lease rule as the presence roster."
  [port now]
  (:handles (coord/online-session-handles port now)))

(defn snapshot-broadcast!
  "Persist a finite audience before the wildcard `to` fact, excluding the sender. The caller
   must publish `to` last so subscribers cannot observe a partial snapshot."
  [port message from]
  (let [sender (bare-handle from)
        recipients (disj (online-handles port (System/currentTimeMillis)) sender)
        result (coord/publish!
                port
                [{:op :set :subject message
                  :predicate "broadcast_audience_version"
                  :values [audience-version] :cardinality :one}
                 {:op :set :subject message
                  :predicate "broadcast_to"
                  :values (vec recipients) :cardinality :many}])]
    (when (:reject result)
      (throw (ex-info "broadcast audience publication rejected"
                      {:type :broadcast-audience-write-rejected
                       :message message :result result})))
    recipients))

(defn audience [port message]
  (set (coord/many port message audience-predicate)))

(defn- sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256")
                        (.getBytes (str value) "UTF-8"))]
    (apply str (map #(format "%02x" (bit-and (int %) 0xff)) digest))))

(defn delivery-claim-resource [message recipient]
  (str "message-delivery:"
       (sha256 (str message "\u0000" (bare-handle recipient)))))

(defn acknowledged? [port message recipient]
  (contains? (set (coord/many port message "acked_by"))
             (bare-handle recipient)))

(defn rejected? [port message recipient]
  (contains? (set (coord/many port message rejected-by-predicate))
             (bare-handle recipient)))

(defn release-delivery-claim! [port {:keys [resource holder epoch]}]
  ;; Ack is already durable when normal completion releases. A transient release
  ;; failure must not turn a successful PostToolUse delivery into a hook failure;
  ;; the lease expires and can then be reclaimed.
  (try
    (coord/release-lease!
     port (coord/lease-fence resource holder epoch))
    (catch Exception _ nil)))

(defn claim-delivery!
  "Atomically elect one live consumer for MESSAGE/RECIPIENT. A short database
   lease closes the listener-vs-hook query/ack race. It is released after ack;
   if the winner dies first, expiry restores at-least-once delivery. Therefore
   concurrent healthy consumers print once, while a crash after print but before
   ack may still replay—the honest non-transactional-output boundary."
  ([port message recipient]
   (claim-delivery! port message recipient delivery-claim-ttl-ms))
  ([port message recipient ttl-ms]
   (when-not (and (integer? ttl-ms)
                  (pos? ttl-ms)
                  (<= ttl-ms delivery-claim-ttl-ms))
     (throw (ex-info "delivery claim TTL is outside the supported bound"
                     {:type :invalid-delivery-claim-ttl
                      :ttl-ms ttl-ms
                      :max-ttl-ms delivery-claim-ttl-ms})))
   (let [recipient (bare-handle recipient)]
     (when-not (or (acknowledged? port message recipient)
                   (rejected? port message recipient))
       (let [resource (delivery-claim-resource message recipient)
             holder (str "message-consumer:" recipient ":" (java.util.UUID/randomUUID))
             result (coord/acquire-lease! port resource holder ttl-ms)]
         (when (:ok result)
           (let [claim (select-keys result [:resource :holder :epoch])]
             ;; A manual ack may have landed between the initial read and acquire.
             (if (or (acknowledged? port message recipient)
                     (rejected? port message recipient))
               (do (release-delivery-claim! port claim) nil)
               claim))))))))

(defn complete-delivery!
  "Commit the durable ack after output has been flushed, then release CLAIM."
  [port message recipient claim]
  (try
    (let [recipient (bare-handle recipient)
          result (coord/publish!
                  port
                  [{:op :assert :subject message :predicate "acked_by"
                    :value recipient :cardinality :many}
                   {:op :assert :subject message :predicate "acked_at"
                    :value (str (java.time.Instant/now)) :cardinality :one}])]
      (when (:reject result)
        (throw (ex-info "message acknowledgement rejected"
                        {:type :message-ack-rejected
                         :message message :recipient recipient})))
      (when-not (acknowledged? port message recipient)
        (throw (ex-info "message acknowledgement read-back mismatch"
                        {:type :message-ack-readback-mismatch
                         :message message :recipient recipient})))
      true)
    (finally
      (release-delivery-claim! port claim))))

(defn reject-delivery!
  "Terminally settle one permanently impossible recipient delivery without
   claiming successful output. Evidence lands first; delivery_rejected_by is
   the durable settlement marker that removes it from pending replay."
  [port message recipient claim
   {:keys [reason expected-manifest observed-manifest]}]
  (try
    (let [recipient (bare-handle recipient)]
      (when-not (and (<= (utf8-bytes recipient)
                         max-rejection-recipient-bytes)
                     (boolean
                      (re-matches #"^[A-Za-z0-9][A-Za-z0-9._:-]*$"
                                  recipient)))
        (throw (ex-info "message rejection recipient is malformed"
                        {:type :invalid-message-rejection})))
      (when-not (contains? rejection-reasons reason)
        (throw (ex-info "unsupported message rejection reason"
                        {:type :invalid-message-rejection :reason reason})))
      (doseq [[label value] [["expected manifest" expected-manifest]
                             ["observed manifest" observed-manifest]]
              :when value]
        (when-not (and (string? value)
                       (re-matches #"^[0-9a-f]{64}$" value))
          (throw (ex-info (str label " is malformed")
                          {:type :invalid-message-rejection
                           :field label}))))
      (let [evidence
            (json/generate-string
             (cond-> (sorted-map
                      "reason" reason
                      "recipient" recipient)
               expected-manifest
               (assoc "expectedManifest" expected-manifest)
               observed-manifest
               (assoc "observedManifest" observed-manifest)))]
        (when (> (utf8-bytes evidence) max-rejection-evidence-bytes)
          (throw (ex-info "message rejection evidence exceeds its byte bound"
                          {:type :invalid-message-rejection})))
        (let [result
              (coord/publish!
               port
               [{:op :assert :subject message :predicate rejection-predicate
                 :value evidence :cardinality :many}
                {:op :assert :subject message :predicate rejected-by-predicate
                 :value recipient :cardinality :many}])]
          (when (:reject result)
            (throw (ex-info "message rejection publication was rejected"
                            {:type :message-rejection-write-rejected
                             :message message :recipient recipient
                             :result result}))))
        (when-not (and (rejected? port message recipient)
                       (contains? (set (coord/many port message
                                                  rejection-predicate))
                                  evidence))
          (throw (ex-info "message rejection read-back mismatch"
                          {:type :message-rejection-readback-mismatch
                           :message message :recipient recipient})))
        true))
    (finally
      (release-delivery-claim! port claim))))

(defn- safe-direct-address? [address]
  (and (string? address)
       (<= (utf8-bytes address) max-direct-address-bytes)
       (boolean
        (re-matches #"^[A-Za-z0-9][A-Za-z0-9._:-]*$" address))))

(defn bounded-direct-addresses
  "Validate and deduplicate the finite direct audience without first
   materializing an attacker-sized role collection."
  [recipient direct-addresses]
  (let [recipient (bare-handle recipient)]
    (when-not (safe-direct-address? recipient)
      (throw (ex-info "message recipient is malformed"
                      {:type :invalid-message-recipient})))
    (loop [remaining (seq direct-addresses)
           addresses #{recipient}
           scanned 0]
      (if (nil? remaining)
        (vec (sort addresses))
        (let [address (first remaining)]
          (when (>= scanned max-direct-addresses)
            (throw (ex-info "direct message address input exceeds its bound"
                            {:type :direct-address-limit-exceeded
                             :max max-direct-addresses})))
          (when-not (safe-direct-address? address)
            (throw (ex-info "direct message address is malformed"
                            {:type :invalid-direct-address
                             :address address})))
          (let [next-addresses (conj addresses address)]
            (when (> (count next-addresses) max-direct-addresses)
              (throw (ex-info "direct message address set exceeds its bound"
                              {:type :direct-address-limit-exceeded
                               :max max-direct-addresses})))
            (recur (next remaining) next-addresses (inc scanned))))))))

(defn pending-query
  "One stratified program for direct + broadcast candidates minus durable ack
   and rejection settlement. First-party attention entities are excluded before
   bounded pagination, while malformed canonical mail remains visible for
   terminal rejection. Dynamic direct-address rules are strictly bounded before
   this data structure exists."
  [recipient direct-addresses]
  (let [recipient (bare-handle recipient)
        addresses (bounded-direct-addresses recipient direct-addresses)
        direct-rules
        (mapv
         (fn [address]
           {:head {:rel "message_candidate" :args [{:var "e"}]}
            :body [{:rel "triple"
                    :args [{:var "e"} "to" address]}]})
         addresses)
        base-rules
        (into
         direct-rules
         [{:head {:rel "message_candidate" :args [{:var "e"}]}
           :body [{:rel "triple"
                   :args [{:var "e"} "broadcast_to" recipient]}
                  {:rel "triple"
                   :args [{:var "e"} "to" broadcast-address]}]}
          {:head {:rel "attention_entity" :args [{:var "e"}]}
           :body [{:rel "triple"
                   :args [{:var "e"} "kind" "notification"]}]}
          {:head {:rel "attention_entity" :args [{:var "e"}]}
           :body [{:rel "triple"
                   :args [{:var "e"} "kind" "subscription"]}]}
          {:head {:rel "message_acknowledged" :args [{:var "e"}]}
           :body [{:rel "triple"
                   :args [{:var "e"} "acked_by" recipient]}]}
          {:head {:rel "message_rejected" :args [{:var "e"}]}
           :body [{:rel "triple"
                   :args [{:var "e"} rejected-by-predicate recipient]}]}])]
    {:find "pending_message"
     :strata
     [base-rules
      [{:head {:rel "pending_message" :args [{:var "e"}]}
        :body [{:rel "message_candidate" :args [{:var "e"}]}
               {:rel "attention_entity"
                :args [{:var "e"}] :neg true}
               {:rel "message_acknowledged"
                :args [{:var "e"}] :neg true}
               {:rel "message_rejected"
                :args [{:var "e"}] :neg true}]}]]}))

(defn pending-msg-query
  "The pending relation restricted to messages admitted by the managed msg
   producer. The immutable route-manifest fact is the producer's durable type
   marker; filtering on it avoids terminal teardown being blocked by ordinary
   inbox mail."
  [recipient direct-addresses]
  (update-in
   (pending-query recipient direct-addresses)
   [:strata 1 0 :body]
   conj
   {:rel "triple"
    :args [{:var "e"} msg-manifest-predicate {:var "manifest"}]}))

(defn pending-message-page
  "Read one bounded deterministic pending page. AFTER is a Beagle Store cursor for
   stable read-only consumers; delivery replay intentionally restarts at nil
   after settling each page."
  ([port recipient direct-addresses]
   (pending-message-page
    port recipient direct-addresses pending-page-limit nil))
  ([port recipient direct-addresses limit after]
   (let [response
         (coord/query-page
          port (pending-query recipient direct-addresses) limit after)]
     (when-not (and (<= (count (:rows response)) limit)
                    (every? #(and (vector? %)
                                  (= 1 (count %))
                                  (string? (first %)))
                            (:rows response)))
       (throw (ex-info "pending message page has malformed rows"
                       {:type :malformed-pending-message-page})))
     (let [rows (->> (:rows response)
                     (filter #(canonical-message-id? (first %)))
                     vec)]
       ;; Preserve Beagle Store's cursor/version exactly. Filtering only the returned
       ;; relation keeps any other routed coordination subjects out of
       ;; hook/live-feed consumers without inventing a client-derived cursor.
       (assoc response :rows rows :messages (mapv first rows))))))

(defn pending-msg-page
  "Read one bounded deterministic page of unsettled managed msg messages."
  ([port recipient direct-addresses]
   (pending-msg-page
    port recipient direct-addresses pending-page-limit nil))
  ([port recipient direct-addresses limit after]
   (let [response
         (coord/query-page
          port (pending-msg-query recipient direct-addresses) limit after)]
     (when-not (and (<= (count (:rows response)) limit)
                    (every? #(and (vector? %)
                                  (= 1 (count %))
                                  (string? (first %)))
                            (:rows response)))
       (throw (ex-info "pending msg page has malformed rows"
                       {:type :malformed-pending-msg-page})))
     (let [rows (->> (:rows response)
                     (filter #(canonical-message-id? (first %)))
                     vec)]
       (assoc response :rows rows :messages (mapv first rows))))))

(defn- recipient-keyed-ids
  "Message ids from one bounded positive-triple query. Keeping negation in the
   client-side set difference preserves the indexed join shape and bounds every
   server response."
  [port body]
  (let [{:keys [rows]}
        (coord/bounded-query
         port
         {:find "pending_candidate"
          :rules [{:head {:rel "pending_candidate" :args [{:var "e"}]}
                   :body body}]}
         coord/query-page-row-limit)]
    (into #{} (map first) rows)))

(defn pending-message-ids
  "All pending ids for human/read-only callers (the `msg inbox` view). Same set as
   `pending-query`: direct + broadcast-audience candidates, minus durable ack and
   rejection settlement — but computed from recipient-keyed index lookups so it
   returns in O(recipient's mail), never the whole-corpus scan that stratified
   negation forces. Live replay uses pending-message-page directly, not this vector."
  [port recipient direct-addresses]
  (let [recipient (bare-handle recipient)
        addresses (bounded-direct-addresses recipient direct-addresses)
        direct (reduce
                (fn [acc address]
                  (into acc (recipient-keyed-ids
                             port [{:rel "triple" :args [{:var "e"} "to" address]}])))
                #{} addresses)
        broadcast (recipient-keyed-ids
                   port [{:rel "triple" :args [{:var "e"} audience-predicate recipient]}
                         {:rel "triple" :args [{:var "e"} "to" broadcast-address]}])
        acknowledged (recipient-keyed-ids
                      port [{:rel "triple" :args [{:var "e"} "acked_by" recipient]}])
        rejected (recipient-keyed-ids
                  port [{:rel "triple" :args [{:var "e"} rejected-by-predicate recipient]}])]
    (->> (set/difference (set/union direct broadcast)
                         (set/union acknowledged rejected))
         (filter #(complete-message-envelope? port %))
         sort
         vec)))

(defn deliverable?
  "Whether RECIPIENT may consume MESSAGE addressed TO. DIRECT-ADDRESSES contains
   the recipient's own handle plus any roles it currently holds. Broadcasts
   deliberately consult only the snapshotted concrete recipient handle."
  [port message to recipient direct-addresses]
  (and
   (canonical-message-id? message)
   (if (= broadcast-address to)
     (contains? (audience port message) (bare-handle recipient))
     (contains? (set direct-addresses) to))))
