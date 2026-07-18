;; Shared human-message audience semantics.
;;
;; Direct mail is addressed by its `to` fact. A broadcast keeps `to="*"` as the
;; subscription trigger, but authority to deliver comes only from the finite
;; `broadcast_to` facts snapshotted before that trigger lands. An audience-less
;; historical wildcard is therefore inert: no future session can receive or ack
;; it, and no time cutoff is needed.
(ns north.message-audience
  (:require [clojure.set :as set]
            [clojure.string :as str]
            [north.coord :as coord]))

(def broadcast-address "*")
(def audience-predicate "broadcast_to")
(def audience-version-predicate "broadcast_audience_version")
(def audience-version "snapshot-v1")
(def lease-session-prefix "@lease:session:")
(def delivery-claim-ttl-ms 30000)

(defn bare-handle [handle]
  (-> (str handle)
      (str/replace-first #"^@agent:" "")
      (str/replace-first #"^@session:" "")))

(defn online-handles
  "Finite session audience at one coordinator observation. Liveness uses the
   same unexpired renewable-lease rule as the presence roster."
  [port now]
  (let [rows (:ok (coord/send-op
                   port
                   {:op :query
                    :query {:find "lease"
                            :rules [{:head {:rel "lease"
                                            :args [{:var "e"} {:var "v"}]}
                                     :body [{:rel "triple"
                                             :args [{:var "e"} "lease" {:var "v"}]}]}]}}))]
    (into (sorted-set)
          (keep (fn [[entity value]]
                  (let [entity (str entity)
                        lease (coord/decode-lease value)]
                    (when (and (str/starts-with? entity lease-session-prefix)
                               lease
                               (> (:exp lease) now))
                      (subs entity (count lease-session-prefix))))))
          (or rows []))))

(defn snapshot-broadcast!
  "Persist a finite audience before the wildcard `to` fact, excluding the sender. The caller
   must publish `to` last so subscribers cannot observe a partial snapshot."
  [port message from]
  (let [sender (bare-handle from)
        recipients (disj (online-handles port (System/currentTimeMillis)) sender)]
    ;; Literal predicate names keep the executable writer visible to North's
    ;; static predicate-registry parity audit.
    (when (:reject (coord/append! port message "broadcast_audience_version" audience-version))
      (throw (ex-info "broadcast audience version write rejected"
                      {:type :broadcast-audience-write-rejected :message message})))
    (doseq [recipient recipients]
      (when (:reject (coord/append! port message "broadcast_to" recipient))
        (throw (ex-info "broadcast audience member write rejected"
                        {:type :broadcast-audience-write-rejected
                         :message message :recipient recipient}))))
    ;; Read-back is the commit barrier before the caller publishes `to="*"`.
    ;; A crash or rejection before this point leaves an inert, unaddressed draft.
    (let [observed-version (coord/resolved port message audience-version-predicate)
          observed-recipients (set (coord/many port message audience-predicate))]
      (when-not (and (= audience-version observed-version)
                     (= (set recipients) observed-recipients))
        (throw (ex-info "broadcast audience read-back mismatch"
                        {:type :broadcast-audience-readback-mismatch
                         :message message
                         :expected-version audience-version
                         :observed-version observed-version
                         :expected-recipients (set recipients)
                         :observed-recipients observed-recipients}))))
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

(defn release-delivery-claim! [port {:keys [resource holder]}]
  ;; Ack is already durable when normal completion releases. A transient release
  ;; failure must not turn a successful PostToolUse delivery into a hook failure;
  ;; the lease expires and can then be reclaimed.
  (try
    (coord/send-op port {:op :release-lease :res resource :holder holder})
    (catch Exception _ nil)))

(defn claim-delivery!
  "Atomically elect one live consumer for MESSAGE/RECIPIENT. A short coordinator
   lease closes the listener-vs-hook query/ack race. It is released after ack;
   if the winner dies first, expiry restores at-least-once delivery. Therefore
   concurrent healthy consumers print once, while a crash after print but before
   ack may still replay—the honest non-transactional-output boundary."
  [port message recipient]
  (let [recipient (bare-handle recipient)]
    (when-not (acknowledged? port message recipient)
      (let [resource (delivery-claim-resource message recipient)
            holder (str "message-consumer:" recipient ":" (java.util.UUID/randomUUID))
            result (coord/send-op
                    port
                    {:op :acquire-lease :res resource :holder holder
                     :ttl-ms delivery-claim-ttl-ms})]
        (when (:ok result)
          (let [claim {:resource resource :holder holder :epoch (:epoch result)}]
            ;; A manual ack may have landed between the initial read and acquire.
            (if (acknowledged? port message recipient)
              (do (release-delivery-claim! port claim) nil)
              claim)))))))

(defn complete-delivery!
  "Commit the durable ack after output has been flushed, then release CLAIM."
  [port message recipient claim]
  (try
    (let [recipient (bare-handle recipient)
          result (coord/append! port message "acked_by" recipient)]
      (when (:reject result)
        (throw (ex-info "message acknowledgement rejected"
                        {:type :message-ack-rejected
                         :message message :recipient recipient})))
      ;; Timestamp is diagnostic; acked_by is the durable delivery marker.
      (try (coord/put! port message "acked_at" (str (java.time.Instant/now)))
           (catch Exception _ nil))
      (when-not (acknowledged? port message recipient)
        (throw (ex-info "message acknowledgement read-back mismatch"
                        {:type :message-ack-readback-mismatch
                         :message message :recipient recipient})))
      true)
    (finally
      (release-delivery-claim! port claim))))

(defn- message-ids-matching [port body]
  (set
   (map first
        (or
         (:ok
          (coord/send-op
           port
           {:op :query
            :query {:find "matching_message"
                    :rules [{:head {:rel "matching_message"
                                    :args [{:var "e"}]}
                             :body body}]}}))
         []))))

(defn pending-message-ids
  "Resolve unacked mail using indexed predicate/object probes, bounded by the
   recipient's address count—not by historical message or wildcard count. A
   concurrent ack between probes is harmless because claim-delivery! rechecks it
   under the atomic delivery lease before any output."
  [port recipient direct-addresses]
  (let [recipient (bare-handle recipient)
        direct
        (reduce set/union #{}
                (map (fn [address]
                       (message-ids-matching
                        port [{:rel "triple"
                               :args [{:var "e"} "to" address]}]))
                     (set direct-addresses)))
        broadcast
        (message-ids-matching
         port [{:rel "triple"
                :args [{:var "e"} "broadcast_to" recipient]}
               {:rel "triple"
                :args [{:var "e"} "to" broadcast-address]}])
        acknowledged
        (message-ids-matching
         port [{:rel "triple"
                :args [{:var "e"} "acked_by" recipient]}])]
    (vec (set/difference (set/union direct broadcast) acknowledged))))

(defn deliverable?
  "Whether RECIPIENT may consume MESSAGE addressed TO. DIRECT-ADDRESSES contains
   the recipient's own handle plus any roles it currently holds. Broadcasts
   deliberately consult only the snapshotted concrete recipient handle."
  [port message to recipient direct-addresses]
  (if (= broadcast-address to)
    (contains? (audience port message) (bare-handle recipient))
    (contains? (set direct-addresses) to)))
