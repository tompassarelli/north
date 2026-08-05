#!/usr/bin/env bb
(ns north.coordination-projection-worker-host
  (:require [babashka.classpath :as classpath]
            [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def root
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile .getCanonicalPath))
(def fram-out
  (or (not-empty (System/getenv "FRAM_OUT"))
      "/home/tom/code/fram/wt-core-target-production-5db9b38/out"))
(classpath/add-classpath fram-out)
(when-not (find-ns 'north.framrpc-client)
  (load-file (str root "/cli/framrpc-client.clj")))
(alias 'rpc 'north.framrpc-client)
(alias 'term 'fram.types)
(load-file (str root "/cli/coord.clj"))

(def port
  (Integer/parseInt
   (or (first *command-line-args*)
       (System/getenv "FRAM_SERVER_PORT")
       "7977")))
(def debounce-ms
  (Integer/parseInt (or (second *command-line-args*) "400")))
(def poll-ms 500)
(def server-host
  (or (not-empty (System/getenv "FRAM_SERVER_CONNECT")) "127.0.0.1"))
(def space-id
  (or (not-empty (System/getenv "FRAM_SPACE_ID")) "north-coordination"))
(def north-bin (str root "/bin/north"))
(def last-commit (atom 0))
(def dirty (atom false))
(def running (atom false))
(def last-heal-output (atom nil))
(def ephemeral-prefixes
  ["@lease:" "@session:" "@run:" "@cmd:" "@agent:" "@role:"
   "@notification:" "@subscription:"])

(defn heal! []
  (try
    (let [result
          (proc/shell
           {:out :string
            :err :string
            :continue true
            :extra-env
            {"FRAM_LOG" (north.coord/expected-log)
             "NORTH_TELEMETRY_PARTITION" "0"
             "FRAM_TELEMETRY_LOG" ""}}
           north-bin "heal")
          output
          (str/trim
           (str (:out result)
                (when (seq (:err result)) (str "\n" (:err result)))))
          line
          (when (seq output)
            (str "[coordination-projection-worker] "
                 (str/replace output #"\n+" " | ")))]
      (when (and line (not= line @last-heal-output))
        (println line)
        (flush))
      (reset! last-heal-output line))
    (catch Throwable error
      (println
       (str "[coordination-projection-worker] heal error: "
            (.getMessage error)))
      (flush))))

(defn flush-when-quiet! []
  (loop []
    (Thread/sleep 100)
    (when (and @dirty
               (not @running)
               (>= (- (System/currentTimeMillis) @last-commit) debounce-ms))
      (reset! dirty false)
      (reset! running true)
      (try
        (heal!)
        (finally (reset! running false))))
    (recur)))

(defn mark! [subject]
  (when (and (string? subject)
             (not
              (some #(str/starts-with? subject %) ephemeral-prefixes)))
    (reset! last-commit (System/currentTimeMillis))
    (reset! dirty true)))

(defn connect! []
  (rpc/connect server-host port space-id
               {:connect-timeout-ms 1000
                :read-timeout-ms 60000
                :max-attempts 3
                :retry-delay-ms 10
                :jitter-ms 25}))

(defn triple-values [triple]
  [(term/triple-slot0 triple)
   (term/triple-slot1 triple)
   (term/triple-slot2 triple)])

(defn tracked-projection [rows]
  (reduce
   (fn [projection triple]
     (let [[subject :as values] (triple-values triple)]
       (if (and (string? subject)
                (not
                 (some #(str/starts-with? subject %) ephemeral-prefixes)))
         (update-in projection [subject values] (fnil inc 0))
         projection)))
   {}
   rows))

(defn observation! [client]
  (let [{:keys [rows served-version]}
        (rpc/scan-all! client nil nil nil)]
    {:version served-version
     :projection (tracked-projection rows)}))

(defn poll-once! [client previous]
  (let [served-version (:served-version (rpc/version! client))]
    (if (= served-version (:version previous))
      previous
      (let [current (observation! client)]
        (when (not= (:projection previous) (:projection current))
          (mark! "@coordination-projection"))
        current))))

(defn session! []
  (let [client (connect!)]
    (try
      (let [initial (observation! client)]
        (mark! "@coordination-projection")
        (loop [observation initial]
          (Thread/sleep poll-ms)
          (recur (poll-once! client observation))))
      (finally
        (rpc/close! client)))))

(defn -main []
  (println
   (str "[coordination-projection-worker] FRAMRPC v1 poll "
        server-host ":" port
        " space=" space-id
        " poll_ms=" poll-ms
        " debounce_ms=" debounce-ms))
  (flush)
  (future (flush-when-quiet!))
  (loop []
    (try
      (session!)
      (catch Throwable error
        (println
         (str "[coordination-projection-worker] FRAMRPC poll lost ("
              (.getMessage error) ") — reconnecting"))
        (flush)))
    (Thread/sleep 1000)
    (recur)))

(when-not
 (= "1" (System/getProperty "north.coordination-projection-worker-host.lib"))
 (-main))
