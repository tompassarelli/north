#!/usr/bin/env bb
(require '[babashka.process :as process]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[fram.kernel :as kernel]
         '[fram.rt :as rt]
         '[north.main :as main])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "..")))
(def helper (str root "/cli/framrpc-command.clj"))

(defn env-value [name default]
  (or (not-empty (System/getenv name)) default))

(defn positive-port [name default]
  (let [value (parse-long (env-value name default))]
    (when-not (and value (<= 1 value 65535))
      (throw (ex-info (str name " must be an integer from 1 through 65535")
                      {:name name :value (System/getenv name)})))
    value))

(def coordination
  {:host (env-value "NORTH_FRAMRPC_HOST" "127.0.0.1")
   :port (positive-port "NORTH_PORT" "7977")
   :space (env-value "FRAM_SPACE_ID" "north-coordination")})

(def telemetry
  {:host (env-value "NORTH_FRAMRPC_HOST" "127.0.0.1")
   :port (positive-port "NORTH_TELEMETRY_PORT" "7978")
   :space (env-value "NORTH_TELEMETRY_SPACE_ID" "north-telemetry")})

(defn telemetry-enabled? []
  (= "1" (env-value "NORTH_TELEMETRY_PARTITION" "0")))

(def ^:dynamic *selected-stores* nil)
(def ^:dynamic *writer-store* nil)

(defn stores []
  (or *selected-stores*
      (cond-> [coordination] (telemetry-enabled?) (conj telemetry))))

(defn invoke! [store command & arguments]
  (let [bb (env-value "NORTH_BB" "bb")
        head-out (env-value "NORTH_FRAMRPC_OUT" "/home/tom/code/fram/main/out")
        argv (into [bb "-cp" head-out helper command
                    (:host store) (str (:port store)) (:space store)]
                   arguments)
        result @(process/process argv {:out :string :err :string})]
    (when-not (zero? (:exit result))
      (throw (ex-info "FRAMRPC compatibility command failed"
                      {:store store :command command :arguments arguments
                       :exit (:exit result) :stderr (str/trim (:err result))})))
    (edn/read-string (:out result))))

(defn fact [row]
  (apply kernel/->Fact row))

(defn live-state [_port _log]
  (let [results (mapv #(invoke! % "scan-all") (stores))]
    {:version (:served-version (first results))
     :facts (mapv fact (mapcat :rows results))
     :complete true
     :domains (zipmap (map :space (stores))
                      (map #(select-keys % [:served-version :pages]) results))}))

(defn live-facts [port log]
  (:facts (live-state port log)))

(defn show-for-log [_port _log subject]
  (let [rows (mapcat :rows (map #(invoke! % "scan-subject" subject) (stores)))]
    {:rows (mapv (fn [[_ predicate value]] [predicate value]) rows)}))

(defn store-for [port log]
  (or *writer-store*
      (if (or (= port (:port telemetry))
              (and (telemetry-enabled?)
                   (= (.getCanonicalPath (io/file log))
                      (.getCanonicalPath
                       (io/file (env-value "FRAM_TELEMETRY_LOG" "/nonexistent"))))))
        telemetry
        coordination)))

(defn version-for-log [port log]
  (try
    (long (invoke! (store-for port log) "version"))
    (catch Throwable _ -1)))

(defn write-for-log [operation port log subject predicate value expected]
  (let [result (invoke! (store-for port log) "write" operation subject predicate
                        value (str expected))]
    (cond
      (= :conflict (:error result)) "conflict"
      (integer? (:served-version result)) (str "ok:" (:served-version result))
      :else (throw (ex-info "FRAMRPC write returned an invalid result"
                            {:result result})))))

(defn status-for-log [port log]
  (try
    (let [result (invoke! (store-for port log) "status")]
      (str "up|" (:served-version result) "|" (:live-count result)
           "|" (name (:state result)) "|" (name (:engine result))))
    (catch Throwable _ "down")))

(defn unsupported-request [_port _log request]
  (throw (ex-info "legacy coordinator request has no FRAMRPC compatibility mapping"
                  {:operation (:op request)})))

(def telemetry-clock-verbs #{"start" "orphan" "sync"})
(def client-clock-authority-verbs #{"in" "current" "status" "out" "stop"})

(defn clock-command? [verbs args]
  (and (telemetry-enabled?) (= "clock" (first args))
       (contains? verbs (or (second args) "current"))))

(defn coordination-projection? [args]
  (let [command (if (= "json" (first args)) (second args) (first args))]
    (and (contains? #{"ready" "board" "plate"} command)
         (not (some #{"--all"} args)))))

(defn -main [& args]
  (let [authority? (clock-command? client-clock-authority-verbs args)
        telemetry-write? (clock-command? telemetry-clock-verbs args)]
    (binding [*selected-stores* (when (or authority? (coordination-projection? args))
                                 [coordination])
              *writer-store* (when telemetry-write? telemetry)]
      (with-redefs [rt/coord-live-state live-state
                    rt/coord-live-facts live-facts
                    rt/coord-show-for-log show-for-log
                    rt/coord-version-for-log version-for-log
                    rt/coord-status-for-log status-for-log
                    rt/coord-assert-for-log
                    (fn [port log subject predicate value expected]
                      (write-for-log "assert" port log subject predicate value expected))
                    rt/coord-retract-for-log
                    (fn [port log subject predicate value expected]
                      (write-for-log "retract" port log subject predicate value expected))
                    rt/coord-request-for-log unsupported-request]
        (apply main/-main args)))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
