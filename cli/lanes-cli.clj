#!/usr/bin/env bb
(ns north.lanes-cli
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.io RandomAccessFile]
           [java.time Instant]))

(load-file (str (.getParent (io/file *file*)) "/coord.clj"))

(def window-ms (* 24 60 60 1000))
(def heartbeat-stale-ms (* 90 1000))
(def startup-diagnostic-prefix "[north startup] NEVER-ACKNOWLEDGED")
(def ^:dynamic *now-ms* #(System/currentTimeMillis))

(defn agents-dir []
  (io/file (or (System/getenv "NORTH_AGENT_LOGS_DIR")
               (str (System/getenv "HOME") "/.local/state/north/agents"))))

(defn port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

(defn read-json [file]
  (try
    (if (.isFile file) (json/parse-string (slurp file)) {})
    (catch Exception _ {})))

(defn instant-ms [value]
  (try (some-> value str Instant/parse .toEpochMilli)
       (catch Exception _ nil)))

(defn bounded-tail
  ([file] (bounded-tail file 16384))
  ([file max-bytes]
   (try
     (if-not (.isFile file)
       ""
       (with-open [raf (RandomAccessFile. file "r")]
         (let [size (.length raf)
               start (max 0 (- size max-bytes))
               bytes (byte-array (int (- size start)))]
           (.seek raf start)
           (.readFully raf bytes)
           (String. bytes java.nio.charset.StandardCharsets/UTF_8))))
     (catch Exception _ ""))))

(defn last-log-line [file]
  (or (->> (str/split-lines (bounded-tail file))
           (remove str/blank?)
           last)
      "(empty log)"))

(defn- normalize-entity [thread]
  (let [value (str thread)]
    (if (str/starts-with? value "@") value (str "@" value))))

(defn resolve-titles
  "Resolve the rendered lane set through one exact FRAMRPC subject batch."
  [port threads]
  (try
    (let [threads-by-entity
          (reduce
           (fn [index thread]
             (update index (normalize-entity thread) (fnil conj []) thread))
           {}
           (distinct (remove str/blank? threads)))]
      (if (empty? threads-by-entity)
        {}
        (let [rows
              (:rows
               (north.coord/show-many-in-domain
                port :coordination (vec (sort (keys threads-by-entity)))))]
          (reduce
           (fn [titles [entity facts]]
             (if-let [title
                      (some (fn [[predicate value]]
                              (when (and (= predicate "title")
                                         (not (str/blank? value)))
                                value))
                            facts)]
               (reduce #(assoc %1 %2 title) titles
                       (get threads-by-entity entity []))
               titles))
           {}
           rows))))
    (catch Exception _ {})))

(defn parse-exit [file]
  (when (.isFile file)
    (try (parse-long (str/trim (slurp file)))
         (catch Exception _ :malformed))))

(defn pid-alive? [file]
  (when (.isFile file)
    (try
      (let [pid (parse-long (str/trim (slurp file)))
            handle (java.lang.ProcessHandle/of pid)]
        (and (.isPresent handle) (.isAlive (.get handle))))
      (catch Exception _ false))))

(defn lane-status [log now-ms]
  (let [path (str log)
        exit-file (io/file (str path ".lane.exit"))
        heartbeat-file (io/file (str path ".lane.heartbeat"))
        pid-file (io/file (str path ".lane.pid"))
        tail (bounded-tail log)
        startup? (str/includes? tail startup-diagnostic-prefix)
        exit (parse-exit exit-file)
        heartbeat-age (when (.isFile heartbeat-file)
                        (- now-ms (.lastModified heartbeat-file)))]
    (cond
      startup? :never-acknowledged
      (some? exit) (if (= 0 exit) :done :failed)
      (some? heartbeat-age) (if (<= heartbeat-age heartbeat-stale-ms)
                              :working
                              :killed)
      ;; Pre-heartbeat logs still carry a wrapper PID with direct lifetime evidence.
      (.isFile pid-file) (if (pid-alive? pid-file) :working :killed)
      :else :unknown)))

(defn lane-files [dir]
  (if-not (.isDirectory dir)
    []
    (->> (.listFiles dir)
         (filter #(.isFile %))
         (filter #(re-matches #"lane-.+\.log" (.getName %))))))

(defn artifact-time [log meta]
  (let [path (str log)
        files [log
               (io/file (str path ".lane.pid"))
               (io/file (str path ".lane.exit"))
               (io/file (str path ".lane.heartbeat"))]]
    (apply max (or (instant-ms (get meta "startedAt")) 0)
           (map #(if (.exists %) (.lastModified %) 0) files))))

;; Same window filter lane-rows applies, factored out so title resolution is
;; bounded to lanes actually rendered rather than every artifact on disk.
(defn windowed-threads
  ([dir now-ms] (windowed-threads dir now-ms window-ms))
  ([dir now-ms period-ms]
   (->> (lane-files dir)
        (keep
         (fn [log]
           (let [id (str/replace (.getName log) #"\.log$" "")
                 meta (read-json (io/file dir (str id ".meta.json")))
                 observed-at (artifact-time log meta)]
             (when (>= observed-at (- now-ms period-ms))
               (get meta "thread")))))
        distinct)))

(defn lane-rows
  ([dir titles now-ms] (lane-rows dir titles now-ms window-ms))
  ([dir titles now-ms period-ms]
   (->> (lane-files dir)
        (keep
         (fn [log]
           (let [id (str/replace (.getName log) #"\.log$" "")
                 meta (read-json (io/file dir (str id ".meta.json")))
                 observed-at (artifact-time log meta)
                 thread (get meta "thread")]
             (when (>= observed-at (- now-ms period-ms))
               {:id id
                :thread thread
                :title (or (get titles thread) thread "(thread unavailable)")
                :status (lane-status log now-ms)
                :observed-at observed-at
                :last-line (last-log-line log)}))))
        (sort-by (juxt (comp - :observed-at) :id))
        vec)))

(defn status-label [status]
  (case status
    :killed "!!! KILLED !!!"
    :never-acknowledged "!!! NEVER-ACKNOWLEDGED !!!"
    :done "done"
    :failed "failed"
    :working "working"
    "UNKNOWN"))

(defn ellipsize [value limit]
  (let [value (str value)]
    (if (<= (count value) limit)
      value
      (str (subs value 0 (dec limit)) "…"))))

(defn render [rows]
  (println "NORTH LANES — last 24h (heartbeat stale after 90s)")
  (if (empty? rows)
    (println "no lane artifacts in the last 24h")
    (doseq [{:keys [id title status last-line]} rows]
      (println (format "%-28s %s | %s" (status-label status) id (ellipsize title 100)))
      (println (str "  last: " last-line)))))

(defn -main [& args]
  (when (seq args)
    (binding [*out* *err*]
      (println "usage: north lanes"))
    (System/exit 2))
  (let [now-ms (*now-ms*)
        dir (agents-dir)
        titles (resolve-titles (port) (windowed-threads dir now-ms))]
    (render (lane-rows dir titles now-ms))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
