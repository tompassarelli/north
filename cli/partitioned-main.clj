#!/usr/bin/env bb
;; Compatibility adapter for North's compiled Beagle CLI while telemetry has an
;; independent writer. Reads compose each writer's materialized live facts; they
;; never concatenate independently sequenced event histories. Mutating clock
;; commands use the telemetry writer while retaining the composed read view.
(require '[fram.kernel :as kernel]
         '[fram.rt :as rt]
         '[north.main :as main]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))

(def original-live-facts rt/coord-live-facts)
(def original-coord-port rt/coord-port)
(def original-version-for-log rt/coord-version-for-log)
(def original-assert-for-log rt/coord-assert-for-log)
(def original-retract-for-log rt/coord-retract-for-log)
(def original-request-for-log rt/coord-request-for-log)

(defn telemetry-port []
  (Integer/parseInt (System/getenv "NORTH_TELEMETRY_PORT")))

(defn coordination-port []
  (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))

(defn telemetry-log []
  (north.coord/telemetry-log-path))

(defn composed-live-facts [port log]
  (if-not (north.coord/telemetry-partition-enabled?)
    (original-live-facts port log)
    (let [{:keys [facts unavailable]}
          (north.coord/live-facts-view (coordination-port))]
      (when (seq unavailable)
        (binding [*out* *err*]
          (println
           (str "north: partial cross-log view; unavailable domain(s): "
                (str/join ", " unavailable)))))
      (mapv (fn [[subject predicate value]]
              (kernel/->Fact subject predicate value))
            facts))))

(def mutating-clock-verbs
  #{"in" "out" "start" "stop" "orphan" "sync"})

(defn telemetry-clock-command? [args]
  (and (north.coord/telemetry-partition-enabled?)
       (= "clock" (first args))
       (contains? mutating-clock-verbs (or (second args) "status"))))

(defn -main [& args]
  (let [telemetry-write? (telemetry-clock-command? args)
        writer-port (if telemetry-write? (telemetry-port) (original-coord-port))
        writer-log (if telemetry-write? (telemetry-log) (rt/log-path))]
    (with-redefs
     [rt/coord-live-facts composed-live-facts
      rt/coord-port (fn [] writer-port)
      rt/coord-version-for-log
      (fn [_port _log] (original-version-for-log writer-port writer-log))
      rt/coord-assert-for-log
      (fn [_port _log te predicate value base]
        (original-assert-for-log
         writer-port writer-log te predicate value base))
      rt/coord-retract-for-log
      (fn [_port _log te predicate value base]
        (original-retract-for-log
         writer-port writer-log te predicate value base))
      rt/coord-request-for-log
      (fn [_port _log request]
        (original-request-for-log writer-port writer-log request))]
     (apply main/-main args))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
