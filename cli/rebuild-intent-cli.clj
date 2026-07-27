#!/usr/bin/env bb
;; Durable rebuild coordination: one intent fact, finite live-roster broadcast,
;; bounded response hold, and CAS-closed all-clear/rebuild terminal markers.
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "..")))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/message-contract.clj"))
(load-file (str root "/cli/message-audience.clj"))
(load-file (str root "/cli/rebuild_intent_state.clj"))

(def port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977")))
(def intent-id-pattern
  #"^(?:@?rebuild-intent:)?([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$")
(def coordinator-retry-attempts 12)
(def max-intents 64)

(defn fail! [message]
  (binding [*out* *err*] (println (str "north rebuild-intent: " message)))
  (System/exit 2))

(defn now-ms [] (System/currentTimeMillis))

(defn canonical-json [value]
  (json/generate-string value))

(defn parse-json-map [label value]
  (let [parsed (try
                 (json/parse-string value)
                 (catch Exception error
                   (throw (ex-info (str label " is not valid JSON")
                                   {:type :malformed-intent-fact}
                                   error))))]
    (when-not (map? parsed)
      (throw (ex-info (str label " must be a JSON object")
                      {:type :malformed-intent-fact})))
    parsed))

(defn normalize-intent-id [value]
  (if-let [[_ id] (re-matches intent-id-pattern (str value))]
    id
    (throw (ex-info "intent id must be a rebuild-intent UUID"
                    {:type :invalid-intent-id :value value}))))

(defn transient-coordinator-error? [error]
  (loop [cause error]
    (cond
      (nil? cause) false
      (instance? java.io.IOException cause) true
      (re-find #"(?i)(connection refused|connection reset|broken pipe|timed out)"
               (or (.getMessage ^Throwable cause) "")) true
      :else (recur (.getCause ^Throwable cause)))))

(defn intent-subject [value]
  (str "@rebuild-intent:" (normalize-intent-id value)))

(defn retry-coordinator [operation]
  (loop [attempt 1
         delay-ms 250]
    (let [result (try
                   {:value (operation)}
                   (catch Exception error {:error error}))]
      (if-let [error (:error result)]
        (if (and (transient-coordinator-error? error)
                 (< attempt coordinator-retry-attempts))
          (do
            (binding [*out* *err*]
              (println
               (str "north rebuild-intent: coordinator unavailable; retry "
                    attempt "/" coordinator-retry-attempts " in " delay-ms "ms")))
            (Thread/sleep delay-ms)
            (recur (inc attempt) (min 4000 (* 2 delay-ms))))
          (throw error))
        (:value result)))))

(defn only-value [subject predicate]
  (let [values (north.coord/many port subject predicate)]
    (when (> (count values) 1)
      (throw (ex-info (str subject " has conflicting " predicate " facts")
                      {:type :ambiguous-intent-state
                       :subject subject :predicate predicate :values values})))
    (first values)))

(defn decode-initial [subject value]
  (let [m (parse-json-map "rebuild_intent" value)]
    (north.rebuild-intent-state/new-intent
     {:id (normalize-intent-id subject)
      :who (get m "who")
      :why (get m "why")
      :planned-window (get m "plannedWindow")
      :created-at-ms (get m "createdAtMs")
      :hold-seconds (get m "holdSeconds")
      :max-delay-seconds (get m "maxDelaySeconds")})))

(defn decode-response [value]
  (let [m (parse-json-map "rebuild_response" value)
        type (case (get m "type") "batch" :batch "hold" :hold nil)]
    {:event-id (get m "eventId")
     :type type
     :from (get m "from")
     :what (get m "what")
     :reason (get m "reason")
     :eta-seconds (get m "etaSeconds")
     :received-at-ms (get m "receivedAtMs")}))

(defn load-intent [subject]
  (let [initial (only-value subject "rebuild_intent")]
    (when-not initial
      (throw (ex-info (str "no rebuild intent at " subject)
                      {:type :intent-not-found :subject subject})))
    (let [responses (->> (north.coord/many port subject "rebuild_response")
                         (map decode-response)
                         (sort-by (juxt :received-at-ms :event-id)))
          state (reduce north.rebuild-intent-state/apply-response
                        (decode-initial subject initial)
                        responses)
          all-clear (only-value subject "all_clear")
          started (only-value subject "rebuild_started")
          outcome (only-value subject "rebuild_outcome")
          state (if all-clear
                  (assoc state :phase :all-clear
                               :all-clear-at-ms
                               (get (parse-json-map "all_clear" all-clear)
                                    "atMs"))
                  state)
          state (if started
                  (north.rebuild-intent-state/mark-rebuild-started
                   state (get (parse-json-map "rebuild_started" started) "atMs"))
                  state)]
      (if outcome
        (let [m (parse-json-map "rebuild_outcome" outcome)
              at (get m "atMs")
              report (get m "report")]
          (case (get m "status")
            "deployment-verified"
            (north.rebuild-intent-state/mark-deployment-verified state at report)
            "failed"
            (north.rebuild-intent-state/mark-failed state at report)
            (throw (ex-info "rebuild_outcome has an unknown status"
                            {:type :malformed-intent-fact}))))
        state))))

(defn assert-batch! [subject facts]
  (let [response
        (north.coord/send-op
         port
         {:op :assert-batch
          :te subject
          :facts (mapv (fn [[predicate value]]
                         {:p predicate :r (str value)})
                       facts)})]
    (when-not (:ok response)
      (throw (ex-info "coordinator rejected atomic rebuild-intent publication"
                      {:type :intent-publication-rejected :response response})))
    response))

(defn ensure-schema! []
  (doseq [predicate ["rebuild_intent" "all_clear"
                     "rebuild_started" "rebuild_outcome"]]
    (north.coord/put! port (str "@" predicate) "cardinality" "single")))

(defn broadcast! [intent-id phase requested-by body]
  (let [subject (str "@msg:rebuild-intent:" intent-id ":" phase)
        sender "rebuild-intent"
        problem
        (north.message-contract/input-problem
         sender north.message-audience/broadcast-address phase body)]
    (when problem
      (throw (ex-info (str "rebuild broadcast rejected: " problem)
                      {:type :invalid-rebuild-broadcast})))
    ;; A synthetic sender intentionally makes the finite snapshot include the
    ;; requesting session too: this protocol says ALL live roster sessions.
    (north.message-audience/snapshot-broadcast! port subject sender)
    (assert-batch!
     subject
     [["from" sender]
      ["subject" phase]
      ["body" body]
      ["sent_at" (str (java.time.Instant/now))]
      ["requested_by" requested-by]
      ["to" north.message-audience/broadcast-address]])
    subject))

(defn parse-start-options [args]
  (loop [remaining args
         options {:hold-seconds north.rebuild-intent-state/default-hold-seconds
                  :max-delay-seconds
                  north.rebuild-intent-state/default-max-delay-seconds}]
    (if (empty? remaining)
      options
      (let [[flag value & more] remaining]
        (when-not value (fail! (str flag " requires a value")))
        (case flag
          "--who" (recur more (assoc options :who value))
          "--why" (recur more (assoc options :why value))
          "--window" (recur more (assoc options :planned-window value))
          "--hold" (recur more
                            (assoc options :hold-seconds
                                   (north.rebuild-intent-state/parse-duration-seconds
                                    value)))
          "--max-delay" (recur more
                                 (assoc options :max-delay-seconds
                                        (north.rebuild-intent-state/parse-duration-seconds
                                         value)))
          (fail! (str "unknown start flag " flag)))))))

(defn current-who []
  (or (System/getenv "NORTH_AGENT_ID")
      (System/getenv "AGENT_ID")
      (System/getenv "NORTH_AUTHOR")
      "tom_passarelli"))

(defn start! [args]
  (let [options (parse-start-options args)
        id (str (java.util.UUID/randomUUID))
        created (now-ms)
        state
        (north.rebuild-intent-state/new-intent
         {:id id
          :who (or (:who options) (current-who))
          :why (:why options)
          :planned-window (or (:planned-window options)
                              "immediately after coordination all-clear")
          :created-at-ms created
          :hold-seconds (:hold-seconds options)
          :max-delay-seconds (:max-delay-seconds options)})
        initial
        (canonical-json
         (sorted-map
          "version" north.rebuild-intent-state/protocol-version
          "who" (:who state)
          "why" (:why state)
          "plannedWindow" (:planned-window state)
          "createdAtMs" created
          "createdAt" (north.rebuild-intent-state/millis->instant created)
          "holdSeconds" (:hold-seconds state)
          "maxDelaySeconds" (:max-delay-seconds state)))
        subject (intent-subject id)
        response-help
        (str "Rebuild requested by " (:who state) ": " (:why state)
             ". Planned window: " (:planned-window state)
             ". Hold until " (north.rebuild-intent-state/millis->instant
                              (:deadline-ms state))
             "; bounded maximum "
             (north.rebuild-intent-state/millis->instant
              (:max-deadline-ms state))
             ". Respond: north rebuild-intent batch-with-me " id
             " \"<pending change>\" OR north rebuild-intent hold " id
             " \"<reason>\" <eta e.g. 5m>. Silence means all-clear.")]
    (retry-coordinator
     #(do
        (ensure-schema!)
        (assert-batch! subject [["kind" "rebuild-intent"]
                                ["rebuild_intent" initial]])
        (broadcast! id "rebuild-intent" (:who state) response-help)))
    (println id)))

(defn active-intent-subject []
  (let [response
        (north.coord/indexed-query
         port
         {:find "intent"
          :rules [{:head {:rel "intent" :args [{:var "e"}]}
                   :body [{:rel "triple"
                           :args [{:var "e"} "rebuild_intent" {:var "v"}]}]}]}
         max-intents)
        open (->> (:ok response)
                  (map first)
                  distinct
                  (keep (fn [subject]
                          (let [state (load-intent subject)]
                            (when (north.rebuild-intent-state/response-open?
                                   state (now-ms))
                              subject))))
                  vec)]
    (case (count open)
      1 (first open)
      0 (throw (ex-info "no rebuild intent currently accepts responses"
                        {:type :no-active-intent}))
      (throw (ex-info "multiple rebuild intents accept responses; pass the intent id"
                      {:type :ambiguous-active-intent :subjects open})))))

(defn response-subject-and-args [args required-after-id]
  (let [explicit? (boolean (and (seq args)
                                (re-matches intent-id-pattern (str (first args)))))
        subject (if explicit?
                  (intent-subject (first args))
                  (active-intent-subject))
        body (vec (if explicit? (rest args) args))]
    (when (< (count body) required-after-id)
      (fail! "response arguments are incomplete"))
    [subject body]))

(defn record-response! [subject response]
  (let [encoded
        (canonical-json
         (cond-> (sorted-map
                  "eventId" (:event-id response)
                  "type" (name (:type response))
                  "from" (:from response)
                  "receivedAtMs" (:received-at-ms response)
                  "receivedAt"
                  (north.rebuild-intent-state/millis->instant
                   (:received-at-ms response)))
           (:what response) (assoc "what" (:what response))
           (:reason response) (assoc "reason" (:reason response))
           (:eta-seconds response) (assoc "etaSeconds" (:eta-seconds response))))
        result
        (north.coord/assert-after-read!
         port subject "rebuild_response" encoded
         #(north.rebuild-intent-state/apply-response
           (load-intent subject) response))]
    (when (:reject result)
      (throw (ex-info "response conflicted until the retry deadline"
                      {:type :response-conflict :response result})))
    (load-intent subject)))

