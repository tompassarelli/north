#!/usr/bin/env bb
(ns north.nix-rebuild-worker
  (:require [cheshire.core :as json]
            [clojure.edn :as edn]
            [clojure.java.io :as io]))

(def ^:private cli-dir
  (.getParent (io/file (System/getProperty "babashka.file"))))
(def ^:private root
  (.getCanonicalPath (io/file cli-dir "..")))
(System/setProperty "north.rebuild-request-cli.lib" "1")
(load-file (str cli-dir "/rebuild-request-cli.clj"))
(load-file (str root "/out/north/worker_policy.clj"))

(def port
  (Integer/parseInt
   (or (System/getenv "NORTH_PORT")
       (System/getenv "FRAM_PORT")
       "7977")))
(def event-debounce-ms 1000)
(def ^:private max-event-debounce-ms 2000)

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

(defn process-queue!
  "Synchronously process at most one coalesced window from the durable queue."
  [port dry?]
  (try
    (let [plan (north.rebuild-request/plan-window port)
          n (:count plan)
          queue-read (:queue-read plan)]
      (println (str "[nix-rebuild-worker] queue"
                    " mode=" (:mode queue-read)
                    " bridge_start=" (:start-offset queue-read)
                    " bridge_end=" (:end-offset queue-read)
                    " bridge_target=" (:target-offset queue-read)
                    " bridge_bytes=" (:bytes-read queue-read)
                    " bridge_events=" (:relevant-events queue-read)
                    " corpus_queries=" (:corpus-queries queue-read)
                    " caught_up=" (:caught-up queue-read)))
      (case (:action plan)
        :idle {:action "idle" :count 0 :queue-read queue-read}
        :queued {:action "queued" :count n :queue-read queue-read}
        :waiting {:action "waiting" :count n :queue-read queue-read}
        :fire
        (if dry?
          (do
            (println (str "[nix-rebuild-worker] WOULD process a rebuild request window for "
                          n " request(s)"))
            {:action "would-fire" :count n :queue-read queue-read})
          (let [window-id
                (north.rebuild-request/open-window!
                 port (mapv :id (:open plan)))
                exit (north.rebuild-request/run-window! port window-id)]
            (if (zero? exit)
              (do
                (println (str "[nix-rebuild-worker] window " window-id
                              " completed for " n " request(s)"))
                {:action "fired" :count n :window window-id
                 :queue-read queue-read})
              {:action "failed" :count n :window window-id :exit exit
               :queue-read queue-read})))
        {:action "error" :count n
         :error (ex-info "unsupported rebuild window plan"
                         {:type :unsupported-rebuild-window-plan
                          :plan plan})}))
    (catch Throwable error
      (println (str "[nix-rebuild-worker] error: " (.getMessage error)))
      {:action "error" :count 0 :error error})))

(defn run-worker! [reason]
  (let [started (System/nanoTime)
        queue-observation (observe-current-queue)
        result (process-queue! port false)
        elapsed-ms (long (/ (- (System/nanoTime) started) 1000000))]
    (println (str "[nix-rebuild-worker] wake=" (name reason)
                  " action=" (:action result)
                  " elapsed_ms=" elapsed-ms))
    (flush)
    (assoc result
           :elapsed-ms elapsed-ms
           :queue-observation queue-observation)))

(defn process-event! [event]
  (when (queue-commit? event)
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
          (let [result (run-worker! :queue-commit)]
            (advance-event-state! (:queue-observation result))
            result))))))

(defn connected-catch-up! []
  (let [result (run-worker! :connected)]
    (advance-event-state! (:queue-observation result))
    result))

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
  (println (str "[nix-rebuild-worker] scoped fact-server subscription :" port
                " -> " north.rebuild-request/queue-subject))
  (flush)
  (loop []
    (try
      (subscribe-once)
      (catch Throwable error
        (println (str "[nix-rebuild-worker] fact-server subscription lost ("
                      (.getMessage error) ") — reconnecting"))
        (flush)))
    (Thread/sleep 1000)
    (recur)))

(when-not (= "1" (System/getProperty "north.nix-rebuild-worker.lib"))
  (-main))
