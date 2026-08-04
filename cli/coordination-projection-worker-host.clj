#!/usr/bin/env bb
(ns north.coordination-projection-worker-host
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str]
            [babashka.process :as proc]))

(def root
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile .getCanonicalPath))
(load-file (str root "/cli/coord.clj"))

(def port
  (Integer/parseInt
   (or (first *command-line-args*)
       (System/getenv "FRAM_PORT")
       "7977")))
(def debounce-ms
  (Integer/parseInt (or (second *command-line-args*) "400")))
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

(defn subscribe-once! []
  (with-open [socket (north.coord/connect-socket port)]
    (let [writer (.getOutputStream socket)
          reader (north.coord/coordinator-reader socket)]
      (.write
       writer
       (.getBytes
        (str (pr-str (north.coord/log-envelope {:op :subscribe})) "\n")
        java.nio.charset.StandardCharsets/UTF_8))
      (.flush writer)
      (north.coord/validate-subscription!
       (north.coord/read-line-bounded! reader))
      (.setSoTimeout socket 0)
      (loop []
        (when-let [line (north.coord/read-stream-line-bounded! reader)]
          (let [event (try (edn/read-string line) (catch Throwable _ nil))]
            (when (and (map? event) (= (:event event) :commit))
              (mark! (:l event))))
          (recur))))))

(defn -main []
  (println
   (str "[coordination-projection-worker] subscribe :" port
        " debounce_ms=" debounce-ms))
  (flush)
  (future (flush-when-quiet!))
  (loop []
    (try
      (subscribe-once!)
      (catch Throwable error
        (println
         (str "[coordination-projection-worker] subscription lost ("
              (.getMessage error) ") — reconnecting"))
        (flush)))
    (Thread/sleep 1000)
    (recur)))

(when-not
 (= "1" (System/getProperty "north.coordination-projection-worker-host.lib"))
 (-main))
