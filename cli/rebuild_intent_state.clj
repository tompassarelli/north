(ns north.rebuild-intent-state
  (:require [clojure.string :as str]))

(def protocol-version "north:rebuild-intent:v1")
(def default-hold-seconds 180)
(def default-max-delay-seconds 900)

(defn parse-duration-seconds [value]
  (let [[_ amount unit] (re-matches #"(?i)^([1-9][0-9]*)(s|m|h)?$" (str value))
        multiplier (case (some-> unit str/lower-case)
                     "m" 60
                     "h" 3600
                     1)]
    (when-not amount
      (throw (ex-info "duration must be a positive integer with optional s, m, or h suffix"
                      {:type :invalid-duration :value value})))
    (let [seconds (* (parse-long amount) multiplier)]
      (when (> seconds 86400)
        (throw (ex-info "duration must not exceed 24 hours"
                        {:type :invalid-duration :value value})))
      seconds)))

(defn instant->millis [value]
  (.toEpochMilli (java.time.Instant/parse value)))

(defn millis->instant [value]
  (str (java.time.Instant/ofEpochMilli (long value))))

(defn new-intent
  [{:keys [id who why planned-window created-at-ms hold-seconds max-delay-seconds]
    :or {hold-seconds default-hold-seconds
         max-delay-seconds default-max-delay-seconds}}]
  (when-not (and (string? id) (not (str/blank? id)))
    (throw (ex-info "rebuild intent requires an id" {:type :invalid-intent})))
  (when-not (and (string? who) (not (str/blank? who)))
    (throw (ex-info "rebuild intent requires who" {:type :invalid-intent})))
  (when-not (and (string? why) (not (str/blank? why)))
    (throw (ex-info "rebuild intent requires why" {:type :invalid-intent})))
  (when-not (and (string? planned-window) (not (str/blank? planned-window)))
    (throw (ex-info "rebuild intent requires a planned window" {:type :invalid-intent})))
  (when-not (and (integer? created-at-ms) (not (neg? created-at-ms)))
    (throw (ex-info "rebuild intent requires a non-negative creation time"
                    {:type :invalid-intent})))
  (when-not (and (integer? hold-seconds) (pos? hold-seconds))
    (throw (ex-info "hold window must be positive" {:type :invalid-intent})))
  (when-not (and (integer? max-delay-seconds)
                 (>= max-delay-seconds hold-seconds))
    (throw (ex-info "maximum delay must be at least the initial hold window"
                    {:type :invalid-intent})))
  {:version protocol-version
   :id id
   :who who
   :why why
   :planned-window planned-window
   :created-at-ms created-at-ms
   :hold-seconds hold-seconds
   :max-delay-seconds max-delay-seconds
   :deadline-ms (+ created-at-ms (* 1000 hold-seconds))
   :max-deadline-ms (+ created-at-ms (* 1000 max-delay-seconds))
   :responses []
   :phase :holding})

(defn holding? [state]
  (= :holding (:phase state)))

(defn response-open? [state now-ms]
  (and (holding? state) (< now-ms (:deadline-ms state))))

(defn apply-response
  [state {:keys [type from what reason eta-seconds received-at-ms] :as response}]
  (when-not (response-open? state received-at-ms)
    (throw (ex-info "rebuild intent response window is closed"
                    {:type :response-window-closed
                     :phase (:phase state)
                     :deadline-ms (:deadline-ms state)
                     :received-at-ms received-at-ms})))
  (when-not (and (string? from) (not (str/blank? from)))
    (throw (ex-info "rebuild response requires a sender"
                    {:type :invalid-response})))
  (case type
    :batch
    (when-not (and (string? what) (not (str/blank? what)))
      (throw (ex-info "batch-with-me requires a pending change"
                      {:type :invalid-response})))

    :hold
    (do
      (when-not (and (string? reason) (not (str/blank? reason)))
        (throw (ex-info "hold requires a reason" {:type :invalid-response})))
      (when-not (and (integer? eta-seconds) (pos? eta-seconds))
        (throw (ex-info "hold requires a positive ETA" {:type :invalid-response}))))

    (throw (ex-info "unknown rebuild response type"
                    {:type :invalid-response :response-type type})))
  (let [record (select-keys response
                            [:event-id :type :from :what :reason
                             :eta-seconds :received-at-ms])
        requested-deadline (if (= :hold type)
                             (+ received-at-ms (* 1000 eta-seconds))
                             (:deadline-ms state))
        next-deadline (min (:max-deadline-ms state)
                           (max (:deadline-ms state) requested-deadline))]
    (-> state
        (update :responses conj record)
        (assoc :deadline-ms next-deadline))))

(defn advance [state now-ms]
  (if (and (holding? state) (>= now-ms (:deadline-ms state)))
    (assoc state :phase :all-clear :all-clear-at-ms now-ms)
    state))

(defn mark-rebuild-started [state now-ms]
  (when-not (= :all-clear (:phase state))
    (throw (ex-info "rebuild may start only after all-clear"
                    {:type :invalid-transition :phase (:phase state)})))
  (assoc state :phase :rebuilding :rebuild-started-at-ms now-ms))

(defn mark-deployment-verified [state now-ms report]
  (when-not (= :rebuilding (:phase state))
    (throw (ex-info "deployment verification requires an in-progress rebuild"
                    {:type :invalid-transition :phase (:phase state)})))
  (when (str/blank? (str report))
    (throw (ex-info "deployment verification requires a report"
                    {:type :invalid-transition})))
  (assoc state
         :phase :deployment-verified
         :deployment-verified-at-ms now-ms
         :deployment-report report))

(defn mark-failed [state now-ms report]
  (when-not (= :rebuilding (:phase state))
    (throw (ex-info "failure reporting requires an in-progress rebuild"
                    {:type :invalid-transition :phase (:phase state)})))
  (when (str/blank? (str report))
    (throw (ex-info "failure reporting requires a report"
                    {:type :invalid-transition})))
  (assoc state
         :phase :failed
         :failed-at-ms now-ms
         :failure-report report))

