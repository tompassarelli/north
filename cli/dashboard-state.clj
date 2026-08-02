(ns north.dashboard.state
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]))

(def schema "north.dashboard/panel-v1")
(def panel-names #{"lanes" "health" "board" "board-all" "ready" "ready-all" "next" "providers"})

(defn cache-dir []
  (str (or (System/getenv "XDG_CACHE_HOME")
           (str (System/getenv "HOME") "/.cache")) "/north/dashboard-v1"))
(defn panel-file [panel] (io/file (cache-dir) (str (name panel) ".edn")))
(defn now [] (System/currentTimeMillis))

(defn- owner-only! [f dir?]
  (.setReadable f false false) (.setWritable f false false) (.setExecutable f false false)
  (.setReadable f true true) (.setWritable f true true)
  (when dir? (.setExecutable f true true))
  f)

(defn read-panel [panel]
  (try
    (let [v (edn/read-string (slurp (panel-file panel)))]
      (when (= schema (:schema v)) v))
    (catch Exception _ nil)))

(defn write-panel! [panel envelope]
  (let [dir (io/file (cache-dir)) f (panel-file panel)
        tmp (io/file dir (str "." (name panel) "." (java.util.UUID/randomUUID) ".tmp"))]
    (.mkdirs dir) (owner-only! dir true)
    (spit tmp (pr-str (assoc envelope :schema schema)))
    (owner-only! tmp false)
    (java.nio.file.Files/move (.toPath tmp) (.toPath f)
      (into-array java.nio.file.StandardCopyOption
                  [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                   java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
    (owner-only! f false)
    (read-panel panel)))

(defn record! [panel result]
  (let [old (or (read-panel panel) {:schema schema}) at (now)
        attempt {:at at :status (name (:status result)) :detail (:detail result)}]
    (write-panel! panel
      (cond-> (assoc old :last-attempt attempt)
        (= :ok (:status result)) (assoc :last-good {:at at :data (:data result)})))))

(declare age-ms)
(defn evidence [envelope]
  (let [good (:last-good envelope) attempt (:last-attempt envelope)]
    (cond
      (nil? good) "never-collected"
      (and attempt (not= "ok" (:status attempt))) "failed-refresh"
      (> (age-ms envelope) 60000) "stale"
      :else "fresh")))

(defn age-ms [envelope]
  (when-let [at (get-in envelope [:last-good :at])] (max 0 (- (now) at))))
(defn age [envelope]
  (if-let [ms (age-ms envelope)]
    (cond (< ms 60000) (str (quot ms 1000) "s")
          (< ms 3600000) (str (quot ms 60000) "m")
          :else (str (quot ms 3600000) "h"))
    "never"))
