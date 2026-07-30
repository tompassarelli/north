#!/usr/bin/env bb
;; Compatibility adapter for North's compiled Beagle CLI while telemetry has an
;; independent writer. General reads compose each writer's materialized live
;; facts; human clock authority reads and writes only coordination. Legacy
;; thread/run clock verbs remain telemetry-owned.
(require '[fram.fold :as fold]
         '[fram.kernel :as kernel]
         '[fram.rt :as rt]
         '[north.clock :as clock]
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
    (let [{:keys [facts unavailable unavailable-detail]}
          (north.coord/live-facts-view (coordination-port))]
      (when (seq unavailable)
        (binding [*out* *err*]
          ;; Name WHY, not just WHICH. "unavailable domain(s): coordination"
          ;; reads as a dead daemon and was, on 2026-07-29, a response that had
          ;; simply outgrown its size cap — with the reason already in hand and
          ;; discarded one frame up.
          (println
           (str "north: partial cross-log view; unavailable domain(s): "
                (str/join ", " unavailable)))
          (doseq [[domain reason] unavailable-detail]
            (println (str "  " domain ": " reason)))))
      (mapv (fn [[subject predicate value]]
              (kernel/->Fact subject predicate value))
            facts))))

(def telemetry-clock-verbs
  #{"start" "orphan" "sync"})

(defn telemetry-clock-command? [args]
  (and (north.coord/telemetry-partition-enabled?)
       (= "clock" (first args))
       (contains? telemetry-clock-verbs (or (second args) "current"))))

(def client-clock-authority-verbs
  #{"in" "current" "status" "out" "stop"})

(defn client-clock-authority-command? [args]
  (and (north.coord/telemetry-partition-enabled?)
       (= "clock" (first args))
       (contains? client-clock-authority-verbs
                  (or (second args) "current"))))

(defn coordination-live-facts [_port _log]
  ;; Stage A gives each origin its own writer, but the compatibility daemon's
  ;; warm store still contains the pre-partition union. Human clock authority
  ;; therefore comes from the exact physical coordination origin.
  (:facts
   (fold/fold
    (rt/read-log (north.coord/expected-log)))))

(defn triples-index [triples]
  (kernel/build-index
   (mapv (fn [[subject predicate value]]
           (kernel/->Fact subject predicate value))
         triples)))

(defn only-value [idx subject predicate]
  (let [values (vec (distinct (kernel/many-i idx subject predicate)))]
    (when (> (count values) 1)
      (throw
       (ex-info "legacy clock bridge found ambiguous single-valued facts"
                {:subject subject :predicate predicate :values values})))
    (first values)))

(defn bridge-rate [coordination-index telemetry-index session owner]
  (let [direct (only-value telemetry-index session "rate")
        thread (only-value telemetry-index session "session_of")
        historical (when thread
                     (only-value coordination-index thread "rate"))
        authority (clock/client-rate-authority coordination-index owner)]
    (or direct
        (when (and historical (clock/positive-rate? historical)) historical)
        (when (= "ok" (:status authority)) (:rate authority)))))

(defn bridge-candidate [coordination-index telemetry-index session]
  (let [thread (only-value telemetry-index session "session_of")
        owner (or (only-value telemetry-index session "owner")
                  (when thread
                    (only-value coordination-index thread "owner")))
        start (only-value telemetry-index session "start_time")
        actor (or (only-value telemetry-index session "clocked_by") "user")
        rate (when owner
               (bridge-rate coordination-index telemetry-index session owner))]
    (when-not (and (= actor "user")
                   (not (str/blank? owner))
                   (not (str/blank? start)))
      (throw
       (ex-info "legacy open human clock is incomplete and cannot be bridged"
                {:session session :owner owner :start start :actor actor})))
    {:subject session
     :thread thread
     :owner owner
     :start start
     :rate rate}))

(defn existing-value-compatible? [idx subject predicate expected]
  (let [values (vec (distinct (kernel/many-i idx subject predicate)))]
    (or (empty? values) (= values [expected]))))

(defn exact-bridge? [idx {:keys [subject thread owner start rate]}]
  (and (= "client_session" (only-value idx subject "kind"))
       (= owner (only-value idx subject "owner"))
       (= "user" (only-value idx subject "clocked_by"))
       (= start (only-value idx subject "start_time"))
       (or (nil? thread) (= thread (only-value idx subject "session_of")))
       (or (nil? rate) (= rate (only-value idx subject "rate")))))

(defn publish-bridge! [{:keys [subject thread owner start rate] :as candidate}]
  (let [port (coordination-port)
        log (north.coord/expected-log)
        facts (cond-> [{:p "owner" :r owner}
                       {:p "clocked_by" :r "user"}
                       {:p "start_time" :r start}]
                thread (conj {:p "session_of" :r thread})
                rate (conj {:p "rate" :r rate})
                true (conj {:p "kind" :r "client_session"}))]
    (loop [attempts 16]
      (let [base-response
            (north.coord/send-op-for-log port log {:op :version})
            base (:version base-response)
            live (coordination-live-facts port log)
            idx (kernel/build-index live)]
        (when-not (integer? base)
          (throw (ex-info "coordination clock authority is unavailable"
                          {:response base-response})))
        (cond
          (exact-bridge? idx candidate)
          :already-bridged

          (seq (clock/open-human-sessions idx))
          (throw
           (ex-info "legacy clock bridge found another coordination-authoritative open session"
                    {:session subject
                     :open (clock/open-human-sessions idx)}))

          (not
           (every?
            (fn [{:keys [p r]}]
              (existing-value-compatible? idx subject p r))
            facts))
          (throw
           (ex-info "legacy clock bridge conflicts with coordination facts"
                    {:session subject}))

          :else
          (let [response
                (north.coord/send-op-for-log
                 port log {:op :assert-batch-at-version
                           :te subject
                           :facts facts
                           :base base})]
            (cond
              (and (integer? (:ok response)) (:batch response))
              :bridged

              (and (= :conflict (:reject response)) (> attempts 1))
              (recur (dec attempts))

              :else
              (throw
               (ex-info "legacy clock bridge publication was rejected"
                        {:session subject :response response})))))))))

(defn maybe-bridge-legacy-clock! []
  (let [coordination-facts
        (coordination-live-facts
         (coordination-port) (north.coord/expected-log))
        coordination-index (kernel/build-index coordination-facts)]
    (when (empty? (clock/open-human-sessions coordination-index))
      (let [view
            (binding [north.coord/*request-deadline-ns*
                      (north.coord/request-deadline-ns 750)]
              (north.coord/live-facts-view (coordination-port)))
            telemetry (get-in view [:domains :telemetry])]
        (when (:available telemetry)
          (let [telemetry-index (triples-index (:facts telemetry))
                candidates
                (filterv
                 (fn [session]
                   (nil? (only-value coordination-index session "end_time")))
                 (clock/open-human-sessions telemetry-index))]
            (when (> (count candidates) 1)
              (throw
               (ex-info "multiple legacy human clocks are open in telemetry"
                        {:sessions candidates})))
            (when (= 1 (count candidates))
              (publish-bridge!
               (bridge-candidate
                coordination-index telemetry-index (first candidates))))))))))

(defn -main [& args]
  (let [authority? (client-clock-authority-command? args)
        _ (when authority? (maybe-bridge-legacy-clock!))
        telemetry-write? (telemetry-clock-command? args)
        writer-port (if telemetry-write? (telemetry-port) (original-coord-port))
        writer-log (if telemetry-write? (telemetry-log) (rt/log-path))
        live-reader (if authority? coordination-live-facts composed-live-facts)]
    (with-redefs
     [rt/coord-live-facts live-reader
      main/compose-telemetry-log? (fn [] (not authority?))
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
