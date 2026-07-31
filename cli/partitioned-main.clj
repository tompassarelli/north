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

(defn coordination-snapshot [_port _log]
  ;; Stage A gives each origin its own writer, but the compatibility daemon's
  ;; warm store still contains the pre-partition union. Human clock authority
  ;; therefore comes from the exact physical coordination origin.
  (fold/fold
   (rt/read-log (north.coord/expected-log))))

(defn coordination-live-facts [port log]
  (:facts (coordination-snapshot port log)))

(defn triples-index [triples]
  (kernel/build-index
   (mapv (fn [[subject predicate value]]
           (kernel/->Fact subject predicate value))
         triples)))

(defn fact-values [idx subject predicate]
  (vec (kernel/many-i idx subject predicate)))

(defn required-value [idx subject predicate]
  (let [values (fact-values idx subject predicate)]
    (when-not (= 1 (count values))
      (throw
       (ex-info "legacy clock bridge requires one exact value"
                {:subject subject :predicate predicate :values values})))
    (let [value (first values)]
      (when (str/blank? value)
        (throw
         (ex-info "legacy clock bridge rejects blank values"
                  {:subject subject :predicate predicate})))
      value)))

(def canonical-start-formatter
  (.withResolverStyle
   (java.time.format.DateTimeFormatter/ofPattern
    "uuuu-MM-dd'T'HH:mm:ss")
   java.time.format.ResolverStyle/STRICT))

(defn canonical-start-instant? [value]
  (boolean
   (and (string? value)
        (re-matches
         #"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}"
         value)
        (try
          (java.time.LocalDateTime/parse value canonical-start-formatter)
          true
          (catch Exception _ false)))))

(defn valid-session-locator? [value]
  (boolean
   (and (string? value)
        (re-matches #"^@[A-Za-z0-9][A-Za-z0-9._:-]*$" value))))

(defn valid-positive-rate? [value]
  (boolean
   (and (string? value)
        (clock/positive-rate? value)
        (let [parsed (parse-double value)]
          (and (some? parsed) (Double/isFinite parsed))))))

(defn bridge-candidate [coordination-index telemetry-index session]
  (let [owner (required-value telemetry-index session "owner")
        actor (required-value telemetry-index session "clocked_by")
        start (required-value telemetry-index session "start_time")
        thread (required-value telemetry-index session "session_of")
        rate (required-value telemetry-index session "rate")
        kind (required-value telemetry-index session "kind")]
    (when-not (= actor "user")
      (throw
       (ex-info "legacy clock bridge requires clocked_by=user"
                {:session session :actor actor})))
    (when-not (= kind "client_session")
      (throw
       (ex-info "legacy clock bridge requires kind=client_session"
                {:session session :kind kind})))
    (when-not (canonical-start-instant? start)
      (throw
       (ex-info "legacy clock bridge requires a canonical start instant"
                {:session session :start start})))
    (when-not (valid-session-locator? thread)
      (throw
       (ex-info "legacy clock bridge requires a valid session_of locator"
                {:session session :session_of thread})))
    (when-not (valid-positive-rate? rate)
      (throw
       (ex-info "legacy clock bridge requires a strictly positive rate"
                {:session session :rate rate})))
    (when (seq (fact-values telemetry-index session "end_time"))
      (throw
       (ex-info "closed legacy clocks cannot be bridged"
                {:session session})))
    (required-value coordination-index thread "title")
    (let [thread-owner (required-value coordination-index thread "owner")]
      (when-not (= owner thread-owner)
        (throw
         (ex-info "legacy clock owner conflicts with coordination thread"
                  {:session session
                   :owner owner
                   :thread thread
                   :thread-owner thread-owner}))))
    {:subject session
     :thread thread
     :owner owner
     :actor actor
     :start start
     :rate rate
     :kind kind}))

(defn bridge-facts [{:keys [thread owner actor start rate kind]}]
  [{:p "owner" :r owner}
   {:p "clocked_by" :r actor}
   {:p "start_time" :r start}
   {:p "session_of" :r thread}
   {:p "rate" :r rate}
   {:p "kind" :r kind}])

(defn existing-value-compatible? [idx subject predicate expected]
  (let [values (fact-values idx subject predicate)]
    (or (empty? values) (= values [expected]))))

(defn exact-bridge? [idx {:keys [subject] :as candidate}]
  (every?
   (fn [{:keys [p r]}]
     (= [r] (fact-values idx subject p)))
   (bridge-facts candidate)))

(defn publish-bridge! [telemetry-index session]
  (let [port (coordination-port)
        log (north.coord/expected-log)]
    (loop [attempts 16]
      (let [base-response
            (north.coord/send-op-for-log port log {:op :version})
            base (:version base-response)
            snapshot (coordination-snapshot port log)]
        (when-not (integer? base)
          (throw (ex-info "coordination clock authority is unavailable"
                          {:response base-response})))
        (if-not (= base (:version snapshot))
          (if (> attempts 1)
            (recur (dec attempts))
            (throw
             (ex-info "coordination clock snapshot did not reach the fenced version"
                      {:daemon-version base
                       :log-version (:version snapshot)})))
          (let [idx (kernel/build-index (:facts snapshot))
                candidate
                (bridge-candidate idx telemetry-index session)
                subject (:subject candidate)
                facts (bridge-facts candidate)]
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
                            {:session subject :response response})))))))))))

(defn bridge-source? [telemetry-index session]
  (and
   (empty? (fact-values telemetry-index session "end_time"))
   (or
    (some #{"client_session"}
          (fact-values telemetry-index session "kind"))
    (and
     (seq (fact-values telemetry-index session "session_of"))
     (seq (fact-values telemetry-index session "start_time"))
     (let [actors (fact-values telemetry-index session "clocked_by")]
       (or (empty? actors) (some #{"user"} actors)))))))

(defn bridge-sources [telemetry-index]
  (filterv
   (fn [session] (bridge-source? telemetry-index session))
   (:subjects telemetry-index)))

(defn maybe-bridge-legacy-clock! []
  (let [coordination-index
        (kernel/build-index
         (coordination-live-facts
          (coordination-port) (north.coord/expected-log)))]
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
                   (empty?
                    (fact-values
                     coordination-index session "end_time")))
                 (bridge-sources telemetry-index))]
            (when (> (count candidates) 1)
              (throw
               (ex-info "multiple legacy human clocks are open in telemetry"
                        {:sessions candidates})))
            (when (= 1 (count candidates))
              (publish-bridge! telemetry-index (first candidates)))))))))

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
