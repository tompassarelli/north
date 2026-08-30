(ns north.attention
  "Deterministic concern notifications."
  (:require [clojure.java.io :as io]
            [clojure.set :as set]
            [clojure.string :as str]))

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-id.clj"))
(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/message-routing.clj"))

(def delivery-values #{"inbox" "notify"})
(def max-event-key-bytes 4096)
(def max-address-bytes 1024)
(def max-reference-bytes 4096)
(def max-attention-kind-bytes 256)
(def max-notification-subject-bytes 4096)
(def max-notification-body-bytes 65536)
(def notification-mutable-predicates #{"read_by" "read_at"})

(defn utf8-bytes [value]
  (alength (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8)))

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

(defn canonical-tuple [values]
  (pr-str (vec values)))

(defn notification-subject [event-key recipient]
  (str "@notification:"
       (north.message-id/sha256
        (str "north-attention-notification-v3\u0000"
             (canonical-tuple [event-key recipient])))))

(defn exact-facts [port subject]
  (->> (north.coord/query-rows!
        port
        {:find "attention_fact"
         :rules
         [{:head {:rel "attention_fact" :args [{:var "p"} {:var "r"}]}
           :body [{:rel "triple" :args [subject {:var "p"} {:var "r"}]}]}]})
       (mapv (fn [[predicate value]] [predicate value]))
       set))

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
  "Map a recipient reference to its literal listener address."
  [principal]
  (let [principal (normalize-ref principal)
        role-address (north.message-routing/bare-role principal)
        address
        (cond
          (str/starts-with? principal north.message-routing/agent-prefix)
          (north.message-routing/bare-agent principal)

          role-address role-address

          :else (subs principal 1))]
    (when-not (valid-address? address)
      (fail! "attention recipient has no valid listener address"
             {:type :invalid-attention-address :principal principal}))
    address))

(defn require-delivery! [delivery]
  (when-not (contains? delivery-values delivery)
    (fail! "attention delivery must be inbox or notify"
           {:type :invalid-attention-delivery :delivery delivery}))
  delivery)

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
  "Publish one deterministic notification."
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
