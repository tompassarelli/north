(ns north.message-routing
  "Send-time address resolution and liveness for durable North mail."
  (:require [clojure.string :as str]))

(def broadcast-address "*")
(def role-prefix "@role:")
(def agent-prefix "@agent:")
(def max-route-candidates 4096)
(def route-page-size 128)

(defn bare-agent [value]
  (when (string? value)
    (str/replace-first value #"^@agent:" "")))

(defn agent-subject [control]
  (str agent-prefix (bare-agent control)))

(defn recipient-live?
  "A direct recipient is reachable when its renewable session lease is live or
   its provider listener is explicitly armed."
  [port control]
  (let [control (bare-agent control)]
    (boolean
     (and (not (str/blank? control))
          (or (north.coord/online? port control)
              (= "armed"
                 (north.coord/resolved
                  port (agent-subject control) "live_input_state")))))))

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
