#!/usr/bin/env bb
(ns north.rebuild-window-watch
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]))

(def ^:private cli-dir
  (.getParent (io/file (System/getProperty "babashka.file"))))
(def ^:private north-bin
  (.getPath (io/file cli-dir ".." "bin" "north")))

(System/setProperty "north.rebuild-request-cli.lib" "1")
(load-file (str cli-dir "/rebuild-request-cli.clj"))
(load-file (str cli-dir "/rebuild_window_owner.clj"))

(def port
  (Integer/parseInt
   (or (System/getenv "NORTH_PORT")
       (System/getenv "FRAM_PORT")
       "7977")))
(def busy-retries 40)
(def busy-retry-ms 100)
(def active-wait-timeout-ms (* 10 60 1000))
(def active-wait-ms 250)

(defn subscription-request []
  {:op :subscribe
   :filter {:watch #{north.rebuild-request/queue-subject}}})

(defn queue-commit? [event]
  (and (map? event)
       (= :commit (:event event))
       (= north.rebuild-request/queue-subject (:l event))
       (= north.rebuild-request/queue-predicate (:p event))))

(defn wait-for-window-release! [deadline-ns]
  (loop []
    (case (:state (north.rebuild-window-owner/window-unit-state))
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
      (let [result
            (north.rebuild-window-owner/collect! port false north-bin)]
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
              (println (str "[rebuild-owner] wake=" (name reason)
                            " action=active-rearm-deferred"
                            " elapsed_ms=" elapsed-ms))
              (flush)
              (assoc result :elapsed-ms elapsed-ms
                     :reason "active window did not release before fallback")))

          :else
          (let [elapsed-ms
                (long (/ (- (System/nanoTime) started) 1000000))]
            (println (str "[rebuild-owner] wake=" (name reason)
                          " action=" (:action result)
                          " elapsed_ms=" elapsed-ms))
            (flush)
            (assoc result :elapsed-ms elapsed-ms)))))))

(defn process-event! [event]
  (when (queue-commit? event)
    (wake-owner! :queue-commit)))

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
      (wake-owner! :connected)
      (loop []
        (when-let [line (north.coord/read-stream-line-bounded! reader)]
          (when-let [event
                     (try (edn/read-string line)
                          (catch Throwable _ nil))]
            (process-event! event))
          (recur))))))

(defn -main []
  (println (str "[rebuild-owner] scoped fact-server subscription :" port
                " -> " north.rebuild-request/queue-subject))
  (flush)
  (loop []
    (try
      (subscribe-once)
      (catch Throwable error
        (println (str "[rebuild-owner] fact-server subscription lost ("
                      (.getMessage error) ") — reconnecting"))
        (flush)))
    (Thread/sleep 1000)
    (recur)))

(when-not (= "1" (System/getProperty "north.rebuild-window-watch.lib"))
  (-main))
