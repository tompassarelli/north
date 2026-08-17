#!/usr/bin/env bb
;; One forward-only North coordination facade. Every data operation below is a
;; canonical FRAMRPC request against a SpaceId; no newline socket, EDN envelope,
;; physical-log fence, protocol selector, or stale-runtime fallback exists here.
(ns north.coord
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [store.rpc :as wire]
            [store.types :as t]))

(def cli-dir (.getParentFile (io/file *file*)))
(load-file (str cli-dir "/store-rpc-client.clj"))
(require '[north.store-rpc-client :as rpc])

(def PORT (or (System/getenv "NORTH_PORT")
              (System/getenv "BEAGLE_STORE_SERVER_PORT")
              "7977"))
(def query-page-row-limit 4096)
(def lease-max-safe-integer 9007199254740991)

(def telemetry-subject-tokens #{"run" "session" "mine" "guard_denial"})
(def ^:dynamic *operation-domain* nil)
(def ^:dynamic *request-deadline-ns* nil)

(defn telemetry-partition-enabled? []
  (= "1" (System/getenv "NORTH_TELEMETRY_PARTITION")))

(defn- port! [value]
  (let [parsed (if (integer? value) value (parse-long (str value)))]
    (when-not (and parsed (<= 1 parsed 65535))
      (throw (ex-info "FRAMRPC port must be an integer from 1 through 65535"
                      {:type :invalid-framrpc-port :value value})))
    (int parsed)))

(defn- positive-env-int [name fallback maximum]
  (let [raw (or (System/getenv name) (str fallback))
        value (when (re-matches #"[1-9][0-9]*" raw) (parse-long raw))]
    (when-not (and value (<= value maximum))
      (throw (ex-info (str name " must be an integer from 1 through " maximum)
                      {:type :invalid-framrpc-bound :name name :value raw})))
    (int value)))

(defn request-deadline-ns [timeout-ms]
  (when-not (and (integer? timeout-ms) (pos? timeout-ms))
    (throw (ex-info "coordination deadline requires positive milliseconds"
                    {:type :invalid-coordination-deadline
                     :timeout-ms timeout-ms})))
  (+ (System/nanoTime) (* 1000000 (long timeout-ms))))

(defn- remaining-timeout-ms [configured]
  (if-not *request-deadline-ns*
    configured
    (let [remaining (- (long *request-deadline-ns*) (System/nanoTime))]
      (when-not (pos? remaining)
        (throw (ex-info "coordination operation deadline exceeded"
                        {:type :coordination-operation-timeout})))
      (int (max 1 (min configured (quot (+ remaining 999999) 1000000)))))))

(defn telemetry-subject? [subject]
  (boolean
   (when (and (string? subject) (str/starts-with? subject "@"))
     (let [colon (str/index-of subject ":")
           token (when (and colon (> colon 1)) (subs subject 1 colon))]
       (contains? telemetry-subject-tokens token)))))

(defn- query-literal-subjects [query]
  (->> (concat (:rules query) (mapcat identity (:strata query)))
       (mapcat :body)
       (keep (fn [literal]
               (let [subject (first (:args literal))]
                 (when (and (= "triple" (:rel literal))
                            (string? subject)
                            (str/starts-with? subject "@"))
                   subject))))
       set))

(defn- domain-for-subject [subject]
  (if (and (telemetry-partition-enabled?) (telemetry-subject? subject))
    :telemetry
    :coordination))

(defn- domain-for-query [query]
  (let [subjects (query-literal-subjects query)]
    (if (and (seq subjects) (every? telemetry-subject? subjects)
             (telemetry-partition-enabled?))
      :telemetry
      :coordination)))

(defn- host []
  (or (not-empty (System/getenv "NORTH_FRAMRPC_HOST")) "127.0.0.1"))

(defn- domain-port [requested domain]
  (if (and (= domain :telemetry) (telemetry-partition-enabled?))
    (port! (or (System/getenv "NORTH_TELEMETRY_PORT") "7978"))
    (port! requested)))

(defn- domain-space [domain]
  (if (and (= domain :telemetry) (telemetry-partition-enabled?))
    (or (not-empty (System/getenv "NORTH_TELEMETRY_SPACE_ID"))
        "north-telemetry")
    (or (not-empty (System/getenv "BEAGLE_STORE_SPACE_ID"))
        "north-coordination")))

(defn- client-for [port domain]
  (rpc/client
   (host) (domain-port port domain) (domain-space domain)
   {:connect-timeout-ms
    (remaining-timeout-ms
     (positive-env-int "NORTH_COORD_CONNECT_TIMEOUT_MS" 1000 999999))
    :read-timeout-ms
    (remaining-timeout-ms
     (positive-env-int "NORTH_FRAMRPC_READ_TIMEOUT_MS" 30000 999999))
    :max-attempts 3 :retry-delay-ms 10 :jitter-ms 25}))

(defn- with-client! [port domain operation]
  (let [client (client-for port domain)]
    (try (operation client) (finally (rpc/close! client)))))

;; Physical paths remain data/migration identities. They never select a wire
;; protocol and are not sent to the server; SpaceId is the runtime identity.
(defn canonical-log-path [log]
  (when-not (and (string? log) (not (str/blank? log)))
    (throw (ex-info "FRAMLOG identity must be a nonblank path"
                    {:type :invalid-log-identity :log log})))
  (.getCanonicalPath (io/file log)))

(defn expected-log []
  (let [home (or (System/getenv "HOME") (System/getProperty "user.home"))]
    (canonical-log-path
     (or (System/getenv "BEAGLE_STORE_LOG")
         (str home "/.local/state/north/coordination.framlog")))))

(defn telemetry-log-path []
  (when (telemetry-partition-enabled?)
    (canonical-log-path
     (or (System/getenv "BEAGLE_STORE_TELEMETRY_LOG")
         (throw (ex-info "BEAGLE_STORE_TELEMETRY_LOG is required for partitioned telemetry"
                         {:type :missing-telemetry-log}))))))

(defn- triple-row! [triple]
  (let [row [(t/triple-t1 triple)
             (t/triple-t2 triple)
             (t/triple-t3 triple)]]
    (when-not (every? string? row)
      (throw (ex-info "North coordination data contains a non-string triple"
                      {:type :malformed-coordination-triple :row row})))
    row))

(defn write-value! [subject predicate value]
  (when-not (and (string? subject) (not (str/blank? subject)))
    (throw (ex-info "coordination write requires a nonblank string subject"
                    {:type :invalid-write :field :subject})))
  (when-not (and (string? predicate) (not (str/blank? predicate)))
    (throw (ex-info "coordination write requires a nonblank string predicate"
                    {:type :invalid-write :field :predicate})))
  (when (nil? value)
    (throw (ex-info "coordination write requires a non-nil value"
                    {:type :invalid-write :field :value})))
  (str value))

(defn- proposition! [subject predicate value]
  (t/triple subject predicate (write-value! subject predicate value)))

;; --- status and point reads -------------------------------------------------

(defn status-in-domain [port domain]
  (with-client! port domain rpc/status!))

(defn port [] (port! PORT))
(defn status [port] (status-in-domain port :coordination))

(defn cur-ver-in-domain [port domain]
  (with-client! port domain #(-> (rpc/version! %) :served-version)))

(defn cur-ver [port] (cur-ver-in-domain port :coordination))
(defn version [port] (cur-ver port))
(defn cur-ver-for-subject [port subject]
  (cur-ver-in-domain port (domain-for-subject subject)))

(defn- scan-rows! [port domain t1 t2 t3]
  (with-client!
   port domain
   (fn [client]
     (let [result (rpc/scan-all! client t1 t2 t3
                                 {:page-size rpc/effective-page-limit})]
       {:version (:served-version result)
        :rows (mapv triple-row! (:rows result))}))))

(defn show-envelope [port subject]
  (let [{:keys [version rows]}
        (scan-rows! port (domain-for-subject subject) subject nil nil)]
    {:version version
     :rows (mapv (fn [[_ predicate value]] [predicate value]) rows)}))

(defn show-rows [port subject] (:rows (show-envelope port subject)))
(defn show [port subject] (show-envelope port subject))

(defn subject-propositions [port subject]
  (mapv (fn [[predicate value]] (t/triple subject predicate value))
        (show-rows port subject)))

(defn resolved-envelope [port subject predicate]
  (let [{:keys [version rows]}
        (scan-rows! port (domain-for-subject subject) subject predicate nil)
        values (->> rows (map #(nth % 2)) distinct sort vec)
        members (count values)]
    {:value (first values)
     :members members
     :ambiguous? (> members 1)
     :values values
     :version version}))

(defn resolved [port subject predicate]
  (:value (resolved-envelope port subject predicate)))
(defn many [port subject predicate]
  (:values (resolved-envelope port subject predicate)))

;; --- typed Datalog ----------------------------------------------------------

(defn- query-argument! [argument]
  (if (map? argument)
    (if (and (= #{:var} (set (keys argument)))
             (string? (:var argument))
             (not (str/blank? (:var argument))))
      (wire/rpc-query-variable! (:var argument))
      (throw (ex-info "query variable is malformed"
                      {:type :invalid-coordination-query :argument argument})))
    (wire/rpc-query-constant! argument)))

(defn- query-relation! [{:keys [rel args neg] :as relation}]
  (when-not (and (string? rel) (not (str/blank? rel))
                 (vector? args) (or (nil? neg) (boolean? neg)))
    (throw (ex-info "query relation is malformed"
                    {:type :invalid-coordination-query :relation relation})))
  (wire/rpc-query-relation! rel (mapv query-argument! args) (boolean neg)))

(defn- query-head! [{:keys [rel args] :as head}]
  (when-not (and (string? rel) (not (str/blank? rel)) (vector? args))
    (throw (ex-info "query head is malformed"
                    {:type :invalid-coordination-query :head head})))
  (wire/rpc-query-head! rel (mapv query-argument! args)))

(defn- query-rule! [{:keys [head body] :as rule}]
  (when-not (and (map? head) (vector? body) (seq body))
    (throw (ex-info "query rule is malformed"
                    {:type :invalid-coordination-query :rule rule})))
  (wire/rpc-query-rule! (query-head! head) (mapv query-relation! body)))

(defn- query-request! [query at-version]
  (let [{:keys [find rules strata]} query
        query-strata
        (cond
          (and (vector? strata) (seq strata) (nil? rules)) strata
          (and (vector? rules) (seq rules) (nil? strata)) [rules]
          :else nil)]
    (when-not (and (string? find) (not (str/blank? find))
                   query-strata
                   (every? #(and (vector? %) (seq %)) query-strata))
      (throw (ex-info "query plan is malformed"
                      {:type :invalid-coordination-query :query query})))
    (wire/rpc-query-request!
     (wire/rpc-query-plan!
      (wire/rpc-query-find-relation! find)
      (mapv (fn [stratum]
              (wire/rpc-query-stratum! (mapv query-rule! stratum)))
            query-strata))
     (if (some? at-version)
       (wire/rpc-query-as-of! at-version)
       wire/query-current))))

(defn- string-query-rows! [rows]
  (when-not (and (vector? rows)
                 (every? #(and (vector? %) (every? string? %)) rows))
    (throw (ex-info "coordination query returned non-string rows"
                    {:type :malformed-coordination-query-response})))
  rows)

(defn valid-query-page-cursor? [cursor]
  (or (nil? cursor) (t/triple? cursor)))

(defn- logical-query-page! [client query limit cursor at-version]
  (when-not (and (integer? limit) (<= 1 limit query-page-row-limit))
    (throw (ex-info "query page limit is outside the supported range"
                    {:type :invalid-query-page-limit :limit limit})))
  (when-not (valid-query-page-cursor? cursor)
    (throw (ex-info "query cursor is not a canonical Term"
                    {:type :invalid-query-page-cursor})))
  (when-not (or (nil? at-version)
                (and (integer? at-version) (not (neg? at-version))))
    (throw (ex-info "query snapshot version is invalid"
                    {:type :invalid-query-page-version :at-version at-version})))
  (let [request (query-request! query at-version)]
    (loop [remaining limit after cursor rows [] served nil]
      (let [wire-limit (min remaining rpc/effective-page-limit)
            response (rpc/query! client request
                                 {:page (wire/rpc-page-request! wire-limit after)})
            page (:page response)
            page-rows (string-query-rows! (:rows response))
            version (or served (:served-version response))]
        (when-not page
          (throw (ex-info "query page omitted cursor metadata"
                          {:type :malformed-coordination-query-response})))
        (when-not (= version (:served-version response))
          (throw (ex-info "query page changed snapshot"
                          {:type :query-page-snapshot-changed
                           :expected version
                           :actual (:served-version response)})))
        (let [all (into rows page-rows)
              left (- remaining (count page-rows))]
          (cond
            (:done? page)
            {:rows all :served-version version :done? true :cursor nil}

            (zero? left)
            {:rows all :served-version version :done? false
             :cursor (:cursor page)}

            (or (empty? page-rows) (= after (:cursor page)))
            (throw (ex-info "query page cursor did not advance"
                            {:type :stalled-query-page-cursor}))

            :else
            (recur left (:cursor page) all version)))))))

(defn query-page-in-domain
  ([port domain query limit cursor]
   (query-page-in-domain port domain query limit cursor nil))
  ([port domain query limit cursor at-version]
   (with-client! port domain
                 #(logical-query-page! % query limit cursor at-version))))

(defn query-page
  ([port query limit cursor]
   (query-page port query limit cursor nil))
  ([port query limit cursor at-version]
   (query-page-in-domain port (domain-for-query query)
                         query limit cursor at-version)))

(defn bounded-query-in-domain [port domain query max-rows]
  (when-not (and (integer? max-rows) (pos? max-rows))
    (throw (ex-info "query row bound must be a positive integer"
                    {:type :invalid-query-row-bound :max-rows max-rows})))
  (loop [remaining max-rows cursor nil rows [] at-version nil]
    (let [limit (min remaining query-page-row-limit)
          page (query-page-in-domain port domain query limit cursor at-version)
          combined (into rows (:rows page))]
      (cond
        (:done? page)
        {:rows combined :served-version (:served-version page)}

        (= (count combined) max-rows)
        (throw (ex-info "query exceeded its row bound"
                        {:type :query-row-limit :max-rows max-rows}))

        :else
        (recur (- remaining (count (:rows page)))
               (:cursor page)
               combined
               (or at-version (:served-version page)))))))

(defn bounded-query [port query max-rows]
  (bounded-query-in-domain port (domain-for-query query) query max-rows))

(defn query-rows-in-domain [port domain query]
  (with-client!
   port domain
   (fn [client]
     (let [result (rpc/query-all! client (query-request! query nil)
                                  {:page-size rpc/effective-page-limit})]
       (string-query-rows! (:rows result))))))

(defn query-rows [port query]
  (query-rows-in-domain port (domain-for-query query) query))

(defn- occurrence-window-request! [lower-exclusive upper-inclusive]
  (let [coordinate (wire/rpc-query-variable! "coordinate")
        action (wire/rpc-query-variable! "action")
        proposition (wire/rpc-query-variable! "proposition")]
    (wire/rpc-query-request!
     (wire/rpc-query-plan!
      (wire/rpc-query-find-relation! "north_occurrence_window")
      [(wire/rpc-query-stratum!
        [(wire/rpc-query-rule!
          (wire/rpc-query-head!
           "north_occurrence_window" [coordinate action proposition])
          [(wire/rpc-query-relation!
            "occurrence" [coordinate action proposition] false)])])])
     (wire/rpc-query-since!
      lower-exclusive (wire/rpc-query-as-of! upper-inclusive)))))

(defn- occurrence-event! [[coordinate action proposition :as row]]
  (when-not (and (= 3 (count row))
                 (t/occurrence-coordinate? coordinate)
                 (contains? #{t/assert-action t/retract-action} action)
                 (t/triple? proposition))
    (throw (ex-info "occurrence window returned a malformed row"
                    {:type :malformed-occurrence-window :row row})))
  (let [[subject predicate value] (triple-row! proposition)
        transaction (t/triple-t1 coordinate)]
    {:operation (if (= action t/assert-action) :assert :retract)
     :subject subject :predicate predicate :value value
     :version (t/triple-t3 transaction)}))

(defn poll-occurrence-window-in-domain!
  [port domain lower-exclusive upper-inclusive handle!]
  (when-not (and (integer? lower-exclusive) (integer? upper-inclusive)
                 (<= 0 lower-exclusive upper-inclusive))
    (throw (ex-info "occurrence window bounds are invalid"
                    {:type :invalid-occurrence-window
                     :lower lower-exclusive :upper upper-inclusive})))
  (when (< lower-exclusive upper-inclusive)
    (with-client!
     port domain
     (fn [client]
       (let [request (occurrence-window-request!
                      lower-exclusive upper-inclusive)]
         (loop [cursor nil]
           (let [response
                 (rpc/query!
                  client request
                  {:page (wire/rpc-page-request!
                          rpc/effective-page-limit cursor)})
                 page (:page response)]
             (when-not (and page
                            (= upper-inclusive (:served-version response)))
               (throw (ex-info "occurrence window changed snapshot"
                               {:type :occurrence-window-snapshot-changed
                                :expected upper-inclusive
                                :actual (:served-version response)})))
             (doseq [row (:rows response)] (handle! (occurrence-event! row)))
             (when-not (:done? page) (recur (:cursor page)))))))))
  upper-inclusive)

(defn poll-occurrence-window! [port lower-exclusive upper-inclusive handle!]
  (poll-occurrence-window-in-domain!
   port :coordination lower-exclusive upper-inclusive handle!))

(defn occurrence-window [port lower-exclusive upper-inclusive]
  (let [events (volatile! [])]
    (poll-occurrence-window!
     port lower-exclusive upper-inclusive #(vswap! events conj %))
    {:version upper-inclusive :events @events}))

(defn- subject-query [subjects]
  {:find "north_subject_fact"
   :rules
   (mapv
    (fn [subject]
      {:head {:rel "north_subject_fact"
              :args [subject {:var "predicate"} {:var "value"}]}
       :body [{:rel "triple"
               :args [subject {:var "predicate"} {:var "value"}]}]})
    subjects)})

(defn show-many-in-domain [port domain subjects]
  (let [subjects (vec (distinct subjects))]
    (when-not (and (every? #(and (string? %) (not (str/blank? %))) subjects)
                   (<= (count subjects) query-page-row-limit))
      (throw (ex-info "show-many subjects are invalid or exceed the bound"
                      {:type :invalid-show-many-subjects})))
    (if (empty? subjects)
      {:version (cur-ver-in-domain port domain) :rows {}}
      (with-client!
       port domain
       (fn [client]
         (let [result (rpc/query-all! client (query-request! (subject-query subjects) nil)
                                      {:page-size rpc/effective-page-limit})
               rows (string-query-rows! (:rows result))]
           {:version (:served-version result)
            :rows
            (reduce (fn [by-subject [subject predicate value]]
                      (update by-subject subject conj [predicate value]))
                    (zipmap subjects (repeat [])) rows)}))))))

(defn show-many [port subjects]
  (let [groups (group-by domain-for-subject (vec (distinct subjects)))
        futures (into {} (map (fn [[domain values]]
                                [domain (future (show-many-in-domain port domain values))])
                              groups))
        results (into {} (map (fn [[domain pending]] [domain @pending]) futures))]
    {:versions (into {} (map (fn [[domain result]] [domain (:version result)]) results))
     :rows (or (apply merge (map :rows (vals results))) {})}))

;; --- live corpus projection -------------------------------------------------

(defn- live-domain [port domain]
  (try
    (let [{:keys [version rows]} (scan-rows! port domain nil nil nil)]
      {:available true :version version :facts rows})
    (catch Throwable error
      {:available false :error (or (.getMessage error) "FRAMRPC scan failed")})))

(defn live-facts-view [port]
  (let [telemetry-future (when (telemetry-partition-enabled?)
                           (future (live-domain port :telemetry)))
        coordination (live-domain port :coordination)
        telemetry (when telemetry-future @telemetry-future)
        domains (cond-> {:coordination coordination}
                  telemetry (assoc :telemetry telemetry))
        unavailable (->> domains (remove (comp :available val))
                         (map (comp name key)) sort vec)
        facts (->> domains vals (filter :available) (mapcat :facts) distinct vec)]
    {:facts facts
     :domains domains
     :unavailable unavailable
     :unavailable-detail
     (->> domains
          (remove (comp :available val))
          (map (fn [[domain result]] [(name domain) (:error result)]))
          (sort-by first) vec)
     :complete (empty? unavailable)}))

(defn live-facts [port] (:facts (live-facts-view port)))

(defn live-propositions [port]
  (let [view (live-facts-view port)]
    (when-not (:complete view)
      (throw (ex-info "FRAMRPC live projection is incomplete"
                      {:type :incomplete-live-projection
                       :unavailable (:unavailable view)})))
    (mapv (fn [[subject predicate value]]
            (t/triple subject predicate value))
          (:facts view))))

;; --- mutation ---------------------------------------------------------------

(defn lease-fence [resource holder epoch]
  (when-not (and (some? resource) (some? holder)
                 (integer? epoch) (pos? epoch))
    (throw (ex-info "lease fence requires resource, holder, and positive epoch"
                    {:type :invalid-lease-fence})))
  (wire/rpc-fence! resource holder epoch))

(defn- fence-term [fence]
  (when fence
    (lease-fence (:resource fence) (:holder fence) (:epoch fence))))

(defn- raw-action! [{:keys [op subject predicate value policy] :as action}]
  (when-not (contains? #{:assert :retract} op)
    (throw (ex-info "transaction action must be :assert or :retract"
                    {:type :invalid-transaction-action :action action})))
  (when-not (contains? #{nil :rpc/subject-any :rpc/subject-existing} policy)
    (throw (ex-info "transaction action policy is invalid"
                    {:type :invalid-transaction-policy :policy policy})))
  {:op (if (= op :assert) :rpc/assert :rpc/retract)
   :proposition (proposition! subject predicate value)
   :policy (or policy :rpc/subject-any)})

;; A batch whose acknowledgement is lost has an outcome only its exact subjects
;; can answer. The comparison below is per (subject, predicate) VALUE PRESENCE
;; rather than occurrence frequency, because that is the granularity North
;; publications plan at: a frequency-exact readback would report a duplicate
;; this batch never touched as a foreign write.

(defn- action-subjects [actions]
  (into (sorted-set) (map #(t/triple-t1 (:proposition %))) actions))

(defn- subject-value-index! [client subjects]
  (reduce
   (fn [index subject]
     (let [projection (rpc/subject-projection! client subject)]
       (-> index
           (assoc-in [:values subject]
                     (into {}
                           (map (fn [[predicate occurrences]]
                                  [predicate (set (keys occurrences))]))
                           (:occurrences projection)))
           (update :version (fnil max 0) (:served-version projection)))))
   {:values {} :version nil}
   subjects))

(defn- action-landed? [values action]
  (let [proposition (:proposition action)
        present (contains? (get-in values [(t/triple-t1 proposition)
                                           (t/triple-t2 proposition)]
                                   #{})
                           (t/triple-t3 proposition))]
    (if (= :rpc/assert (:op action)) present (not present))))

(defn- exact-subject-resolver
  "Resolve an unacknowledged batch from the exact subjects it names. A batch
   applies whole or not at all, so every action landed is a commit and every
   action still holding its inverse is a proven non-commit. A proven non-commit
   re-sends the identical request: the pinned expected-version admits at most one
   commit, and at value-set granularity a re-assert of an absent value and a
   re-retract of a present one are the same write. Any other reading is another
   writer inside this subject and is refused instead of guessed."
  [actions]
  (fn [client _request _error]
    (let [{:keys [values version]}
          (subject-value-index! client (action-subjects actions))
          landed (map #(action-landed? values %) actions)]
      (cond
        (every? true? landed) {:resolution :committed :served-version version}
        (every? false? landed) {:resolution :retry}
        :else {:resolution :torn-subject :served-version version}))))

(defn- mutation-envelope [operation]
  (try
    (let [result (operation)]
      (if-let [resolved (:resolved result)]
        {:ok (:served-version resolved) :changed? true :results []}
        {:ok (:served-version result)
         :changed? (boolean (some :changed? (:results result)))
         :results (:results result)}))
    (catch clojure.lang.ExceptionInfo error
      (case (:type (ex-data error))
        :rpc/conflict {:reject :conflict
                       :version (:served-version (ex-data error))}
        :rpc/lease-fence-mismatch {:reject :fence-lost
                                   :version (:served-version (ex-data error))}
        :rpc/lease-held {:reject :held
                         :version (:served-version (ex-data error))}
        ;; The budget is exhausted only after every attempt was resolved as a
        ;; proven non-commit, so nothing landed and replanning is safe.
        :rpc/retry-exhausted {:reject :conflict
                              :version (:served-version (ex-data error))}
        (throw error)))))

(defn transact!
  ([port actions] (transact! port actions {}))
  ([port actions options]
   (let [actions (mapv raw-action! actions)
         domains (into #{} (map #(domain-for-subject
                                  (t/triple-t1 (:proposition %)))) actions)]
     (when (> (count domains) 1)
       (throw (ex-info "one transaction cannot cross SpaceIds"
                       {:type :cross-domain-transaction :domains domains})))
     (let [domain (or (first domains) :coordination)]
       (if (empty? actions)
         {:ok (cur-ver-in-domain port domain) :changed? false :results []}
         (with-client!
          port domain
          (fn [client]
            (mutation-envelope
             #(rpc/batch!
               client actions
               (cond-> {:ambiguity-resolver (exact-subject-resolver actions)}
                 (contains? options :expected-version)
                 (assoc :expected-version (:expected-version options))
                 (:fence options) (assoc :fence (fence-term (:fence options)))))))))))))

(defn assert-at-version! [port subject predicate value expected-version]
  (transact! port [{:op :assert :subject subject :predicate predicate :value value}]
             {:expected-version expected-version}))

(defn assert! [port subject predicate value expected-version]
  (assert-at-version! port subject predicate value expected-version))

(defn retract-at-version! [port subject predicate value expected-version]
  (transact! port [{:op :retract :subject subject :predicate predicate :value value}]
             {:expected-version expected-version}))

(defn- before-state [rows]
  (reduce-kv
   (fn [state subject pairs]
     (assoc state subject
            (reduce (fn [predicates [predicate value]]
                      (update predicates predicate (fnil conj #{}) value))
                    {} pairs)))
   {} rows))

(defn- publication-action! [{:keys [op subject predicate value values cardinality]
                             :as action}]
  (when-not (contains? #{:assert :retract :set} op)
    (throw (ex-info "publication action must be :assert, :retract, or :set"
                    {:type :invalid-publication-action :action action})))
  (write-value! subject predicate (if (= op :set) "set" value))
  (when (and (contains? #{:assert :set} op)
             (not (contains? #{:one :many} cardinality)))
    (throw (ex-info "assert/set publication requires :one or :many cardinality"
                    {:type :invalid-publication-cardinality :action action})))
  (when (and (= op :set) (not (vector? values)))
    (throw (ex-info "set publication requires a vector of values"
                    {:type :invalid-publication-values :action action})))
  (let [normalized (if (= op :set) (mapv str values) [(str value)])]
    (when (and (= cardinality :one) (> (count (distinct normalized)) 1))
      (throw (ex-info "single-valued publication has multiple values"
                      {:type :publication-cardinality-violation :action action})))
    (assoc action :value (first normalized) :values normalized)))

(defn- apply-publication [state {:keys [op subject predicate value values cardinality]}]
  (case op
    :assert
    (assoc-in state [subject predicate]
              (if (= cardinality :one)
                #{value}
                (conj (get-in state [subject predicate] #{}) value)))
    :retract
    (update-in state [subject predicate] (fnil disj #{}) value)
    :set
    (assoc-in state [subject predicate] (set values))))

(defn- raw-difference [before desired touched]
  (let [rows
        (mapcat
         (fn [[subject predicate]]
           (let [old (get-in before [subject predicate] #{})
                 new (get-in desired [subject predicate] #{})]
             (concat
              (map (fn [value] {:op :retract :subject subject
                                :predicate predicate :value value})
                   (sort (remove new old)))
              (map (fn [value] {:op :assert :subject subject
                                :predicate predicate :value value})
                   (sort (remove old new))))))
         (sort touched))]
    (vec (sort-by (fn [{:keys [op subject predicate value]}]
                    [(if (= op :retract) 0 1) subject predicate value]) rows))))

(defn publish!
  ([port actions] (publish! port actions {}))
  ([port actions options]
   (let [actions (mapv publication-action! actions)
         subjects (mapv :subject actions)
         domains (into #{} (map domain-for-subject subjects))
         _ (when (> (count domains) 1)
             (throw (ex-info "one publication cannot cross SpaceIds"
                             {:type :cross-domain-publication :domains domains})))
         domain (or (first domains) :coordination)
         touched (into #{} (map (juxt :subject :predicate)) actions)
         max-attempts (if (contains? options :expected-version)
                        1 (get options :max-attempts 4))]
     (loop [attempt 1]
       (let [{:keys [version rows]} (show-many-in-domain port domain subjects)]
         (if (and (contains? options :expected-version)
                  (not= version (:expected-version options)))
           {:reject :conflict :version version}
           (let [before (before-state rows)
                 desired (reduce apply-publication before actions)
                 raw (raw-difference before desired touched)]
             (if (empty? raw)
               {:ok version :changed? false :results []}
               (let [result (transact! port raw
                                       (cond-> {:expected-version version}
                                         (:fence options)
                                         (assoc :fence (:fence options))))]
                 (if (and (= :conflict (:reject result)) (< attempt max-attempts))
                   (recur (inc attempt))
                   result))))))))))

(defn append! [port subject predicate value]
  (publish! port [{:op :assert :subject subject :predicate predicate
                   :value value :cardinality :many}]))

(defn put! [port subject predicate value]
  (publish! port [{:op :assert :subject subject :predicate predicate
                   :value value :cardinality :one}]))

(defn retract!
  ([port subject predicate value]
   (publish! port [{:op :retract :subject subject :predicate predicate
                    :value value}]))
  ([port subject predicate value expected-version]
   (retract-at-version! port subject predicate value expected-version)))

(defn put-with-fence! [port fence subject predicate value]
  (publish! port [{:op :assert :subject subject :predicate predicate
                   :value value :cardinality :one}]
            {:fence fence}))

(defn retract-with-fence! [port fence subject predicate value]
  (publish! port [{:op :retract :subject subject :predicate predicate
                   :value value}]
            {:fence fence}))

(defn subject-readback!
  ([port subject desired] (subject-readback! port subject desired {}))
  ([port subject desired options]
   (with-client! port (domain-for-subject subject)
                 #(rpc/subject-readback! % subject desired options))))

(def assert-after-read-deadline-ms 30000)
(def ^:dynamic *retry-monotonic-now-ns* #(System/nanoTime))
(def ^:dynamic *retry-sleep-ms!* #(Thread/sleep %))

(defn retry-deadline-ns
  ([] (request-deadline-ns assert-after-read-deadline-ms))
  ([timeout-ms] (request-deadline-ns timeout-ms)))

(defn retry-conflicts-until!
  ([deadline operation] (retry-conflicts-until! deadline Integer/MAX_VALUE operation))
  ([deadline attempts operation]
   (loop [remaining attempts backoff 1]
     (if-not (< (long (*retry-monotonic-now-ns*)) deadline)
       {:reject :deadline}
       (let [result (operation)]
         (if (and (= :conflict (:reject result)) (> remaining 1))
           (do (*retry-sleep-ms!* backoff)
               (recur (dec remaining) (min 64 (* 2 backoff))))
           result))))))

(defn assert-after-read!
  ([port subject predicate value validate!]
   (assert-after-read! port subject predicate value validate! Integer/MAX_VALUE))
  ([port subject predicate value validate! attempts]
   (retry-conflicts-until!
    (retry-deadline-ns) attempts
    (fn []
      (let [base (cur-ver-for-subject port subject)]
        (validate!)
        (assert-at-version! port subject predicate value base))))))

(defn assert-after-read-with-fence!
  ([port fence subject predicate value validate!]
   (assert-after-read-with-fence! port fence subject predicate value validate! 16))
  ([port fence subject predicate value validate! attempts]
   (retry-conflicts-until!
    (retry-deadline-ns) attempts
    (fn []
      (let [base (cur-ver-for-subject port subject)]
        (validate!)
        (transact! port [{:op :assert :subject subject
                          :predicate predicate :value value}]
                   {:expected-version base :fence fence}))))))

(defn assert-batch-after-read!
  ([port subject plan!]
   (assert-batch-after-read! port subject plan! Integer/MAX_VALUE
                             (retry-deadline-ns)))
  ([port subject plan! attempts deadline]
   (retry-conflicts-until!
    deadline attempts
    (fn []
      (let [base (cur-ver-for-subject port subject)
            planned (plan!)]
        (if (contains? planned :done)
          planned
          (let [facts (:facts planned)]
            (when-not (and (vector? facts) (seq facts))
              (throw (ex-info "planned publication requires non-empty :facts"
                              {:type :invalid-planned-publication})))
            (publish!
             port
             (mapv (fn [fact]
                     {:op :assert :subject subject
                      :predicate (or (:predicate fact) (:p fact))
                      :value (if (contains? fact :value) (:value fact) (:r fact))
                      :cardinality (or (:cardinality fact) :many)})
                   facts)
             {:expected-version base}))))))))

;; --- typed leases -----------------------------------------------------------

(defn- instant-millis [instant]
  (+ (* 1000 (t/instant-epoch-seconds instant))
     (quot (t/instant-nanos instant) 1000000)))

(defn- fence-map [fence]
  (let [[resource holder epoch] (rpc/fence-parts fence)]
    {:resource resource :holder holder :epoch epoch}))

(defn authoritative-lease? [lease]
  (boolean
   (and (map? lease) (some? (:resource lease)) (some? (:holder lease))
        (integer? (:epoch lease)) (<= 1 (:epoch lease) lease-max-safe-integer))))

(defn acquire-lease! [port resource holder ttl-ms]
  (with-client!
   port :coordination
   (fn [client]
     (try
       (let [result (rpc/lease-acquire! client resource holder ttl-ms)
             fence (fence-map (:fence result))]
         (merge {:ok (:epoch fence)
                 :exp (instant-millis (:expires result))
                 :served-version (:served-version result)} fence))
       (catch clojure.lang.ExceptionInfo error
         (if (= :rpc/lease-held (:type (ex-data error)))
           {:reject :held :version (:served-version (ex-data error))}
           (throw error)))))))

(defn renew-lease! [port fence ttl-ms]
  (with-client!
   port :coordination
   (fn [client]
     (try
       (let [result (rpc/lease-renew! client (fence-term fence) ttl-ms)
             next (fence-map (:fence result))]
         (merge {:ok (:epoch next)
                 :exp (instant-millis (:expires result))
                 :served-version (:served-version result)} next))
       (catch clojure.lang.ExceptionInfo error
         (if (= :rpc/lease-fence-mismatch (:type (ex-data error)))
           {:reject :fence-lost :version (:served-version (ex-data error))}
           (throw error)))))))

(defn release-lease! [port fence]
  (with-client!
   port :coordination
   (fn [client]
     (let [result (rpc/lease-release! client (fence-term fence))]
       {:ok (:epoch fence) :released? (:released? result)
        :served-version (:served-version result)}))))

(defn check-lease! [port fence]
  (with-client!
   port :coordination
   (fn [client]
     (let [result (rpc/lease-check! client (fence-term fence))]
       {:valid? (:valid? result)
        :exp (when-let [expires (:expires result)] (instant-millis expires))
        :served-version (:served-version result)}))))

(defn lease-status [port resource]
  (let [now (System/currentTimeMillis)
        result
        (with-client!
         port :coordination
         (fn [client]
           (rpc/scan-all! client resource :kernel/lease nil {:page-size 2})))
        rows (:rows result)]
    (when (> (count rows) 1)
      (throw (ex-info "lease resource has multiple live propositions"
                      {:type :duplicate-resource-lease
                       :resource resource})))
    (if-let [proposition (first rows)]
      (let [lease (t/triple-t3 proposition)]
        (when-not
         (and (t/triple? proposition)
              (= resource (t/triple-t1 proposition))
              (= :kernel/lease (t/triple-t2 proposition))
              (t/triple? lease)
              (= :kernel/expires-at (t/triple-t2 lease))
              (integer? (t/triple-t3 lease)))
          (throw (ex-info "lease projection is malformed"
                          {:type :malformed-resource-lease
                           :resource resource})))
        (let [holder (t/triple-t1 lease)
              exp (t/triple-t3 lease)]
          {:resource resource :holder holder :exp exp
           :online? (> exp now) :version (:served-version result)}))
      {:resource resource :holder nil :exp nil
       :online? false :version (:served-version result)})))

(def ^:private session-resource-prefix "session:")

(defn- parse-session-lease! [proposition]
  (let [resource (when (t/triple? proposition)
                   (t/triple-t1 proposition))]
    (when (and (string? resource)
               (str/starts-with? resource session-resource-prefix))
      (let [handle (subs resource (count session-resource-prefix))
            value (t/triple-t3 proposition)]
        (when-not (and (not (str/blank? handle))
                       (t/triple? value)
                       (= handle (t/triple-t1 value))
                       (= :kernel/expires-at (t/triple-t2 value))
                       (integer? (t/triple-t3 value))
                       (<= 0 (t/triple-t3 value) lease-max-safe-integer))
          (throw (ex-info "session lease proposition is malformed"
                          {:type :malformed-session-lease
                           :resource resource})))
        {:handle handle :holder handle :exp (t/triple-t3 value)}))))

(defn- session-lease-scan [port resource]
  (with-client!
   port :coordination
   (fn [client]
     (rpc/scan-all! client resource :kernel/lease nil
                    {:page-size rpc/effective-page-limit}))))

(defn- session-lease-index! [rows]
  (reduce
   (fn [known proposition]
     (if-let [{:keys [handle] :as lease} (parse-session-lease! proposition)]
       (if (contains? known handle)
         (throw (ex-info "session has multiple live lease propositions"
                         {:type :duplicate-session-lease :handle handle}))
         (assoc known handle lease))
       known))
   {} rows))

(defn- all-session-status [port now]
  (when-not (and (integer? now) (not (neg? now)))
    (throw (ex-info "session status requires a non-negative observation time"
                    {:type :invalid-session-observation-time :now now})))
  (let [result (session-lease-scan port nil)
        sessions
        (into {}
              (map (fn [[handle lease]]
                     [handle (assoc lease :online? (> (:exp lease) now))]))
              (session-lease-index! (:rows result)))]
    {:version (:served-version result) :sessions sessions}))

(defn online-session-leases [port now]
  (->> (:sessions (all-session-status port now))
       vals (filter :online?) (sort-by :handle) vec))

(defn sessions-status [port handles]
  (let [handles (vec (distinct handles))]
    (when-not (every? #(and (string? %) (not (str/blank? %))) handles)
      (throw (ex-info "session handles must be nonblank strings"
                      {:type :invalid-session-handles})))
    (let [{:keys [version sessions]}
          (all-session-status port (System/currentTimeMillis))]
      {:version version
       :sessions
       (into {} (map (fn [handle]
                       [handle (get sessions handle
                                    {:holder nil :online? false :exp nil})])
                     handles))})))

(defn online-session-handles
  ([port] (online-session-handles port (System/currentTimeMillis)))
  ([port now]
   (let [{:keys [version sessions]} (all-session-status port now)]
     {:version version
      :handles (into (sorted-set)
                     (keep (fn [[handle status]]
                             (when (:online? status) handle)))
                     sessions)})))

(defn session-lease-status [port handle]
  (get-in (sessions-status port [handle]) [:sessions handle]))

(defn session-online? [port handle]
  (boolean (:online? (session-lease-status port handle))))

;; --- reductions used by North projections ----------------------------------

(def distinct-reducer
  {:init #{} :step (fn [values row] (conj values (first row))) :final identity})
(def sum-reducer
  {:init 0
   :step (fn [total row] (+ total (or (parse-double (str (second row))) 0)))
   :final identity})

(defn reduce-rows [{:keys [init step final]} rows]
  (final (reduce step init rows)))
(defn sum-rows [rows] (reduce-rows sum-reducer rows))
(defn distinct-rows [rows] (reduce-rows distinct-reducer rows))

(defn agg-rows [port project body]
  (query-rows
   port {:find "north_aggregate"
         :rules [{:head {:rel "north_aggregate"
                         :args (mapv (fn [name] {:var name}) project)}
                  :body body}]}))

(defn aggregate [port project body reducer]
  (reduce-rows reducer (agg-rows port project body)))
(defn distinct-of [port project body]
  (aggregate port project body distinct-reducer))
(defn count-distinct [port project body]
  (count (distinct-of port project body)))
(defn sum-of [port project body]
  (aggregate port project body sum-reducer))
(defn quorum-met? [port k project body]
  (>= (count-distinct port project body) k))

(defn pending-cmds [port]
  (query-rows
   port
   {:find "pending"
    :strata
    [[{:head {:rel "settled" :args [{:var "command"}]}
       :body [{:rel "triple"
               :args [{:var "command"} "acked_by" {:var "agent"}]}]}
      {:head {:rel "settled" :args [{:var "command"}]}
       :body [{:rel "triple"
               :args [{:var "command"} "failed_by" {:var "agent"}]}]}]
     [{:head {:rel "pending"
              :args [{:var "command"} {:var "operation"} {:var "target"}]}
       :body [{:rel "triple"
               :args [{:var "command"} "op" {:var "operation"}]}
              {:rel "triple"
               :args [{:var "command"} "target" {:var "target"}]}
              {:rel "settled" :args [{:var "command"}] :neg true}]}]]}))

(defn -main [& args]
  (let [port (port! (or (first args) PORT))
        domain (case (second args)
                 nil :coordination
                 "coordination" :coordination
                 "telemetry" :telemetry
                 (throw (ex-info "coord status domain must be coordination or telemetry"
                                 {:type :invalid-domain
                                  :domain (second args)})))]
    (prn (status-in-domain port domain))))

(when (and (= *file* (System/getProperty "babashka.file"))
           (not (contains? #{"-e" "-m"} (first *command-line-args*))))
  (apply -main *command-line-args*))
