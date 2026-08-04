#!/usr/bin/env bb
(ns north.coordinated-nix-rebuild-worker-host
  (:require [cheshire.core :as json]
            [clojure.edn :as edn]
            [clojure.java.io :as io]))

(def ^:private cli-dir
  (.getParent (io/file (System/getProperty "babashka.file"))))
(def ^:private root
  (.getCanonicalPath (io/file cli-dir "..")))
(def ^:private north-bin
  (.getPath (io/file cli-dir ".." "bin" "north")))

(System/setProperty "north.rebuild-request-cli.lib" "1")
(load-file (str cli-dir "/rebuild-request-cli.clj"))
(load-file (str cli-dir "/coordinated_nix_rebuild.clj"))
(load-file (str root "/out/north/worker_policy.clj"))

(def port
  (Integer/parseInt
   (or (System/getenv "NORTH_PORT")
       (System/getenv "FRAM_PORT")
       "7977")))
(def busy-retries 40)
(def busy-retry-ms 100)
(def active-wait-timeout-ms (* 10 60 1000))
(def active-wait-ms 250)
(def event-debounce-ms 1000)
(def ^:private max-event-debounce-ms 2000)

(def ^:private event-lock (Object.))
(def ^:private event-state
  (atom {:version -1 :semantic nil}))

