#!/usr/bin/env bb
(ns north.nix-rebuild-worker
  (:require [babashka.classpath :as classpath]
            [cheshire.core :as json]
            [clojure.java.io :as io]))

(def ^:private cli-dir
  (.getParent (io/file (System/getProperty "babashka.file"))))
(def ^:private root
  (.getCanonicalPath (io/file cli-dir "..")))
(def ^:private fram-out
  (or (not-empty (System/getenv "FRAM_OUT"))
      (str (System/getProperty "user.home") "/code/fram/main/out")))
(classpath/add-classpath fram-out)
(when-not (find-ns 'north.framrpc-client)
  (load-file (str cli-dir "/framrpc-client.clj")))
(alias 'rpc 'north.framrpc-client)
(alias 'term 'fram.types)
(System/setProperty "north.rebuild-request-cli.lib" "1")
(load-file (str cli-dir "/rebuild-request-cli.clj"))
(load-file (str root "/out/north/worker_policy.clj"))

(def port
  (Integer/parseInt
   (or (System/getenv "FRAM_SERVER_PORT") "7977")))
(def server-host
  (or (not-empty (System/getenv "FRAM_SERVER_CONNECT")) "127.0.0.1"))
(def space-id
  (or (not-empty (System/getenv "FRAM_SPACE_ID")) "north-coordination"))
(def coalesce-ms 1000)
(def ^:private max-coalesce-ms 2000)
(def poll-ms 1000)

(def ^:private observed-state
  (atom {:version -1 :semantic nil}))
(def ^:dynamic *client* nil)

(defn queue-semantic [raw]
  (let [payload
        (try
          (json/parse-string (str raw))
          (catch Throwable error
            (throw (ex-info "rebuild queue projection is not valid JSON"
                            {:type :malformed-rebuild-queue-projection}
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
      (throw (ex-info "rebuild queue projection has invalid semantic fields"
                      {:type :malformed-rebuild-queue-projection})))
    {:request-ids (vec (sort ids))
     :last-fired-ms last-fired-ms}))

(defn queue-observation [version raw]
  (when-not (and (integer? version) (not (neg? version)))
    (throw (ex-info "rebuild queue projection requires a non-negative version"
                    {:type :malformed-rebuild-queue-projection-version
                     :version version})))
  {:version version :semantic (queue-semantic raw)})

(defn connect! []
  (rpc/connect server-host port space-id
               {:connect-timeout-ms 1000
                :read-timeout-ms 60000
                :max-attempts 3
                :retry-delay-ms 10
                :jitter-ms 25}))

(defn with-client [operation]
  (if *client*
    (operation *client*)
    (let [client (connect!)]
      (try
        (operation client)
        (finally
          (rpc/close! client))))))

(defn queue-scan! [client]
  (rpc/scan-all! client
                 north.rebuild-request/queue-subject
                 north.rebuild-request/queue-predicate
                 nil))

(defn triple-value [triple]
  (term/triple-slot2 triple))

(defn current-queue-observation []
  (with-client
    (fn [client]
      (let [{:keys [served-version rows]} (queue-scan! client)
            values (mapv triple-value rows)]
        (when (> (count values) 1)
          (throw (ex-info "rebuild queue singleton has multiple live values"
                          {:type :malformed-rebuild-queue-projection})))
        (if-let [raw (first values)]
          (queue-observation served-version raw)
          {:version served-version
           :semantic {:request-ids [] :last-fired-ms nil}})))))

(defn reset-observed-state! []
  (reset! observed-state {:version -1 :semantic nil}))

(defn- observe-current-queue []
  (try
    (current-queue-observation)
    (catch Throwable _ nil)))

(defn- advance-observed-state! [observation]
  (when observation
    (swap! observed-state
           (fn [current]
             (if (>= (:version observation) (:version current))
               observation
               current)))))

(defn- bounded-coalesce-ms []
  (long (min max-coalesce-ms
             (max 0 coalesce-ms))))

(defn- coalesce! []
  (Thread/sleep (bounded-coalesce-ms)))

(defn process-queue!
  "Synchronously process at most one coalesced window from the durable queue."
  [port dry?]
  (try
    (let [plan (north.rebuild-request/plan-window port)
          n (:count plan)
          queue-read (:queue-read plan)]
      (println (str "[nix-rebuild-worker] queue"
                    " mode=" (:mode queue-read)
                    " served_version=" (:served-version queue-read)
                    " pages=" (:pages queue-read)
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

(defn process-observation! [incoming reason]
  (let [observed @observed-state
        decision
        (north.worker-policy/rebuild-wake-decision
         (:version observed)
         (:version incoming)
         (not= (:semantic incoming) (:semantic observed))
         (boolean
          (seq (get-in incoming [:semantic :request-ids]))))]
    (case (:action decision)
      :ignore nil
      :advance (advance-observed-state! incoming)
      :wake
      (do
        (advance-observed-state! incoming)
        (coalesce!)
        (let [result (run-worker! reason)]
          (advance-observed-state! (:queue-observation result))
          result)))))

(defn poll-once! []
  (process-observation! (current-queue-observation) :queue-poll))

(defn initial-catch-up! []
  (let [result (run-worker! :connected)]
    (advance-observed-state! (:queue-observation result))
    result))

(defn session! []
  (let [client (connect!)]
    (try
      (binding [*client* client]
        (initial-catch-up!)
        (loop []
          (Thread/sleep poll-ms)
          (poll-once!)
          (recur)))
      (finally
        (rpc/close! client)))))

(defn -main []
  (println
   (str "[nix-rebuild-worker] FRAMRPC projection poll "
        server-host ":" port
        " space=" space-id
        " -> rebuild request/window projections"))
  (flush)
  (loop []
    (try
      (session!)
      (catch Throwable error
        (println
         (str "[nix-rebuild-worker] FRAMRPC projection poll lost ("
              (.getMessage error) ") — reconnecting"))
        (flush)))
    (Thread/sleep 1000)
    (recur)))

(when-not (= "1" (System/getProperty "north.nix-rebuild-worker.lib"))
  (-main))
