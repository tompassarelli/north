;; Durable liveness markers for independently supervised workers.
(ns north.worker-heartbeat
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.time Instant]
           [java.time.temporal ChronoUnit]
           [java.nio.file Files StandardCopyOption]))

(defn valid-worker? [worker]
  (boolean (re-matches #"[a-z0-9]+(?:-[a-z0-9]+)*" (str worker))))

(defn heartbeat-file [worker port]
  (when-not (valid-worker? worker)
    (throw
     (ex-info "worker heartbeat requires a canonical worker name"
              {:worker worker})))
  (if-let [override (System/getenv "NORTH_WORKER_HEARTBEAT")]
    (io/file override)
    (io/file
     (System/getenv "HOME")
     ".cache"
     "north"
     (str "worker-heartbeat-" worker "-" port))))

(defn write-heartbeat! [worker port details]
  (try
    (let [file (heartbeat-file worker port)
          directory (.getParentFile file)
          now (Instant/now)
          temporary (io/file (str (.getPath file) ".tmp"))]
      (when directory (.mkdirs directory))
      (spit temporary (pr-str {:at (str now) :details details}))
      (Files/move
       (.toPath temporary)
       (.toPath file)
       (into-array
        StandardCopyOption
        [StandardCopyOption/ATOMIC_MOVE
         StandardCopyOption/REPLACE_EXISTING]))
      now)
    (catch Throwable error
      (println
       (str "[worker-heartbeat] worker=" worker
            " write failed: " (.getMessage error)))
      nil)))

(defn read-heartbeat-record [worker port]
  (try
    (let [file (heartbeat-file worker port)]
      (when (.isFile file)
        (let [record (edn/read-string (str/trim (slurp file)))
              at (:at record)
              details (:details record)]
          (when (and (map? record)
                     (string? at)
                     (or (nil? details) (map? details)))
            {:ts (Instant/parse at) :details details}))))
    (catch Throwable _ nil)))

(defn heartbeat-status [worker port stale-ms]
  (if-let [{:keys [ts details]} (read-heartbeat-record worker port)]
    (let [age (.between ChronoUnit/MILLIS ts (Instant/now))]
      {:state (if (>= age stale-ms) :stale :fresh)
       :ts ts
       :age-ms age
       :details details})
    {:state :missing :ts nil :age-ms nil :details nil}))

(defn humanize-age [age-ms]
  (let [seconds (quot age-ms 1000)]
    (cond
      (< seconds 60) (str seconds "s")
      (< seconds 3600) (str (quot seconds 60) "m")
      (< seconds 86400) (str (quot seconds 3600) "h")
      :else (str (quot seconds 86400) "d"))))
