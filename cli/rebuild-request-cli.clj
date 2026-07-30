#!/usr/bin/env bb
;; The rebuild QUEUE surface: record a durable ask and return — never build,
;; never hold. Loadable as a library (north.rebuild-request-cli.lib=1) so the
;; reactor's window owner drives this exact read/write path; every function
;; therefore takes its port explicitly instead of reading one global.
(ns north.rebuild-request
  (:require [babashka.process :as proc]
            [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def ^:private cli-dir
  (.getParent (io/file (System/getProperty "babashka.file"))))
(def ^:private repo-root
  (.getCanonicalPath (io/file cli-dir "..")))

(load-file (str cli-dir "/coord.clj"))
(load-file (str cli-dir "/harness-state.clj"))
(load-file (str cli-dir "/rebuild_intent_state.clj"))
(load-file (str cli-dir "/rebuild_request_state.clj"))

(def subject-prefix "@rebuild-request:")
(def window-prefix "@rebuild-window:")
(def request-id-pattern #"^(?:@?rebuild-request:)?(\d{10,}-[0-9a-f]{8})$")
(def window-id-pattern #"^(?:@?rebuild-window:)?(\d{10,}-[0-9a-f]{8})$")

;; Doctor-cheap read bound: mint-time lives in the subject id (concern-cli
;; idiom), so the newest slice is selected before any per-subject fact read and
;; the remainder is REPORTED as unread, never silently dropped.
(def max-subjects 4096)
(def max-detail 256)
(def urgent-rate-period-ms (* 24 60 60 1000))
(def exact-read-attempts 4)
(def exact-read-initial-backoff-ms 250)
(def exact-read-max-backoff-ms 2000)

(def promote-root
  (or (System/getenv "NORTH_PROMOTE_ROOT") "/var/lib/north-enforcement"))

(defn now-ms [] (System/currentTimeMillis))

(defn instant [ms] (str (java.time.Instant/ofEpochMilli (long ms))))

(defn mint-id []
  (str (now-ms) "-"
       (format "%08x" (bit-and (.nextLong (java.security.SecureRandom.)) 0xffffffff))))

(defn mint-ms [id]
  (some-> (re-find #"(\d{10,})-" (str id)) second parse-long))

(defn normalize-request-id [value]
  (if-let [[_ id] (re-matches request-id-pattern (str value))]
    id
    (throw (ex-info "rebuild request id must be <epoch-ms>-<8 hex>"
                    {:type :invalid-request-id :value value}))))

(defn normalize-window-id [value]
  (if-let [[_ id] (re-matches window-id-pattern (str value))]
    id
    (throw (ex-info "rebuild window id must be <epoch-ms>-<8 hex>"
                    {:type :invalid-window-id :value value}))))

(defn request-subject [id] (str subject-prefix (normalize-request-id id)))
(defn window-subject [id] (str window-prefix (normalize-window-id id)))

(defn current-requester []
  (or (System/getenv "NORTH_AGENT_ID")
      (System/getenv "AGENT_ID")
      (System/getenv "NORTH_AUTHOR")
      "tom_passarelli"))

(defn home [] (System/getenv "HOME"))

(defn coordination-on? []
  (= "on" (north.harness-state/get-value (home) "rebuild-coordination" "off")))

(defn window-seconds
  "Configured coalescing window, `north config rebuild-window`. A malformed
   stored value falls back to the default rather than breaking the sweep."
  []
  (let [raw (north.harness-state/get-value
             (home) "rebuild-window"
             (str north.rebuild-request-state/default-window-seconds "s"))]
    (try
      (north.rebuild-intent-state/parse-duration-seconds raw)
      (catch Exception _ north.rebuild-request-state/default-window-seconds))))

;; ---- reads ------------------------------------------------------------------

(defn- parse-json-map [label value]
  (let [parsed (try (json/parse-string (str value))
                    (catch Exception error
                      (throw (ex-info (str label " is not valid JSON")
                                      {:type :malformed-request-fact} error))))]
    (when-not (map? parsed)
      (throw (ex-info (str label " must be a JSON object")
                      {:type :malformed-request-fact})))
    parsed))

(defn- only-value [port subject predicate]
  (first (north.coord/many port subject predicate)))

(def ^:private retryable-exact-read-types
  #{:coordinator-response-timeout
    :coordinator-response-closed
    :coordinator-response-truncated})

(defn- retryable-exact-read-error? [error]
  (boolean
   (some (fn [cause]
           (or (instance? java.net.ConnectException cause)
               (instance? java.net.SocketTimeoutException cause)
               (instance? java.net.SocketException cause)
               (instance? java.io.EOFException cause)
               (contains? retryable-exact-read-types (:type (ex-data cause)))))
         (take-while some? (iterate #(.getCause ^Throwable %) error)))))

(defn- exact-read-with-retry [operation]
  (loop [attempt 1
         backoff-ms exact-read-initial-backoff-ms]
    (let [result (try
                   {:value (operation)}
                   (catch Exception error {:error error}))]
      (if-let [error (:error result)]
        (if (and (< attempt exact-read-attempts)
                 (retryable-exact-read-error? error))
          (do
            (binding [*out* *err*]
              (println (str "north rebuild: exact coordinator read unavailable; retry "
                            attempt "/" exact-read-attempts " in " backoff-ms "ms")))
            (Thread/sleep backoff-ms)
            (recur (inc attempt)
                   (min exact-read-max-backoff-ms (* 2 backoff-ms))))
          (throw error))
        (:value result)))))

;; Window execution already knows each exact subject; keep it off the global query cache.
(defn- subject-facts [port subject]
  (reduce (fn [facts [predicate value]]
            (update facts predicate (fnil conj []) value))
          {}
          (exact-read-with-retry #(north.coord/show-rows port subject))))

;; ONE indexed subject+value query per predicate. A per-subject `many` read
;; measured ~150ms on the live corpus, so fanning 70 of them out cost 10s —
;; a doctor probe cannot be built out of per-subject round trips.
(defn pairs-with [port predicate limit]
  (let [response
        (north.coord/indexed-query
         port
         {:find "row"
          :rules [{:head {:rel "row" :args [{:var "e"} {:var "v"}]}
                   :body [{:rel "triple"
                           :args [{:var "e"} predicate {:var "v"}]}]}]}
         limit)]
    (->> (:ok response)
         (filter #(and (vector? %) (= 2 (count %))))
         vec)))

(defn- index-by-subject [pairs]
  (reduce (fn [acc [subject value]] (if (contains? acc subject) acc (assoc acc subject value)))
          {} pairs))

(defn- decode-request-facts [subject raw satisfied-raw]
  (let [m (parse-json-map "rebuild_request" raw)
        satisfied (some->> satisfied-raw (parse-json-map "rebuild_request_satisfied"))]
    (cond-> {:id (subs subject (count subject-prefix))
             :subject subject
             :version (get m "version")
             :requester (get m "requester")
             :why (get m "why")
             :thread (get m "thread")
             :created-at-ms (get m "createdAtMs")
             :urgent (boolean (get m "urgent"))}
      (get m "urgentReason") (assoc :urgent-reason (get m "urgentReason"))
      satisfied (assoc :satisfied
                       {:intent (get satisfied "intent")
                        :generation (get satisfied "generation")
                        :at-ms (get satisfied "atMs")}))))

(defn decode-request
  "Single-subject decode — one exact show supplies both request and settlement."
  [port subject]
  (let [facts (subject-facts port subject)]
    (when-let [raw (first (get facts "rebuild_request"))]
      (decode-request-facts
       subject raw (first (get facts "rebuild_request_satisfied"))))))

(defn load-requests
  "The newest `max-detail` requests, decoded, plus how many older subjects were
   left unread. Ordered oldest-first so window composition follows arrival."
  [port]
  (let [requests (pairs-with port "rebuild_request" max-subjects)
        satisfied (index-by-subject (pairs-with port "rebuild_request_satisfied" max-subjects))
        ordered (->> requests
                     (sort-by (fn [[subject _]] (or (mint-ms subject) 0)))
                     reverse
                     vec)
        head (take max-detail ordered)]
    {:requests (->> head
                    (keep (fn [[subject raw]]
                            (try (decode-request-facts subject raw (get satisfied subject))
                                 (catch Exception _ nil))))
                    (sort-by :created-at-ms)
                    vec)
     :unread-older (max 0 (- (count ordered) (count head)))}))

(defn load-window-records
  "Window-owner records, newest first: {:id :at-ms :action :requests}."
  [port]
  (let [requests (reduce (fn [acc [subject value]] (update acc subject (fnil conj []) value))
                         {} (pairs-with port "window_request" max-subjects))]
    (->> (pairs-with port "window_action" max-subjects)
         (sort-by (fn [[subject _]] (or (mint-ms subject) 0)))
         reverse
         (take max-detail)
         (mapv (fn [[subject action]]
                 {:id (subs subject (count window-prefix))
                  :subject subject
                  :at-ms (mint-ms subject)
                  :action action
                  :requests (get requests subject [])})))))

(defn load-window-record
  "Load one claimed window through its exact subject, without a global query."
  [port id]
  (let [id (normalize-window-id id)
        subject (str window-prefix id)
        facts (subject-facts port subject)]
    (when-let [action (first (get facts "window_action"))]
      {:id id
       :subject subject
       :at-ms (mint-ms subject)
       :action action
       :requests (get facts "window_request" [])})))

(defn last-fired-window-ms
  "When the owner last CONSUMED a window. A deferred launch never consumes one."
  [port]
  ;; `(reduce max nil times)` reads as "no windows yet -> nil" but seeds the
  ;; reduction WITH nil, so the first real record NPEs. Empty must be the only
  ;; nil-producing case.
  (let [times (->> (load-window-records port)
                   (filter #(= "fired" (:action %)))
                   (keep :at-ms))]
    (when (seq times) (apply max times))))

(defn intent-creation-times
  "createdAtMs of every recorded rebuild intent — the only durable trace a
   coordinated rebuild leaves."
  [port]
  (->> (pairs-with port "rebuild_intent" max-subjects)
       (keep (fn [[_ raw]]
               (try (get (parse-json-map "rebuild_intent" raw) "createdAtMs")
                    (catch Exception _ nil))))
       vec))

;; ---- writes -----------------------------------------------------------------

;; Declare-if-absent: a schema write invalidates the coordinator's whole query
;; cache, so skip it when the exact-subject read already shows it declared.
(defn ensure-schema! [port]
  (doseq [predicate ["rebuild_request" "rebuild_request_urgent"
                     "rebuild_request_satisfied" "window_action"
                     "window_intent" "window_generation"]]
    (when-not (= "single" (north.coord/resolved port (str "@" predicate) "cardinality"))
      (north.coord/put! port (str "@" predicate) "cardinality" "single"))))

(defn assert-batch! [port subject facts]
  (let [response (north.coord/send-op
                  port
                  {:op :assert-batch
                   :te subject
                   :facts (mapv (fn [[predicate value]] {:p predicate :r (str value)}) facts)})]
    (when-not (:ok response)
      (throw (ex-info "coordinator rejected the rebuild-request publication"
                      {:type :request-publication-rejected :response response})))
    response))

(defn record-request!
  "Assert one durable ask atomically and return its id. Never builds, never holds."
  [port {:keys [requester why thread urgent-reason]}]
  (let [id (mint-id)
        created (or (mint-ms id) (now-ms))
        request (north.rebuild-request-state/new-request
                 {:id id :requester requester :why why :thread thread
                  :created-at-ms created :urgent-reason urgent-reason})
        payload (json/generate-string
                 (cond-> (sorted-map
                          "version" north.rebuild-request-state/protocol-version
                          "requester" (:requester request)
                          "why" (:why request)
                          "createdAtMs" created
                          "createdAt" (instant created)
                          "urgent" (:urgent request))
                   thread (assoc "thread" thread)
                   urgent-reason (assoc "urgentReason" urgent-reason)))
        facts (cond-> [["kind" "rebuild-request"]
                       ["rebuild_request" payload]]
                thread (conj ["thread" thread])
                urgent-reason
                (conj ["rebuild_request_urgent"
                       (json/generate-string
                        (sorted-map "reason" urgent-reason
                                    "atMs" created
                                    "at" (instant created)))]))]
    (ensure-schema! port)
    (assert-batch! port (str subject-prefix id) facts)
    id))

(defn mark-satisfied!
  "Close one request against the generation that actually landed. Idempotent."
  [port id {:keys [intent generation]}]
  (let [subject (request-subject id)
        at (now-ms)]
    (when-not (only-value port subject "rebuild_request")
      (throw (ex-info (str "no rebuild request at " subject)
                      {:type :request-not-found :subject subject})))
    (north.coord/put! port subject "rebuild_request_satisfied"
                      (json/generate-string
                       (cond-> (sorted-map "atMs" at "at" (instant at)
                                           "generation" (str generation))
                         intent (assoc "intent" intent))))
    subject))

(defn open-window!
  "Claim the window BEFORE anything fires: the record carries the exact request
   set the launched rebuild is answering, so a crash mid-rebuild cannot silently
   re-collect them on the next sweep."
  [port request-ids]
  (let [id (mint-id)
        subject (str window-prefix id)]
    (ensure-schema! port)
    (assert-batch! port subject
                   (into [["kind" "rebuild-window"]
                          ["window_action" "launching"]
                          ["window_run_at" (instant (or (mint-ms id) (now-ms)))]]
                         (map (fn [request-id] ["window_request" request-id]) request-ids)))
    id))

(defn set-window-action! [port id action]
  (north.coord/put! port (window-subject id) "window_action" action))

;; ---- generation identity ----------------------------------------------------

(defn current-generation
  "The store path the system profile currently points at. Best-effort: identity
   we cannot resolve is reported as unknown, never guessed."
  []
  (try
    (let [path (or (System/getenv "NORTH_CURRENT_SYSTEM") "/run/current-system")
          file (io/file path)]
      (if (.exists file) (.getCanonicalPath file) "unknown"))
    (catch Exception _ "unknown")))

;; Absence of the (unlanded) promote lane is a STATE, never an error: doctor
;; must stay renderable before the infra exists.
(defn promote-status []
  (let [current (io/file promote-root "active" "current")]
    (if (.exists current)
      {:available true
       :path (.getPath current)
       :note "promote root present; drift comparison lands with the promote lane"}
      {:available false
       :path (.getPath current)
       :note "promote infra not yet deployed"})))

;; ---- window owner -----------------------------------------------------------

(defn plan-window [port]
  (let [{:keys [requests unread-older]} (load-requests port)]
    (assoc (north.rebuild-request-state/window-plan
            {:now-ms (now-ms)
             :last-window-ms (last-fired-window-ms port)
             :window-ms (* 1000 (window-seconds))
             :requests requests
             :coordination-on? (coordination-on?)})
           :unread-older unread-older)))

(defn run-window!
  "Execute one claimed window through the mutexed rebuild/readiness path, then
   close every request the window claimed against the landed generation. Runs
  OUTSIDE the reactor sweep (a rebuild outlives the sweep's bounded lifecycle),
  so it is a verb rather than an inline call."
  [port window-id]
  (let [record (load-window-record port window-id)
        _ (when-not record
            (throw (ex-info (str "no rebuild window " window-id)
                            {:type :window-not-found})))
        requests (->> (:requests record)
                      (keep #(decode-request port (request-subject %)))
                      vec)
        why (north.rebuild-request-state/compose-why requests)
        result (proc/shell {:out :string :err :string :continue true}
                           (str repo-root "/bin/firn-rebuild-coordinated")
                           "--automatic" "--why" why)
        out (str (:out result) (:err result))
        intent (second (re-find #"rebuild intent ([0-9a-f-]{36})" out))]
    (print out)
    (flush)
    (if (zero? (:exit result))
      (let [generation (current-generation)]
        (doseq [request requests]
          (mark-satisfied! port (:id request) {:intent intent :generation generation}))
        (when intent (north.coord/put! port (window-subject window-id) "window_intent" intent))
        (north.coord/put! port (window-subject window-id) "window_generation" generation)
        (set-window-action! port window-id "fired")
        (println (str "rebuild window " window-id " fired · " (count requests)
                      " request(s) satisfied · generation " generation))
        0)
      (do
        (set-window-action! port window-id "failed")
        (binding [*out* *err*]
          (println (str "north rebuild: window " window-id " failed (rc "
                        (:exit result) "); requests remain open")))
        (:exit result)))))

;; ---- health projection ------------------------------------------------------

(defn activation-health [port]
  (let [now (now-ms)
        window-ms (* 1000 (window-seconds))
        {:keys [requests unread-older]} (load-requests port)
        open (north.rebuild-request-state/open-requests requests)
        last-window (first (load-window-records port))]
    {"version" "north:rebuild-activation-health:v1"
     "nowMs" now
     "coordinationOn" (coordination-on?)
     "windowSeconds" (window-seconds)
     "unreadOlder" unread-older
     "openCount" (count open)
     "open" (mapv (fn [r]
                    (let [age (north.rebuild-request-state/age-ms r now)]
                      (cond-> {"id" (:id r)
                               "requester" (:requester r)
                               "why" (:why r)
                               "urgent" (:urgent r)
                               "ageMs" age
                               "age" (north.rebuild-request-state/humanize-age age)}
                        (:thread r) (assoc "thread" (:thread r)))))
                  open)
     "gauge" (let [g (north.rebuild-request-state/rebuild-gauge
                      (intent-creation-times port) now window-ms)]
               {"count" (:count g) "threshold" (:threshold g) "breached" (:breached? g)})
     "urgent" (let [u (north.rebuild-request-state/urgent-rate
                       requests now urgent-rate-period-ms)]
                {"total" (:total u) "urgent" (:urgent u) "rate" (:rate u)
                 "periodHours" (quot urgent-rate-period-ms 3600000)})
     "lastWindow" (when last-window
                    {"id" (:id last-window)
                     "action" (:action last-window)
                     "ageMs" (when (:at-ms last-window) (- now (:at-ms last-window)))
                     "requests" (count (:requests last-window))})
     "promote" (let [p (promote-status)]
                 {"available" (:available p) "path" (:path p) "note" (:note p)})}))

;; ---- command line -----------------------------------------------------------

(defn fail! [message]
  (binding [*out* *err*] (println (str "north rebuild: " message)))
  (System/exit 2))

(defn parse-request-options [args]
  (loop [remaining args options {}]
    (if (empty? remaining)
      options
      (let [[flag value & more] remaining]
        (when-not value (fail! (str flag " requires a value")))
        (case flag
          "--why" (recur more (assoc options :why value))
          "--thread" (recur more (assoc options :thread value))
          "--urgent" (recur more (assoc options :urgent-reason value))
          (fail! (str "unknown request flag " flag)))))))

(defn cmd-request! [port args]
  (let [options (parse-request-options args)]
    (when-not (:why options) (fail! "--why is required"))
    (let [id (record-request! port (assoc options :requester (current-requester)))]
      (println id)
      (println (str "queued: the reactor window owner coalesces open requests into one "
                    "coordinated rebuild"
                    (when-not (coordination-on?)
                      " (rebuild-coordination is off — requests queue and report, nothing fires)")))
      (when (:urgent-reason options)
        (println "urgent recorded (never refused; counted in north doctor)")))))

(defn cmd-list [port]
  (let [now (now-ms)
        {:keys [requests unread-older]} (load-requests port)
        open (north.rebuild-request-state/open-requests requests)]
    (println (str "rebuild queue — " (count open) " open request(s) · window "
                  (window-seconds) "s · coordination "
                  (if (coordination-on?) "on" "off")))
    (if (empty? open)
      (println "  (none)")
      (doseq [r open]
        (println (format "  %-24s %-10s %-22s %s"
                         (:id r)
                         (north.rebuild-request-state/humanize-age
                          (north.rebuild-request-state/age-ms r now))
                         (str (:requester r) (when (:urgent r) " [urgent]"))
                         (:why r)))))
    (when (pos? unread-older)
      (println (str "  (" unread-older " older request subject(s) beyond the "
                    max-detail "-row read bound)")))))

(def usage
  (str
   "usage:\n"
   "  north rebuild request --why <text> [--thread <id>] [--urgent <reason>]\n"
   "      an ask that exists only to adopt north/fram/beagle code is a DEFECT:\n"
   "      tag it --why \"code-adoption: <what>\" so `north doctor` counts it,\n"
   "      then fix the delivery channel (promote, not rebuild). Target: zero.\n"
   "  north rebuild list\n"
   "  north rebuild satisfy <request-id> --generation <path> [--intent <id>]\n"
   "  north rebuild run-window <window-id>\n"
   "  north rebuild health-json"))

(when-not (= "1" (System/getProperty "north.rebuild-request-cli.lib"))
  (let [port (Integer/parseInt (or (System/getenv "NORTH_PORT") "7977"))]
    (try
      (let [[command & args] *command-line-args*]
        (case command
          "request" (cmd-request! port args)
          "list" (cmd-list port)
          "satisfy"
          (let [[id & flags] args
                options (loop [remaining flags acc {}]
                          (if (empty? remaining)
                            acc
                            (let [[flag value & more] remaining]
                              (when-not value (fail! (str flag " requires a value")))
                              (case flag
                                "--generation" (recur more (assoc acc :generation value))
                                "--intent" (recur more (assoc acc :intent value))
                                (fail! (str "unknown satisfy flag " flag))))))]
            (when-not id (fail! "satisfy requires a request id"))
            (when-not (:generation options) (fail! "satisfy requires --generation"))
            (mark-satisfied! port id options)
            (println (str "satisfied " (normalize-request-id id))))
          "run-window"
          (if (= 1 (count args))
            (System/exit (run-window! port (first args)))
            (fail! usage))
          "health-json" (println (json/generate-string (activation-health port)))
          (do (println usage) (when command (System/exit 2)))))
      (catch clojure.lang.ExceptionInfo error
        (fail! (.getMessage error)))
      (catch Exception error
        (fail! (or (.getMessage error) (str (class error))))))))
