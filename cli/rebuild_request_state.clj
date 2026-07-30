(ns north.rebuild-request-state
  (:require [clojure.string :as str]))

(def protocol-version "north:rebuild-request:v1")

;; One hour: long enough that a burst of agent asks coalesces into a single
;; disruption, short enough that a queued request is never more than one working
;; interruption away from landing. Overridable as `north config rebuild-window`.
(def default-window-seconds 3600)

;; The window owner opens at most ONE coordinated rebuild per window, so a
;; second is already an out-of-band firing; three inside one window means the
;; queue is being bypassed or something is rebuilding in a loop.
(def default-rebuilds-per-window-threshold 2)

;; A rebuild reason is one line of provenance, not an essay; the composed intent
;; why must stay readable in a broadcast.
(def max-why-chars 512)
(def max-composed-reasons 8)

(defn- non-blank-string? [value]
  (and (string? value) (not (str/blank? value))))

(defn new-request
  [{:keys [id requester why thread created-at-ms urgent-reason]}]
  (when-not (non-blank-string? id)
    (throw (ex-info "rebuild request requires an id" {:type :invalid-request})))
  (when-not (non-blank-string? requester)
    (throw (ex-info "rebuild request requires a requester" {:type :invalid-request})))
  (when-not (non-blank-string? why)
    (throw (ex-info "rebuild request requires --why" {:type :invalid-request})))
  (when (> (count why) max-why-chars)
    (throw (ex-info (str "rebuild request --why must not exceed " max-why-chars
                         " characters")
                    {:type :invalid-request})))
  (when-not (or (nil? thread) (non-blank-string? thread))
    (throw (ex-info "rebuild request --thread must not be blank" {:type :invalid-request})))
  (when-not (and (integer? created-at-ms) (not (neg? created-at-ms)))
    (throw (ex-info "rebuild request requires a non-negative creation time"
                    {:type :invalid-request})))
  (when-not (or (nil? urgent-reason) (non-blank-string? urgent-reason))
    (throw (ex-info "rebuild request --urgent requires a reason" {:type :invalid-request})))
  (when (and urgent-reason (> (count urgent-reason) max-why-chars))
    (throw (ex-info (str "rebuild request --urgent reason must not exceed " max-why-chars
                         " characters")
                    {:type :invalid-request})))
  (cond-> {:version protocol-version
           :id id
           :requester requester
           :why why
           :created-at-ms created-at-ms
           :urgent (boolean urgent-reason)}
    thread (assoc :thread thread)
    urgent-reason (assoc :urgent-reason urgent-reason)))

(defn open? [request]
  (nil? (:satisfied request)))

(defn age-ms [request now-ms]
  (max 0 (- now-ms (:created-at-ms request))))

(defn open-requests [requests]
  (->> requests (filter open?) (sort-by :created-at-ms) vec))

(defn requesters [requests]
  (->> requests (map :requester) distinct vec))

(defn compose-why
  "One intent reason for the whole window: every queued ask, in arrival order,
   bounded so the broadcast stays readable."
  [requests]
  (let [ordered (sort-by :created-at-ms requests)
        shown (take max-composed-reasons ordered)
        extra (max 0 (- (count ordered) max-composed-reasons))]
    (str "queued rebuild requests (" (count ordered) "): "
         (str/join "; " (map (fn [r]
                               (str (:requester r) ": " (:why r)
                                    (when (:urgent r) " [urgent]")))
                             shown))
         (when (pos? extra) (str "; +" extra " more")))))

(defn window-plan
  "What the window owner should do this sweep. Pure: `now-ms`, `last-window-ms`
   (the last window that actually fired, or nil), `window-ms`, the decoded
   `requests`, and whether the rebuild-coordination flip is on.

   Cause order matters. Coordination-off is reported ahead of the window bound
   because it is the dominant reason nothing fires, and an empty queue is idle
   whatever the clock says. A window is consumed only by a firing, so a parked
   queue never silently burns its own window."
  [{:keys [now-ms last-window-ms window-ms requests coordination-on?]}]
  (let [open (open-requests requests)
        base {:open open :count (count open)}]
    (cond
      (empty? open)
      (assoc base :action :idle)

      (not coordination-on?)
      (assoc base :action :queued
             :reason :rebuild-coordination-off)

      (and (not-any? :urgent open)
           last-window-ms
           (< (- now-ms last-window-ms) window-ms))
      (assoc base :action :waiting
             :reason :window-not-due
             :next-due-ms (+ last-window-ms window-ms))

      :else
      (assoc base :action :fire
             :why (compose-why open)
             :requesters (requesters open)))))

(defn rebuild-gauge
  "Coordinated rebuilds observed inside the trailing window. `rebuild-times-ms`
   are intent creation times; a rebuild fired while coordination is off leaves no
   intent and is therefore invisible here — the renderer says so."
  ([rebuild-times-ms now-ms window-ms]
   (rebuild-gauge rebuild-times-ms now-ms window-ms default-rebuilds-per-window-threshold))
  ([rebuild-times-ms now-ms window-ms threshold]
   (let [n (count (filter #(and (some? %) (>= % (- now-ms window-ms)) (<= % now-ms))
                          rebuild-times-ms))]
     {:count n :threshold threshold :breached? (> n threshold)})))

(defn urgent-rate
  "Share of requests inside `period-ms` that bypassed the window with --urgent.
   --urgent is never refused, so visibility is its only guard."
  [requests now-ms period-ms]
  (let [recent (filter #(>= (:created-at-ms %) (- now-ms period-ms)) requests)
        total (count recent)
        urgent (count (filter :urgent recent))]
    {:total total
     :urgent urgent
     :rate (if (pos? total) (/ (double urgent) total) 0.0)}))

(defn humanize-age [age-ms]
  (let [s (quot age-ms 1000)]
    (cond
      (< s 60) (str s "s")
      (< s 3600) (str (quot s 60) "m")
      (< s 86400) (str (quot s 3600) "h")
      :else (str (quot s 86400) "d"))))