(defn subscription-request []
  {:op :subscribe
   :filter {:watch #{north.rebuild-request/queue-subject}}})

(defn queue-commit? [event]
  (and (map? event)
       (= :commit (:event event))
       (= north.rebuild-request/queue-subject (:l event))
       (= north.rebuild-request/queue-predicate (:p event))))

(defn queue-semantic [raw]
  (let [payload
        (try
          (json/parse-string (str raw))
          (catch Throwable error
            (throw (ex-info "rebuild queue event is not valid JSON"
                            {:type :malformed-rebuild-queue-event}
                            error))))
        requests (get payload "requests")
        last-fired-ms (get payload "lastFiredMs")
        ids (when (vector? requests) (mapv #(get % "id") requests))]
    (when-not (and (map? payload)
                   (vector? requests)
                   (every? string? ids)
                   (= (count ids) (count (distinct ids)))
                   (or (nil? last-fired-ms)
                       (and (integer? last-fired-ms)
                            (not (neg? last-fired-ms)))))
      (throw (ex-info "rebuild queue event has invalid semantic fields"
                      {:type :malformed-rebuild-queue-event})))
    {:request-ids (vec (sort ids))
     :last-fired-ms last-fired-ms}))

(defn queue-observation [version raw]
  (when-not (and (integer? version) (not (neg? version)))
    (throw (ex-info "rebuild queue event requires a non-negative version"
                    {:type :malformed-rebuild-queue-event-version
                     :version version})))
  {:version version :semantic (queue-semantic raw)})

(defn current-queue-observation []
  (let [{:keys [version rows]}
        (north.coord/show-envelope port north.rebuild-request/queue-subject)
        values (->> rows
                    (keep (fn [[predicate value]]
                            (when (= north.rebuild-request/queue-predicate
                                     predicate)
                              value)))
                    vec)]
    (when (> (count values) 1)
      (throw (ex-info "rebuild queue singleton has multiple live values"
                      {:type :malformed-rebuild-queue-event})))
    (if-let [raw (first values)]
      (queue-observation version raw)
      {:version version
       :semantic {:request-ids [] :last-fired-ms nil}})))

(defn reset-event-state! []
  (reset! event-state {:version -1 :semantic nil}))

(defn- observe-current-queue []
  (try
    (current-queue-observation)
    (catch Throwable _ nil)))

(defn- advance-event-state! [observation]
  (when observation
    (swap! event-state
           (fn [current]
             (if (>= (:version observation) (:version current))
               observation
               current)))))

(defn- bounded-event-debounce-ms []
  (long (min max-event-debounce-ms
             (max 0 event-debounce-ms))))

(defn- debounce! []
  (Thread/sleep (bounded-event-debounce-ms)))

(defn wait-for-window-release! [deadline-ns]
  (loop []
    (case (:state (north.coordinated-nix-rebuild/window-unit-state))
      :inactive true
      :unknown false
      :active
      (if (>= (System/nanoTime) deadline-ns)
        false
        (do
          (Thread/sleep active-wait-ms)
          (recur))))))

(defn wake-owner! [reason]
  (let [started (System/nanoTime)
        active-deadline
        (+ started (* 1000000 active-wait-timeout-ms))]
    (loop [attempt 0]
      (let [queue-observation (observe-current-queue)
            result
            (north.coordinated-nix-rebuild/collect! port false north-bin)]
        (cond
          (and (= "owner-busy" (:action result))
               (< attempt busy-retries))
          (do
            (Thread/sleep busy-retry-ms)
            (recur (inc attempt)))

          (= "active" (:action result))
          (if (wait-for-window-release! active-deadline)
            (recur 0)
            (let [elapsed-ms
                  (long (/ (- (System/nanoTime) started) 1000000))]
              (println (str "[coordinated-nix-rebuild-worker] wake=" (name reason)
                            " action=active-rearm-deferred"
                            " elapsed_ms=" elapsed-ms))
              (flush)
              (assoc result :elapsed-ms elapsed-ms
                     :queue-observation queue-observation
                     :reason "active window did not release before fallback")))

          :else
          (let [elapsed-ms
                (long (/ (- (System/nanoTime) started) 1000000))]
            (println (str "[coordinated-nix-rebuild-worker] wake=" (name reason)
                          " action=" (:action result)
                          " elapsed_ms=" elapsed-ms))
            (flush)
            (assoc result
                   :elapsed-ms elapsed-ms
                   :queue-observation queue-observation)))))))

(defn process-event! [event]
  (when (queue-commit? event)
    (locking event-lock
      (let [incoming (queue-observation (:version event) (:r event))
            observed @event-state
            decision
            (north.worker-policy/rebuild-wake-decision
             (:version observed)
             (:version incoming)
             (not= (:semantic incoming) (:semantic observed))
             (boolean
              (seq (get-in incoming [:semantic :request-ids]))))]
        (case (:action decision)
          :ignore nil
          :advance (advance-event-state! incoming)
          :wake
          (do
            (advance-event-state! incoming)
            (debounce!)
            (let [result (wake-owner! :queue-commit)]
              (advance-event-state! (:queue-observation result))
              result)))))))

(defn connected-catch-up! []
  (locking event-lock
    (let [result (wake-owner! :connected)]
      (advance-event-state! (:queue-observation result))
      result)))

(defn subscribe-once []
  (with-open [socket (north.coord/connect-socket port)]
    (let [writer (.getOutputStream socket)
          reader (north.coord/coordinator-reader socket)]
      (.write writer
              (.getBytes
               (str (pr-str
                     (north.coord/log-envelope (subscription-request)))
                    "\n")
               java.nio.charset.StandardCharsets/UTF_8))
      (.flush writer)
      (north.coord/validate-subscription!
       (north.coord/read-line-bounded! reader))
      (.setSoTimeout socket 0)
      (connected-catch-up!)
      (loop []
        (when-let [line (north.coord/read-stream-line-bounded! reader)]
          (when-let [event
                     (try (edn/read-string line)
                          (catch Throwable _ nil))]
            (process-event! event))
          (recur))))))

(defn -main []
  (println (str "[coordinated-nix-rebuild-worker] scoped fact-server subscription :" port
                " -> " north.rebuild-request/queue-subject))
  (flush)
  (loop []
    (try
      (subscribe-once)
      (catch Throwable error
        (println (str "[coordinated-nix-rebuild-worker] fact-server subscription lost ("
                      (.getMessage error) ") — reconnecting"))
        (flush)))
    (Thread/sleep 1000)
    (recur)))

(when-not (= "1" (System/getProperty "north.coordinated-nix-rebuild-worker-host.lib"))
  (-main))
