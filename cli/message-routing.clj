(ns north.message-routing
  "Send-time address resolution and liveness for durable North mail."
  (:require [clojure.string :as str]))

(def broadcast-address "*")
(def role-prefix "@role:")
(def agent-prefix "@agent:")
(def listener-lease-prefix "listener:")
(def listener-generation-pattern
  #"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
(def max-route-candidates 4096)
(def route-page-size 128)
(def max-mail-candidates 16384)
(def mail-page-size 512)

(defn bare-agent [value]
  (when (string? value)
    (str/replace-first value #"^@agent:" "")))

(defn agent-subject [control]
  (str agent-prefix (bare-agent control)))

(defn listener-resource [control]
  (str listener-lease-prefix (bare-agent control)))

(defn lease-live-at? [lease now]
  (and (north.coord/authoritative-lease? lease)
       (> (:exp lease) now)
       (pos? (:epoch lease))))

(defn exact-singleton?
  [envelope expected]
  (and (map? envelope)
       (= 1 (:members envelope))
       (false? (:ambiguous? envelope))
       (= [expected] (:values envelope))
       (= expected (:value envelope))))

(defn native-listener-live?
  "A native listener's durable armed bit is only descriptive. Reachability also
   requires the matching renewable generation lease. The before/after lease
   reads close release/reacquire races around the point reads without rejecting
   an ordinary renewal, whose holder stays generation-stable while its numeric
   fence epoch advances."
  [port control]
  (let [subject (agent-subject control)
        resource (listener-resource control)
        before (north.coord/lease-of port resource)]
    ;; Keep every load-bearing read explicit and ordered: lease -> generation
    ;; -> state -> lease. Arming writes frozen, generation, armed; cleanup
    ;; writes frozen before release. No interleaving can expose a false live
    ;; generation through both lease snapshots.
    (let [generation
          (north.coord/resolved-envelope
           port subject "live_input_epoch")
          state
          (north.coord/resolved-envelope
           port subject "live_input_state")
          after (north.coord/lease-of port resource)
          now (System/currentTimeMillis)]
      (boolean
       (and (lease-live-at? before now)
            (lease-live-at? after now)
            (= (:holder before) (:holder after))
            (re-matches listener-generation-pattern (:holder before))
            (exact-singleton? generation (:holder before))
            (exact-singleton? state "armed"))))))

(defn armed-route-live?
  [port control]
  (let [subject (agent-subject control)
        kind (north.coord/resolved port subject "kind")]
    (case kind
      "session" (native-listener-live? port control)
      ;; Managed route state is SDK-owned. `north listen` never mutates it and
      ;; mail keeps the existing managed route behavior.
      "lane" (exact-singleton?
              (north.coord/resolved-envelope
               port subject "live_input_state")
              "armed")
      false)))

(defn recipient-live?
  "A direct recipient is reachable when its renewable session lease is live or
   an authoritative listener route is explicitly armed."
  [port control]
  (let [control (bare-agent control)]
    (boolean
     (and (not (str/blank? control))
          (or (north.coord/online? port control)
              (armed-route-live? port control))))))

(defn role-holders
  "All graph holders of ROLE-SLUG. Ordering is deterministic; liveness decides
   which holder may receive new mail."
  [port role-slug]
  (let [response
        (north.coord/query-page
         port
         {:find "role_holder"
          :rules
          [{:head {:rel "role_holder" :args [{:var "agent"}]}
            :body [{:rel "triple"
                    :args [{:var "agent"} "holds"
                           (str role-prefix role-slug)]}]}]}
         route-page-size nil)]
    (when (:more response)
      (throw
       (ex-info "role holder projection exceeds its bounded page"
                {:type :role-holder-overflow
                 :role role-slug
                 :max route-page-size})))
    (->> (:ok response)
         (map first)
         (filter #(str/starts-with? % agent-prefix))
         (map bare-agent)
         distinct
         sort
         vec)))

(defn live-role-holder [port role-slug]
  (some #(when (recipient-live? port %) %)
        (role-holders port role-slug)))

(defn resolve-address
  "Resolve ADDRESS to one concrete session at send time. A durable @role target
   is authoritative. Existing held-role routing remains supported while old
   sessions converge onto durable aliases."
  [port address]
  (cond
    (= broadcast-address address)
    {:address address :recipient address :kind :broadcast}

    :else
    (let [role (str role-prefix address)
          durable (north.coord/resolved port role "target")]
      (cond
        (not (str/blank? durable))
        {:address address :recipient (bare-agent durable) :kind :alias}

        (recipient-live? port address)
        {:address address :recipient (bare-agent address) :kind :direct}

        :else
        (if-let [holder (live-role-holder port address)]
          {:address address :recipient holder :kind :held-role}
          {:address address :recipient (bare-agent address) :kind :direct})))))

(defn- route-candidate-query [repo role]
  {:find "same_route_agent"
   :rules
   [{:head {:rel "same_route_agent" :args [{:var "agent"}]}
     :body [{:rel "triple" :args [{:var "agent"} "repo" repo]}
            {:rel "triple" :args [{:var "agent"} "role" role]}]}]})

(defn live-same-route
  "Best-effort diagnostic successor for a dead recipient. Returns nil when the
   dead identity lacks repo/role provenance or the bounded lookup is unavailable."
  [port dead-control]
  (try
    (let [dead-control (bare-agent dead-control)
          subject (agent-subject dead-control)
          repo (north.coord/resolved port subject "repo")
          role (north.coord/resolved port subject "role")]
      (when (and (not (str/blank? repo)) (not (str/blank? role)))
        (loop [after nil
               seen 0]
          (when (< seen max-route-candidates)
            (let [page (north.coord/query-page
                        port (route-candidate-query repo role)
                        route-page-size after)
                  candidates
                  (->> (:ok page)
                       (map first)
                       (filter #(str/starts-with? % agent-prefix))
                       (map bare-agent)
                       (remove #(= dead-control %))
                       distinct
                       sort)]
              (or (some #(when (recipient-live? port %) %) candidates)
                  (when (:more page)
                    (recur (:next page) (+ seen (count (:ok page)))))))))))
    (catch Exception _ nil)))

(defn require-live-address
  "Return the concrete send-time route or a structured dead-recipient result.
   Read failures are distinguished from genuine negative liveness."
  [port address]
  (try
    (let [route (resolve-address port address)
          recipient (:recipient route)]
      (cond
        (= :broadcast (:kind route)) (assoc route :live true)
        (recipient-live? port recipient) (assoc route :live true)
        :else (assoc route
                     :live false
                     :alternative (live-same-route port recipient))))
    (catch Exception error
      {:address address :live :unavailable :error error})))

(defn- mail-candidate-query []
  {:find "mail_candidate"
   :strata
   [[{:head {:rel "mail_settled" :args [{:var "message"}]}
      :body [{:rel "triple"
              :args [{:var "message"} "acked_by" {:var "recipient"}]}]}
     {:head {:rel "mail_settled" :args [{:var "message"}]}
      :body [{:rel "triple"
              :args [{:var "message"} "delivery_rejected_by"
                     {:var "recipient"}]}]}]
    [{:head {:rel "mail_candidate"
             :args [{:var "message"} {:var "from"} {:var "to"} {:var "sent"}]}
      :body [{:rel "triple" :args [{:var "message"} "from" {:var "from"}]}
             {:rel "triple" :args [{:var "message"} "to" {:var "to"}]}
             {:rel "triple" :args [{:var "message"} "sent_at" {:var "sent"}]}
             {:rel "mail_settled"
              :args [{:var "message"}]
              :neg true}]}]]})

(defn mail-candidates [port]
  (loop [after nil
         seen 0
         rows []]
    (let [page (north.coord/query-page
                port (mail-candidate-query) mail-page-size after)
          next-seen (+ seen (count (:ok page)))]
      (when (> next-seen max-mail-candidates)
        (throw
         (ex-info "dead-letter scan exceeds its bounded mail corpus"
                  {:type :mail-candidate-overflow
                   :max max-mail-candidates})))
      (let [all (into rows (:ok page))]
        (if (:more page)
          (recur (:next page) next-seen all)
          all)))))

(defn age-ms [now sent]
  (try
    (max 0 (- (.toEpochMilli ^java.time.Instant now)
              (.toEpochMilli (java.time.Instant/parse sent))))
    (catch Exception _ nil)))

(defn human-age [milliseconds]
  (if (nil? milliseconds)
    "unknown"
    (let [seconds (quot milliseconds 1000)]
      (cond
        (< seconds 60) (str seconds "s")
        (< seconds 3600) (str (quot seconds 60) "m")
        (< seconds 86400) (str (quot seconds 3600) "h")
        :else (str (quot seconds 86400) "d")))))

(defn dead-letter-scan
  "Pending human mail whose concrete recipient has neither a live lease nor an
   armed provider listener. Returns a strict diagnostic envelope."
  ([port] (dead-letter-scan port (java.time.Instant/now)))
  ([port now]
   (try
     {:rows
      (->> (mail-candidates port)
           (keep
            (fn [[message from to sent]]
              (when (and (str/starts-with? message "@msg:")
                         (not= broadcast-address to))
                (let [route (resolve-address port to)
                      recipient (:recipient route)]
                  (when-not (recipient-live? port recipient)
                    (let [milliseconds (age-ms now sent)]
                      {:message message
                       :sender from
                       :recipient to
                       :resolved-recipient recipient
                       :age-ms milliseconds
                       :age (human-age milliseconds)}))))))
           (sort-by (juxt #(or (:age-ms %) -1) :message)
                    #(compare %2 %1))
           vec)}
     (catch Exception error
       {:error (.getMessage error)}))))

(defn readiness-dead-letter-scan
  "Bounded doctor projection. The query itself excludes acknowledged/rejected
   mail. Only messages inside ACTION-WINDOW-MS pay recipient routing/liveness
   reads; older pending mail remains visible as unacknowledged history without
   an N×recipient scan that can hold the health command for tens of seconds."
  ([port action-window-ms]
   (readiness-dead-letter-scan
    port action-window-ms (java.time.Instant/now)))
  ([port action-window-ms now]
   (try
     {:rows
      (->> (mail-candidates port)
           (keep
            (fn [[message from to sent]]
              (when (and (str/starts-with? message "@msg:")
                         (not= broadcast-address to))
                (let [milliseconds (age-ms now sent)
                      base {:message message
                            :sender from
                            :recipient to
                            :age-ms milliseconds
                            :age (human-age milliseconds)}]
                  (if (and milliseconds
                           (>= milliseconds action-window-ms))
                    (assoc base
                           :resolved-recipient to
                           :historical? true)
                    (let [route (resolve-address port to)
                          recipient (:recipient route)]
                      (when-not (recipient-live? port recipient)
                        (assoc base :resolved-recipient recipient))))))))
           (sort-by (juxt #(or (:age-ms %) -1) :message)
                    #(compare %2 %1))
           vec)}
     (catch Exception error
       {:error (.getMessage error)}))))