(defn batch! [args]
  (let [[subject body] (response-subject-and-args args 1)
        what (str/join " " body)
        response {:event-id (str (java.util.UUID/randomUUID))
                  :type :batch
                  :from (current-who)
                  :what what
                  :received-at-ms (now-ms)}
        state (retry-coordinator #(record-response! subject response))]
    (println
     (str "batched " (normalize-intent-id subject) ": " what
          " · deadline "
          (north.rebuild-intent-state/millis->instant (:deadline-ms state))))))

(defn hold! [args]
  (let [[subject body] (response-subject-and-args args 2)
        eta (last body)
        reason (str/join " " (butlast body))
        response {:event-id (str (java.util.UUID/randomUUID))
                  :type :hold
                  :from (current-who)
                  :reason reason
                  :eta-seconds
                  (north.rebuild-intent-state/parse-duration-seconds eta)
                  :received-at-ms (now-ms)}
        state (retry-coordinator #(record-response! subject response))]
    (println
     (str "held " (normalize-intent-id subject) ": " reason
          " · effective deadline "
          (north.rebuild-intent-state/millis->instant (:deadline-ms state))))))

(defn commit-all-clear! [subject at-ms]
  (let [marker (canonical-json
                (sorted-map "atMs" at-ms
                            "at" (north.rebuild-intent-state/millis->instant at-ms)))
        result
        (north.coord/assert-after-read!
         port subject "all_clear" marker
         (fn []
           (let [current (load-intent subject)]
             (when-not (= :all-clear
                          (:phase
                           (north.rebuild-intent-state/advance current at-ms)))
               (throw (ex-info "hold window remains open"
                               {:type :hold-window-open
                                :deadline-ms (:deadline-ms current)}))))))]
    (when (:reject result)
      (throw (ex-info "all-clear conflicted until the retry deadline"
                      {:type :all-clear-conflict :response result})))
    marker))

(defn await! [args]
  (let [[id & options] args
        _ (when-not id (fail! "await requires an intent id"))
        poll-ms
        (if (seq options)
          (do
            (when-not (and (= "--poll-ms" (first options))
                           (= 2 (count options))
                           (re-matches #"[1-9][0-9]*" (second options)))
              (fail! "await accepts only --poll-ms <positive integer>"))
            (parse-long (second options)))
          1000)
        subject (intent-subject id)]
    (loop []
      (let [state (retry-coordinator #(load-intent subject))
            now (now-ms)]
        (case (:phase state)
          :holding
          (if (< now (:deadline-ms state))
            (do
              (Thread/sleep
               (long (max 1 (min poll-ms (- (:deadline-ms state) now)))))
              (recur))
            (do
              (retry-coordinator #(commit-all-clear! subject now))
              (retry-coordinator
               #(broadcast!
                 (normalize-intent-id subject) "rebuild-all-clear" (:who state)
                 (str "All-clear for rebuild intent "
                      (normalize-intent-id subject) "; "
                      (count (:responses state)) " response(s), including "
                      (count (filter (comp #{:batch} :type) (:responses state)))
                      " batched change(s).")))
              (println (str "all-clear " (normalize-intent-id subject)))))

          :all-clear
          (do
            (retry-coordinator
             #(broadcast!
               (normalize-intent-id subject) "rebuild-all-clear" (:who state)
               (str "All-clear for rebuild intent "
                    (normalize-intent-id subject) "; "
                    (count (:responses state)) " response(s), including "
                    (count (filter (comp #{:batch} :type) (:responses state)))
                    " batched change(s).")))
            (println (str "all-clear " (normalize-intent-id subject))))

          (throw (ex-info "intent is already past all-clear"
                          {:type :invalid-await :phase (:phase state)})))))))

(defn mark-started! [id]
  (let [subject (intent-subject id)
        at (now-ms)
        marker (canonical-json
                (sorted-map "atMs" at
                            "at" (north.rebuild-intent-state/millis->instant at)))
        result
        (try
          (retry-coordinator
           #(north.coord/assert-after-read!
             port subject "rebuild_started" marker
             (fn []
               (let [state (load-intent subject)]
                 (if (= :rebuilding (:phase state))
                   (throw (ex-info "rebuild already started"
                                   {:type :rebuild-already-started}))
                   (north.rebuild-intent-state/mark-rebuild-started state at))))))
          (catch clojure.lang.ExceptionInfo error
            (if (= :rebuild-already-started (:type (ex-data error)))
              {:already true}
              (throw error))))]
    (when (:reject result)
      (throw (ex-info "rebuild-started conflicted until the retry deadline"
                      {:type :rebuild-start-conflict :response result})))
    (println (str "rebuild-started " (normalize-intent-id subject)))))

(defn report-outcome! [id status report]
  (let [subject (intent-subject id)
        at (now-ms)
        marker
        (canonical-json
         (sorted-map "status" status
                     "report" report
                     "atMs" at
                     "at" (north.rebuild-intent-state/millis->instant at)))
        result
        (try
          (retry-coordinator
           #(north.coord/assert-after-read!
             port subject "rebuild_outcome" marker
             (fn []
               (let [state (load-intent subject)
                     expected-phase
                     (if (= status "deployment-verified")
                       :deployment-verified
                       :failed)
                     existing-report
                     (if (= status "deployment-verified")
                       (:deployment-report state)
                       (:failure-report state))]
                 (if (= expected-phase (:phase state))
                   (if (= report existing-report)
                     (throw (ex-info "rebuild outcome already recorded"
                                     {:type :rebuild-outcome-already-recorded}))
                     (throw (ex-info "rebuild outcome conflicts with the recorded report"
                                     {:type :rebuild-outcome-conflict})))
                   (if (= status "deployment-verified")
                     (north.rebuild-intent-state/mark-deployment-verified
                      state at report)
                     (north.rebuild-intent-state/mark-failed
                      state at report)))))))
          (catch clojure.lang.ExceptionInfo error
            (if (= :rebuild-outcome-already-recorded (:type (ex-data error)))
              {:already true}
              (throw error))))]
    (when (:reject result)
      (throw (ex-info "rebuild outcome conflicted until the retry deadline"
                      {:type :rebuild-outcome-conflict :response result})))
    (retry-coordinator
     #(broadcast!
       (normalize-intent-id subject)
       (str "rebuild-" status)
       (:who (load-intent subject))
       (str "Rebuild " status " for intent "
            (normalize-intent-id subject) ": " report)))
    (println (str status " " (normalize-intent-id subject)))))

(defn show! [id]
  (let [state (retry-coordinator #(load-intent (intent-subject id)))]
    (println
     (canonical-json
      (-> state
          (update :phase name)
          (update :responses
                  (fn [responses]
                    (mapv #(update % :type name) responses))))))))

(def usage
  (str
   "usage:\n"
   "  north rebuild-intent start --why <text> [--who <session>] [--window <text>]"
   " [--hold 3m] [--max-delay 15m]\n"
   "  north rebuild-intent batch-with-me [<intent-id>] <pending-change>\n"
   "  north rebuild-intent hold [<intent-id>] <reason> <eta>\n"
   "  north rebuild-intent await <intent-id>\n"
   "  north rebuild-intent mark-started <intent-id>\n"
   "  north rebuild-intent deployment-verified <intent-id> <report>\n"
   "  north rebuild-intent failed <intent-id> <report>\n"
   "  north rebuild-intent show <intent-id>"))

(when-not (= "1" (System/getProperty "north.rebuild-intent-cli.lib"))
  (try
    (let [[command & args] *command-line-args*]
      (case command
        "start" (start! args)
        "batch-with-me" (batch! args)
        "hold" (hold! args)
        "await" (await! args)
        "mark-started"
        (if (= 1 (count args)) (mark-started! (first args)) (fail! usage))
        "deployment-verified"
        (if (>= (count args) 2)
          (report-outcome! (first args) "deployment-verified"
                           (str/join " " (rest args)))
          (fail! usage))
        "failed"
        (if (>= (count args) 2)
          (report-outcome! (first args) "failed"
                           (str/join " " (rest args)))
          (fail! usage))
        "show"
        (if (= 1 (count args)) (show! (first args)) (fail! usage))
        (do (println usage) (when command (System/exit 2)))))
    (catch clojure.lang.ExceptionInfo error
      (fail! (.getMessage error)))
    (catch Exception error
      (fail! (or (.getMessage error) (str (class error)))))))
