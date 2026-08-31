(ns north.coord
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [store.rpc :as wire]
            [store.types :as t]))

(def cli-dir (.getParentFile (io/file *file*)))

(clojure.core/load-file (str cli-dir "/store-rpc-client.clj"))

(def PORT (or (System/getenv "NORTH_PORT") "7977"))

(def query-page-row-limit 4096)

(def lease-max-safe-integer 9007199254740991)

(def telemetry-subject-tokens #{"run" "session" "mine" "guard_denial"})

(def ^:dynamic *operation-domain* nil)

(def ^:dynamic *request-deadline-ns* nil)

(defn telemetry-partition-enabled? []
  (= "1" (System/getenv "NORTH_TELEMETRY_PARTITION")))

(defn- parse-port! [value]
  (let [parsed (if (integer? value) value (parse-long (str value)))]
  (if (not (and parsed (<= 1 parsed 65535))) (do
  (throw (ex-info "Store RPC port must be an integer from 1 through 65535" {:type :invalid-store-rpc-port :value value}))))
  (int parsed)))

(defn- positive-env-int [name fallback maximum]
  (let [raw (or (System/getenv name) (str fallback))
   value (if (re-matches #"[1-9][0-9]*" raw) (do
  (parse-long raw)))]
  (if (not (and value (<= value maximum))) (do
  (throw (ex-info (str name " must be an integer from 1 through " maximum) {:type :invalid-store-rpc-bound :name name :value raw}))))
  (int value)))

(defn request-deadline-ns [timeout-ms]
  (if (not (and (integer? timeout-ms) (pos? timeout-ms))) (do
  (throw (ex-info "coordination deadline requires positive milliseconds" {:type :invalid-coordination-deadline :timeout-ms timeout-ms}))))
  (+ (System/nanoTime) (* 1000000 (long timeout-ms))))

(defn- remaining-timeout-ms [configured]
  (if *request-deadline-ns* (let [remaining (- (long *request-deadline-ns*) (System/nanoTime))]
  (if (not (pos? remaining)) (do
  (throw (ex-info "coordination operation deadline exceeded" {:type :coordination-operation-timeout}))))
  (int (max 1 (min configured (quot (+ remaining 999999) 1000000))))) configured))

(defn telemetry-subject? [subject]
  (boolean (if (and (string? subject) (str/starts-with? subject "@")) (do
  (let [colon (str/index-of subject ":")
   token (if (and colon (> colon 1)) (do
  (subs subject 1 colon)))]
  (contains? telemetry-subject-tokens token))))))

(defn- query-literal-subjects [query]
  (->> (concat (:rules query) (mapcat identity (:strata query))) (mapcat :body) (keep (fn [literal] (let [subject (first (:args literal))]
  (if (and (= "triple" (:rel literal)) (string? subject) (str/starts-with? subject "@")) (do
  subject))))) set))

(defn- domain-for-subject [subject]
  (if (and (telemetry-partition-enabled?) (telemetry-subject? subject)) :telemetry :coordination))

(defn- domain-for-query [query]
  (let [subjects (query-literal-subjects query)]
  (if (and (seq subjects) (every? telemetry-subject? subjects) (telemetry-partition-enabled?)) :telemetry :coordination)))

(defn- host []
  (or (not-empty (System/getenv "NORTH_STORE_HOST")) "127.0.0.1"))

(defn- domain-port! [requested domain]
  (if (and (= domain :telemetry) (telemetry-partition-enabled?)) (parse-port! (or (System/getenv "NORTH_TELEMETRY_PORT") "7978")) (parse-port! requested)))

(defn- domain-space [domain]
  (if (and (= domain :telemetry) (telemetry-partition-enabled?)) (or (not-empty (System/getenv "NORTH_TELEMETRY_SPACE_ID")) "north-telemetry") (or (not-empty (System/getenv "BEAGLE_STORE_SPACE_ID")) "north-coordination")))

(defn- client-for! [port domain]
  (north.store-rpc-client/client (host) (domain-port! port domain) (domain-space domain) {:connect-timeout-ms (remaining-timeout-ms (positive-env-int "NORTH_COORD_CONNECT_TIMEOUT_MS" 1000 999999)) :read-timeout-ms (remaining-timeout-ms (positive-env-int "NORTH_STORE_READ_TIMEOUT_MS" 30000 999999)) :max-attempts 3 :retry-delay-ms 10 :jitter-ms 25}))

(defn- with-client! [port domain operation]
  (let [client (client-for! port domain)]
  (try
  (operation client)
  (finally
    (north.store-rpc-client/close! client)))))

(defn canonical-log-path [log]
  (if (not (and (string? log) (not (str/blank? log)))) (do
  (throw (ex-info "Beagle Store log identity must be a nonblank path" {:type :invalid-log-identity :log log}))))
  (.getCanonicalPath (io/file log)))

(defn expected-log []
  (let [home (or (System/getenv "HOME") (System/getProperty "user.home"))]
  (canonical-log-path (or (System/getenv "BEAGLE_STORE_LOG") (str home "/.local/state/north/coordination.storelog")))))

(defn telemetry-log-path []
  (if (telemetry-partition-enabled?) (do
  (canonical-log-path (or (System/getenv "BEAGLE_STORE_TELEMETRY_LOG") (throw (ex-info "BEAGLE_STORE_TELEMETRY_LOG is required for partitioned telemetry" {:type :missing-telemetry-log})))))))

(defn- triple-row! [triple]
  (let [row [(t/triple-t1 triple) (t/triple-t2 triple) (t/triple-t3 triple)]]
  (if (not (every? string? row)) (do
  (throw (ex-info "North coordination data contains a non-string triple" {:type :malformed-coordination-triple :row row}))))
  row))

(defn- lease-proposition? [proposition]
  (and (t/triple? proposition) (string? (t/triple-t1 proposition)) (= :kernel/lease (t/triple-t2 proposition)) (let [lease (t/triple-t3 proposition)]
  (and (t/triple? lease) (string? (t/triple-t1 lease)) (= :kernel/expires-at (t/triple-t2 lease)) (integer? (t/triple-t3 lease))))))

(defn- occurrence-metadata-proposition? [proposition]
  (and (t/triple? proposition) (let [subject (t/triple-t1 proposition)
   predicate (t/triple-t2 proposition)
   value (t/triple-t3 proposition)
   transaction? (t/transaction-coordinate? subject)
   occurrence? (t/occurrence-coordinate? subject)]
  (case predicate
    :kernel/asserted-by (and (or transaction? occurrence?) (t/term? value))
    :kernel/supersedes (and occurrence? (t/occurrence-coordinate? value))
    false))))

(defn- bridge-command-receipt-proposition? [proposition]
  (and (t/triple? proposition) (let [subject (t/triple-t1 proposition)
   predicate (t/triple-t2 proposition)]
  (and (string? subject) (str/starts-with? subject "@bridge-command:") (string? predicate) (str/starts-with? predicate "bridge.command/")))))

(defn- projection-internal-proposition? [proposition]
  (or (lease-proposition? proposition) (bridge-command-receipt-proposition? proposition) (occurrence-metadata-proposition? proposition)))

(defn- coordination-fact-rows! [propositions]
  (->> propositions (remove projection-internal-proposition?) (mapv triple-row!)))

(defn write-value! [subject predicate value]
  (if (not (and (string? subject) (not (str/blank? subject)))) (do
  (throw (ex-info "coordination write requires a nonblank string subject" {:type :invalid-write :field :subject}))))
  (if (not (and (string? predicate) (not (str/blank? predicate)))) (do
  (throw (ex-info "coordination write requires a nonblank string predicate" {:type :invalid-write :field :predicate}))))
  (if (nil? value) (do
  (throw (ex-info "coordination write requires a non-nil value" {:type :invalid-write :field :value}))))
  (str value))

(defn- proposition! [subject predicate value]
  (t/triple subject predicate (write-value! subject predicate value)))

(defn status-in-domain! [port domain]
  (with-client! port domain north.store-rpc-client/status!))

(defn port! []
  (parse-port! PORT))

(defn status! [port]
  (status-in-domain! port :coordination))

(defn cur-ver-in-domain! [port domain]
  (with-client! port domain (fn [%1] (-> (north.store-rpc-client/version! %1) :served-version))))

(defn cur-ver! [port]
  (cur-ver-in-domain! port :coordination))

(defn version! [port]
  (cur-ver! port))

(defn cur-ver-for-subject! [port subject]
  (cur-ver-in-domain! port (domain-for-subject subject)))

(defn- scan-terms! [port domain t1 t2 t3]
  (with-client! port domain (fn [client] (let [result (north.store-rpc-client/scan-all! client t1 t2 t3 {:page-size north.store-rpc-client/effective-page-limit})]
  {:version (:served-version result) :rows (:rows result)}))))

(defn- scan-rows! [port domain t1 t2 t3]
  (let [{:keys [version rows]} (scan-terms! port domain t1 t2 t3)]
  {:version version :rows (mapv triple-row! rows)}))

(defn show-envelope! [port subject]
  (let [{:keys [version rows]} (scan-rows! port (domain-for-subject subject) subject nil nil)]
  {:version version :rows (mapv (fn [[_ predicate value]] [predicate value]) rows)}))

(defn show-rows! [port subject]
  (:rows (show-envelope! port subject)))

(defn show! [port subject]
  (show-envelope! port subject))

(defn subject-propositions! [port subject]
  (mapv (fn [[predicate value]] (t/triple subject predicate value)) (show-rows! port subject)))

(defn resolved-envelope! [port subject predicate]
  (let [{:keys [version rows]} (scan-rows! port (domain-for-subject subject) subject predicate nil)
   values (->> rows (map (fn [%1] (nth %1 2))) distinct sort vec)
   members (count values)]
  {:value (first values) :members members :ambiguous? (> members 1) :values values :version version}))

(defn resolved! [port subject predicate]
  (:value (resolved-envelope! port subject predicate)))

(defn many! [port subject predicate]
  (:values (resolved-envelope! port subject predicate)))

(defn- query-argument! [argument]
  (if (map? argument) (if (and (= #{:var} (set (keys argument))) (string? (:var argument)) (not (str/blank? (:var argument)))) (wire/rpc-query-variable! (:var argument)) (throw (ex-info "query variable is malformed" {:type :invalid-coordination-query :argument argument}))) (wire/rpc-query-constant! argument)))

(defn- query-relation! [{:keys [rel args neg] :as relation}]
  (if (not (and (string? rel) (not (str/blank? rel)) (vector? args) (or (nil? neg) (boolean? neg)))) (do
  (throw (ex-info "query relation is malformed" {:type :invalid-coordination-query :relation relation}))))
  (wire/rpc-query-relation! rel (mapv query-argument! args) (boolean neg)))

(defn- query-head! [{:keys [rel args] :as head}]
  (if (not (and (string? rel) (not (str/blank? rel)) (vector? args))) (do
  (throw (ex-info "query head is malformed" {:type :invalid-coordination-query :head head}))))
  (wire/rpc-query-head! rel (mapv query-argument! args)))

(defn- query-rule! [{:keys [head body] :as rule}]
  (if (not (and (map? head) (vector? body) (seq body))) (do
  (throw (ex-info "query rule is malformed" {:type :invalid-coordination-query :rule rule}))))
  (wire/rpc-query-rule! (query-head! head) (mapv query-relation! body)))

(defn- query-request! [query at-version]
  (let [{:keys [find rules strata]} query
   query-strata (cond
  (and (vector? strata) (seq strata) (nil? rules)) strata
  (and (vector? rules) (seq rules) (nil? strata)) [rules]
  :else nil)]
  (if (not (and (string? find) (not (str/blank? find)) query-strata (every? (fn [%1] (and (vector? %1) (seq %1))) query-strata))) (do
  (throw (ex-info "query plan is malformed" {:type :invalid-coordination-query :query query}))))
  (wire/rpc-query-request! (wire/rpc-query-plan! (wire/rpc-query-find-relation! find) (mapv (fn [stratum] (wire/rpc-query-stratum! (mapv query-rule! stratum))) query-strata)) (if (some? at-version) (wire/rpc-query-as-of! at-version) wire/query-current))))

(defn- string-query-rows! [rows]
  (if (not (and (vector? rows) (every? (fn [%1] (and (vector? %1) (every? string? %1))) rows))) (do
  (throw (ex-info "coordination query returned non-string rows" {:type :malformed-coordination-query-response}))))
  rows)

(defn valid-query-page-cursor? [cursor]
  (or (nil? cursor) (t/triple? cursor)))

(defn- logical-query-page! [client query limit cursor at-version]
  (if (not (and (integer? limit) (<= 1 limit query-page-row-limit))) (do
  (throw (ex-info "query page limit is outside the supported range" {:type :invalid-query-page-limit :limit limit}))))
  (if (not (valid-query-page-cursor? cursor)) (do
  (throw (ex-info "query cursor is not a canonical Term" {:type :invalid-query-page-cursor}))))
  (if (not (or (nil? at-version) (and (integer? at-version) (not (neg? at-version))))) (do
  (throw (ex-info "query snapshot version is invalid" {:type :invalid-query-page-version :at-version at-version}))))
  (let [request (query-request! query at-version)]
  (loop [remaining (long limit)
   after cursor
   rows []
   served nil]
  (let [wire-limit (min remaining north.store-rpc-client/effective-page-limit)
   response (north.store-rpc-client/query! client request {:page (wire/rpc-page-request! wire-limit after)})
   page (:page response)
   page-rows (string-query-rows! (:rows response))
   version (or served (:served-version response))]
  (if (not page) (do
  (throw (ex-info "query page omitted cursor metadata" {:type :malformed-coordination-query-response}))))
  (if (not (= version (:served-version response))) (do
  (throw (ex-info "query page changed snapshot" {:type :query-page-snapshot-changed :expected version :actual (:served-version response)}))))
  (let [all (into rows page-rows)
   left (- remaining (count page-rows))]
  (cond
  (:done? page) {:rows all :served-version version :done? true :cursor nil}
  (zero? left) {:rows all :served-version version :done? false :cursor (:cursor page)}
  (or (empty? page-rows) (= after (:cursor page))) (throw (ex-info "query page cursor did not advance" {:type :stalled-query-page-cursor}))
  :else (recur left (:cursor page) all version)))))))

(defn query-page-in-domain!
  ([port domain query limit cursor]
    (query-page-in-domain! port domain query limit cursor nil))
  ([port domain query limit cursor at-version]
    (with-client! port domain (fn [%1] (logical-query-page! %1 query limit cursor at-version)))))

(defn query-page!
  ([port query limit cursor]
    (query-page! port query limit cursor nil))
  ([port query limit cursor at-version]
    (query-page-in-domain! port (domain-for-query query) query limit cursor at-version)))

(defn bounded-query-in-domain! [port domain query max-rows]
  (if (not (and (integer? max-rows) (pos? max-rows))) (do
  (throw (ex-info "query row bound must be a positive integer" {:type :invalid-query-row-bound :max-rows max-rows}))))
  (loop [remaining (long max-rows)
   cursor nil
   rows []
   at-version nil]
  (let [limit (min remaining query-page-row-limit)
   page (query-page-in-domain! port domain query limit cursor at-version)
   combined (into rows (:rows page))]
  (cond
  (:done? page) {:rows combined :served-version (:served-version page)}
  (= (count combined) max-rows) (throw (ex-info "query exceeded its row bound" {:type :query-row-limit :max-rows max-rows}))
  :else (recur (- remaining (count (:rows page))) (:cursor page) combined (or at-version (:served-version page)))))))

(defn bounded-query! [port query max-rows]
  (bounded-query-in-domain! port (domain-for-query query) query max-rows))

(defn query-rows-in-domain! [port domain query]
  (with-client! port domain (fn [client] (let [result (north.store-rpc-client/query-all! client (query-request! query nil) {:page-size north.store-rpc-client/effective-page-limit})]
  (string-query-rows! (:rows result))))))

(defn query-rows! [port query]
  (query-rows-in-domain! port (domain-for-query query) query))

(defn- occurrence-window-request! [lower-exclusive upper-inclusive]
  (let [coordinate (wire/rpc-query-variable! "coordinate")
   action (wire/rpc-query-variable! "action")
   proposition (wire/rpc-query-variable! "proposition")]
  (wire/rpc-query-request! (wire/rpc-query-plan! (wire/rpc-query-find-relation! "north_occurrence_window") [(wire/rpc-query-stratum! [(wire/rpc-query-rule! (wire/rpc-query-head! "north_occurrence_window" [coordinate action proposition]) [(wire/rpc-query-relation! "occurrence" [coordinate action proposition] false)])])]) (wire/rpc-query-since! lower-exclusive (wire/rpc-query-as-of! upper-inclusive)))))

(defn- occurrence-event! [[coordinate action proposition :as row]]
  (if (not (and (= 3 (count row)) (t/occurrence-coordinate? coordinate) (contains? #{t/assert-action t/retract-action} action) (t/triple? proposition))) (do
  (throw (ex-info "occurrence window returned a malformed row" {:type :malformed-occurrence-window :row row}))))
  (let [subject (t/triple-t1 proposition)
   predicate (t/triple-t2 proposition)
   value (t/triple-t3 proposition)
   transaction (t/triple-t1 coordinate)]
  {:operation (if (= action t/assert-action) :assert :retract) :subject subject :predicate predicate :value value :version (t/triple-t3 transaction)}))

(defn poll-occurrence-window-in-domain! [port domain lower-exclusive upper-inclusive handle!]
  (if (not (and (integer? lower-exclusive) (integer? upper-inclusive) (<= 0 lower-exclusive upper-inclusive))) (do
  (throw (ex-info "occurrence window bounds are invalid" {:type :invalid-occurrence-window :lower lower-exclusive :upper upper-inclusive}))))
  (if (< lower-exclusive upper-inclusive) (do
  (with-client! port domain (fn [client] (let [request (occurrence-window-request! lower-exclusive upper-inclusive)]
  (loop [cursor nil]
  (let [response (north.store-rpc-client/query! client request {:page (wire/rpc-page-request! north.store-rpc-client/effective-page-limit cursor)})
   page (:page response)]
  (if (not (and page (= upper-inclusive (:served-version response)))) (do
  (throw (ex-info "occurrence window changed snapshot" {:type :occurrence-window-snapshot-changed :expected upper-inclusive :actual (:served-version response)}))))
  (doseq [row (:rows response)]
  (handle! (occurrence-event! row)))
  (if (not (:done? page)) (do
  (recur (:cursor page)))))))))))
  upper-inclusive)

(defn poll-occurrence-window! [port lower-exclusive upper-inclusive handle!]
  (poll-occurrence-window-in-domain! port :coordination lower-exclusive upper-inclusive handle!))

(defn occurrence-window! [port lower-exclusive upper-inclusive]
  (let [events (volatile! [])]
  (poll-occurrence-window! port lower-exclusive upper-inclusive (fn [%1] (vswap! events conj %1)))
  {:version upper-inclusive :events (deref events)}))

(def proposition-occurrence-result-limit 2)

(def proposition-occurrence-page-limit (inc proposition-occurrence-result-limit))

(defn- proposition-occurrences-request! [subject predicate value]
  (let [coordinate (wire/rpc-query-variable! "coordinate")
   action (wire/rpc-query-variable! "action")
   proposition (wire/rpc-query-constant! (proposition! subject predicate value))]
  (wire/rpc-query-request! (wire/rpc-query-plan! (wire/rpc-query-find-relation! "north_proposition_occurrence") [(wire/rpc-query-stratum! [(wire/rpc-query-rule! (wire/rpc-query-head! "north_proposition_occurrence" [coordinate action]) [(wire/rpc-query-relation! "occurrence" [coordinate action proposition] false)])])]) wire/query-current)))

(defn proposition-occurrences!
  "Bounded exact occurrence history for one proposition. Coordinates remain\n   the Store ordering authority; callers receive only their validated logical\n   transaction version and in-transaction ordinal. More than two matching\n   operations is already contradictory for every North singleton receipt and\n   therefore fails closed without draining unrelated history." [port subject predicate value]
  (with-client! port (domain-for-subject subject) (fn [client] (let [response (north.store-rpc-client/query! client (proposition-occurrences-request! subject predicate value) {:page (wire/rpc-page-request! proposition-occurrence-page-limit nil)})
   page (:page response)
   rows (:rows response)]
  (if (not (and page (:done? page) (<= (count rows) proposition-occurrence-result-limit))) (do
  (throw (ex-info "proposition occurrence history exceeds its exact bound" {:type :proposition-occurrence-history-ambiguous :subject subject :predicate predicate}))))
  (mapv (fn [[coordinate action :as row]] (if (not (and (= 2 (count row)) (t/occurrence-coordinate? coordinate) (contains? #{t/assert-action t/retract-action} action))) (do
  (throw (ex-info "proposition occurrence query returned a malformed row" {:type :malformed-proposition-occurrence :row row}))))
  (let [transaction (t/triple-t1 coordinate)]
  (if (not (t/transaction-coordinate? transaction)) (do
  (throw (ex-info "proposition occurrence has an invalid transaction" {:type :malformed-proposition-occurrence :coordinate coordinate}))))
  {:operation (if (= action t/assert-action) :assert :retract) :version (t/triple-t3 transaction) :ordinal (t/triple-t3 coordinate)})) rows)))))

(defn- subject-query [subjects]
  {:find "north_subject_fact" :rules (mapv (fn [subject] {:head {:rel "north_subject_fact" :args [subject {:var "predicate"} {:var "value"}]} :body [{:rel "triple" :args [subject {:var "predicate"} {:var "value"}]}]}) subjects)})

(defn show-many-in-domain! [port domain subjects]
  (let [subjects (vec (distinct subjects))]
  (if (not (and (every? (fn [%1] (and (string? %1) (not (str/blank? %1)))) subjects) (<= (count subjects) query-page-row-limit))) (do
  (throw (ex-info "show-many subjects are invalid or exceed the bound" {:type :invalid-show-many-subjects}))))
  (if (empty? subjects) {:version (cur-ver-in-domain! port domain) :rows {}} (with-client! port domain (fn [client] (let [result (north.store-rpc-client/query-all! client (query-request! (subject-query subjects) nil) {:page-size north.store-rpc-client/effective-page-limit})
   rows (string-query-rows! (:rows result))]
  {:version (:served-version result) :rows (reduce (fn [by-subject [subject predicate value]] (update by-subject subject conj [predicate value])) (zipmap subjects (repeat [])) rows)}))))))

(defn show-many! [port subjects]
  (let [groups (group-by domain-for-subject (vec (distinct subjects)))
   futures (into {} (map (fn [[domain values]] [domain (future-call (fn [] (show-many-in-domain! port domain values)))]) groups))
   results (into {} (map (fn [[domain pending]] [domain (deref pending)]) futures))]
  {:versions (into {} (map (fn [[domain result]] [domain (:version result)]) results)) :rows (or (apply merge (map (fn [result] (:rows result)) (vals results))) {})}))

(defn- live-domain! [port domain]
  (try
  (let [{:keys [version rows]} (scan-terms! port domain nil nil nil)
   facts (if (= :coordination domain) (coordination-fact-rows! rows) (mapv triple-row! rows))]
  {:available true :version version :facts facts})
  (catch Throwable error
    {:available false :error (or (.getMessage error) "Store RPC scan failed")})))

(defn live-facts-view! [port]
  (let [telemetry-future (if (telemetry-partition-enabled?) (do
  (future-call (fn [] (live-domain! port :telemetry)))))
   coordination (live-domain! port :coordination)
   telemetry (if telemetry-future (do
  (deref telemetry-future)))
   domains (cond-> {:coordination coordination} telemetry (assoc :telemetry telemetry))
   unavailable (->> domains (remove (fn [entry] (:available (val entry)))) (map (comp name key)) sort vec)
   facts (->> domains vals (filter (fn [result] (:available result))) (mapcat (fn [result] (:facts result))) distinct vec)]
  {:facts facts :domains domains :unavailable unavailable :unavailable-detail (->> domains (remove (fn [entry] (:available (val entry)))) (map (fn [[domain result]] [(name domain) (:error result)])) (sort-by first) vec) :complete (empty? unavailable)}))

(defn live-facts! [port]
  (:facts (live-facts-view! port)))

(defn live-propositions! [port]
  (let [view (live-facts-view! port)]
  (if (not (:complete view)) (do
  (throw (ex-info "Store RPC live projection is incomplete" {:type :incomplete-live-projection :unavailable (:unavailable view)}))))
  (mapv (fn [[subject predicate value]] (t/triple subject predicate value)) (:facts view))))

(defn lease-fence [resource holder epoch]
  (if (not (and (some? resource) (some? holder) (integer? epoch) (pos? epoch))) (do
  (throw (ex-info "lease fence requires resource, holder, and positive epoch" {:type :invalid-lease-fence}))))
  (wire/rpc-fence! resource holder epoch))

(defn- fence-term [fence]
  (if fence (do
  (lease-fence (:resource fence) (:holder fence) (:epoch fence)))))

(defn- raw-action! [{:keys [op subject predicate value policy] :as action}]
  (if (not (contains? #{:assert :retract} op)) (do
  (throw (ex-info "transaction action must be :assert or :retract" {:type :invalid-transaction-action :action action}))))
  (if (not (contains? #{nil :rpc/subject-any :rpc/subject-existing} policy)) (do
  (throw (ex-info "transaction action policy is invalid" {:type :invalid-transaction-policy :policy policy}))))
  {:op (if (= op :assert) :rpc/assert :rpc/retract) :proposition (proposition! subject predicate value) :policy (or policy :rpc/subject-any)})

(defn- action-subjects [actions]
  (into (sorted-set) (map (fn [%1] (t/triple-t1 (:proposition %1)))) actions))

(defn- subject-value-index! [client subjects]
  (reduce (fn [index subject] (let [projection (north.store-rpc-client/subject-projection! client subject)]
  (-> index (assoc-in [:values subject] (into {} (map (fn [[predicate occurrences]] [predicate (set (keys occurrences))])) (:occurrences projection))) (update :version (fnil max 0) (:served-version projection))))) {:values {} :version nil} subjects))

(defn- action-landed? [values action]
  (let [proposition (:proposition action)
   present (contains? (get-in values [(t/triple-t1 proposition) (t/triple-t2 proposition)] #{}) (t/triple-t3 proposition))]
  (if (= :rpc/assert (:op action)) present (not present))))

(defn- exact-subject-resolver!
  "Resolve an unacknowledged batch from the exact subjects it names. A batch\n   applies whole or not at all, so every action landed is a commit and every\n   action still holding its inverse is a proven non-commit. A proven non-commit\n   re-sends the identical request: the pinned expected-version admits at most one\n   commit, and at value-set granularity a re-assert of an absent value and a\n   re-retract of a present one are the same write. Any other reading is another\n   writer inside this subject and is refused instead of guessed." [actions]
  (fn [client _request _error] (let [{:keys [values version]} (subject-value-index! client (action-subjects actions))
   landed (map (fn [%1] (action-landed? values %1)) actions)]
  (cond
  (every? true? landed) {:resolution :committed :served-version version}
  (every? false? landed) {:resolution :retry}
  :else {:resolution :torn-subject :served-version version}))))

(defn- mutation-envelope [operation]
  (try
  (let [result (operation)]
  (let [resolved (:resolved result)]
  (if resolved {:ok (:served-version resolved) :changed? true :results []} {:ok (:served-version result) :changed? (boolean (some (fn [item] (:changed? item)) (:results result))) :results (:results result)})))
  (catch clojure.lang.ExceptionInfo error
    (case (:type (ex-data error))
    :rpc/conflict {:reject :conflict :version (:served-version (ex-data error))}
    :rpc/lease-fence-mismatch {:reject :fence-lost :version (:served-version (ex-data error))}
    :rpc/lease-held {:reject :held :version (:served-version (ex-data error))}
    :rpc/retry-exhausted {:reject :conflict :version (:served-version (ex-data error))}
    (throw error)))))

(defn transact!
  ([port actions]
    (transact! port actions {}))
  ([port actions options]
    (let [actions (mapv raw-action! actions)
   domains (into #{} (map (fn [%1] (domain-for-subject (t/triple-t1 (:proposition %1))))) actions)]
  (if (> (count domains) 1) (do
  (throw (ex-info "one transaction cannot cross SpaceIds" {:type :cross-domain-transaction :domains domains}))))
  (let [domain (or (first domains) :coordination)]
  (if (empty? actions) {:ok (cur-ver-in-domain! port domain) :changed? false :results []} (with-client! port domain (fn [client] (mutation-envelope (fn [] (north.store-rpc-client/batch! client actions (cond-> {:ambiguity-resolver (exact-subject-resolver! actions)} (contains? options :expected-version) (assoc :expected-version (:expected-version options)) (:fence options) (assoc :fence (fence-term (:fence options))))))))))))))

(defn assert-at-version! [port subject predicate value expected-version]
  (transact! port [{:op :assert :subject subject :predicate predicate :value value}] {:expected-version expected-version}))

(defn assert! [port subject predicate value expected-version]
  (assert-at-version! port subject predicate value expected-version))

(defn retract-at-version! [port subject predicate value expected-version]
  (transact! port [{:op :retract :subject subject :predicate predicate :value value}] {:expected-version expected-version}))

(defn- before-state [rows]
  (reduce-kv (fn [state subject pairs] (assoc state subject (reduce (fn [predicates [predicate value]] (update predicates predicate (fnil conj #{}) value)) {} pairs))) {} rows))

(defn- publication-action! [{:keys [op subject predicate value values cardinality] :as action}]
  (if (not (contains? #{:assert :retract :set} op)) (do
  (throw (ex-info "publication action must be :assert, :retract, or :set" {:type :invalid-publication-action :action action}))))
  (write-value! subject predicate (if (= op :set) "set" value))
  (if (and (contains? #{:assert :set} op) (not (contains? #{:one :many} cardinality))) (do
  (throw (ex-info "assert/set publication requires :one or :many cardinality" {:type :invalid-publication-cardinality :action action}))))
  (if (and (= op :set) (not (vector? values))) (do
  (throw (ex-info "set publication requires a vector of values" {:type :invalid-publication-values :action action}))))
  (let [normalized (if (= op :set) (mapv str values) [(str value)])]
  (if (and (= cardinality :one) (> (count (distinct normalized)) 1)) (do
  (throw (ex-info "single-valued publication has multiple values" {:type :publication-cardinality-violation :action action}))))
  (assoc action :value (first normalized) :values normalized)))

(defn- apply-publication [state {:keys [op subject predicate value values cardinality]}]
  (case op
    :assert (assoc-in state [subject predicate] (if (= cardinality :one) #{value} (conj (get-in state [subject predicate] #{}) value)))
    :retract (update-in state [subject predicate] (fnil disj #{}) value)
    :set (assoc-in state [subject predicate] (set values))))

(defn- raw-difference [before desired touched]
  (let [rows (mapcat (fn [[subject predicate]] (let [old (get-in before [subject predicate] #{})
   new (get-in desired [subject predicate] #{})]
  (concat (map (fn [value] {:op :retract :subject subject :predicate predicate :value value}) (sort (remove new old))) (map (fn [value] {:op :assert :subject subject :predicate predicate :value value}) (sort (remove old new)))))) (sort touched))]
  (vec (sort-by (fn [{:keys [op subject predicate value]}] [(if (= op :retract) 0 1) subject predicate value]) rows))))

(defn publish!
  ([port actions]
    (publish! port actions {}))
  ([port actions options]
    (let [actions (mapv publication-action! actions)
   subjects (mapv (fn [action] (:subject action)) actions)
   domains (into #{} (map domain-for-subject subjects))
   _ (if (> (count domains) 1) (do
  (throw (ex-info "one publication cannot cross SpaceIds" {:type :cross-domain-publication :domains domains}))))
   domain (or (first domains) :coordination)
   touched (into #{} (map (juxt :subject :predicate)) actions)
   max-attempts (if (contains? options :expected-version) 1 (:max-attempts options 4))]
  (loop [attempt 1]
  (let [{:keys [version rows]} (show-many-in-domain! port domain subjects)]
  (if (and (contains? options :expected-version) (not= version (:expected-version options))) {:reject :conflict :version version} (let [before (before-state rows)
   desired (reduce apply-publication before actions)
   raw (raw-difference before desired touched)]
  (if (empty? raw) {:ok version :changed? false :results []} (let [result (transact! port raw (cond-> {:expected-version version} (:fence options) (assoc :fence (:fence options))))]
  (if (and (= :conflict (:reject result)) (< attempt max-attempts)) (recur (inc attempt)) result))))))))))

(defn append! [port subject predicate value]
  (publish! port [{:op :assert :subject subject :predicate predicate :value value :cardinality :many}]))

(defn put! [port subject predicate value]
  (publish! port [{:op :assert :subject subject :predicate predicate :value value :cardinality :one}]))

(defn retract!
  ([port subject predicate value]
    (publish! port [{:op :retract :subject subject :predicate predicate :value value}]))
  ([port subject predicate value expected-version]
    (retract-at-version! port subject predicate value expected-version)))

(defn put-with-fence! [port fence subject predicate value]
  (publish! port [{:op :assert :subject subject :predicate predicate :value value :cardinality :one}] {:fence fence}))

(defn retract-with-fence! [port fence subject predicate value]
  (publish! port [{:op :retract :subject subject :predicate predicate :value value}] {:fence fence}))

(defn subject-readback!
  ([port subject desired]
    (subject-readback! port subject desired {}))
  ([port subject desired options]
    (with-client! port (domain-for-subject subject) (fn [%1] (north.store-rpc-client/subject-readback! %1 subject desired options)))))

(def assert-after-read-deadline-ms 30000)

(def ^:dynamic *retry-monotonic-now-ns* (fn [] (System/nanoTime)))

(def ^:dynamic *retry-sleep-ms!* (fn [%1] (Thread/sleep %1)))

(defn retry-deadline-ns
  ([]
    (request-deadline-ns assert-after-read-deadline-ms))
  ([timeout-ms]
    (request-deadline-ns timeout-ms)))

(defn retry-conflicts-until!
  ([deadline operation]
    (retry-conflicts-until! deadline Integer/MAX_VALUE operation))
  ([deadline attempts operation]
    (loop [remaining (long attempts)
   backoff 1]
  (if (< (long (*retry-monotonic-now-ns*)) deadline) (let [result (operation)]
  (if (and (= :conflict (:reject result)) (> remaining 1)) (do
  (*retry-sleep-ms!* backoff)
  (recur (dec remaining) (min 64 (* 2 backoff)))) result)) {:reject :deadline}))))

(defn assert-after-read!
  ([port subject predicate value validate!]
    (assert-after-read! port subject predicate value validate! Integer/MAX_VALUE))
  ([port subject predicate value validate! attempts]
    (retry-conflicts-until! (retry-deadline-ns) attempts (fn [] (let [base (cur-ver-for-subject! port subject)]
  (validate!)
  (assert-at-version! port subject predicate value base))))))

(defn assert-after-read-with-fence!
  ([port fence subject predicate value validate!]
    (assert-after-read-with-fence! port fence subject predicate value validate! 16))
  ([port fence subject predicate value validate! attempts]
    (retry-conflicts-until! (retry-deadline-ns) attempts (fn [] (let [base (cur-ver-for-subject! port subject)]
  (validate!)
  (transact! port [{:op :assert :subject subject :predicate predicate :value value}] {:expected-version base :fence fence}))))))

(defn assert-batch-after-read!
  ([port subject plan!]
    (assert-batch-after-read! port subject plan! Integer/MAX_VALUE (retry-deadline-ns)))
  ([port subject plan! attempts deadline]
    (retry-conflicts-until! deadline attempts (fn [] (let [base (cur-ver-for-subject! port subject)
   planned (plan!)]
  (if (contains? planned :done) planned (let [facts (:facts planned)]
  (if (not (and (vector? facts) (seq facts))) (do
  (throw (ex-info "planned publication requires non-empty :facts" {:type :invalid-planned-publication}))))
  (publish! port (mapv (fn [fact] {:op :assert :subject subject :predicate (or (:predicate fact) (:p fact)) :value (if (contains? fact :value) (:value fact) (:r fact)) :cardinality (or (:cardinality fact) :many)}) facts) {:expected-version base}))))))))

(defn- instant-millis [instant]
  (+ (* 1000 (t/instant-epoch-seconds instant)) (quot (t/instant-nanos instant) 1000000)))

(defn- fence-map [fence]
  (let [[resource holder epoch] (north.store-rpc-client/fence-parts fence)]
  {:resource resource :holder holder :epoch epoch}))

(defn authoritative-lease? [lease]
  (boolean (and (map? lease) (some? (:resource lease)) (some? (:holder lease)) (integer? (:epoch lease)) (<= 1 (:epoch lease) lease-max-safe-integer))))

(defn acquire-lease! [port resource holder ttl-ms]
  (with-client! port :coordination (fn [client] (try
  (let [result (north.store-rpc-client/lease-acquire! client resource holder ttl-ms)
   fence (fence-map (:fence result))]
  (merge {:ok (:epoch fence) :exp (instant-millis (:expires result)) :served-version (:served-version result)} fence))
  (catch clojure.lang.ExceptionInfo error
    (if (= :rpc/lease-held (:type (ex-data error))) {:reject :held :version (:served-version (ex-data error))} (throw error)))))))

(defn renew-lease! [port fence ttl-ms]
  (with-client! port :coordination (fn [client] (try
  (let [result (north.store-rpc-client/lease-renew! client (fence-term fence) ttl-ms)
   next (fence-map (:fence result))]
  (merge {:ok (:epoch next) :exp (instant-millis (:expires result)) :served-version (:served-version result)} next))
  (catch clojure.lang.ExceptionInfo error
    (if (= :rpc/lease-fence-mismatch (:type (ex-data error))) {:reject :fence-lost :version (:served-version (ex-data error))} (throw error)))))))

(defn release-lease! [port fence]
  (with-client! port :coordination (fn [client] (let [result (north.store-rpc-client/lease-release! client (fence-term fence))]
  {:ok (:epoch fence) :released? (:released? result) :served-version (:served-version result)}))))

(defn check-lease! [port fence]
  (with-client! port :coordination (fn [client] (let [result (north.store-rpc-client/lease-check! client (fence-term fence))]
  {:valid? (:valid? result) :exp (let [expires (:expires result)]
  (if expires (do
  (instant-millis expires)))) :served-version (:served-version result)}))))

(defn lease-status! [port resource]
  (let [now (System/currentTimeMillis)
   result (with-client! port :coordination (fn [client] (north.store-rpc-client/scan-all! client resource :kernel/lease nil {:page-size 2})))
   rows (:rows result)]
  (if (> (count rows) 1) (do
  (throw (ex-info "lease resource has multiple live propositions" {:type :duplicate-resource-lease :resource resource}))))
  (let [proposition (first rows)]
  (if proposition (let [lease (t/triple-t3 proposition)]
  (if (not (and (t/triple? proposition) (= resource (t/triple-t1 proposition)) (= :kernel/lease (t/triple-t2 proposition)) (t/triple? lease) (= :kernel/expires-at (t/triple-t2 lease)) (integer? (t/triple-t3 lease)))) (do
  (throw (ex-info "lease projection is malformed" {:type :malformed-resource-lease :resource resource}))))
  (let [holder (t/triple-t1 lease)
   exp (t/triple-t3 lease)]
  {:resource resource :holder holder :exp exp :online? (> exp now) :version (:served-version result)})) {:resource resource :holder nil :exp nil :online? false :version (:served-version result)}))))

(def ^:private session-resource-prefix "session:")

(defn- parse-session-lease! [proposition]
  (let [resource (if (t/triple? proposition) (do
  (t/triple-t1 proposition)))]
  (if (and (string? resource) (str/starts-with? resource session-resource-prefix)) (do
  (let [handle (subs resource (count session-resource-prefix))
   value (t/triple-t3 proposition)]
  (if (not (and (not (str/blank? handle)) (t/triple? value) (= handle (t/triple-t1 value)) (= :kernel/expires-at (t/triple-t2 value)) (integer? (t/triple-t3 value)) (<= 0 (t/triple-t3 value) lease-max-safe-integer))) (do
  (throw (ex-info "session lease proposition is malformed" {:type :malformed-session-lease :resource resource}))))
  {:handle handle :holder handle :exp (t/triple-t3 value)})))))

(defn- session-lease-scan! [port resource]
  (with-client! port :coordination (fn [client] (north.store-rpc-client/scan-all! client resource :kernel/lease nil {:page-size north.store-rpc-client/effective-page-limit}))))

(defn- session-lease-index! [rows]
  (reduce (fn [known proposition] (let [bind__7 (parse-session-lease! proposition)]
  (if bind__7 (let [{:keys [handle] :as lease} bind__7]
  (if (contains? known handle) (throw (ex-info "session has multiple live lease propositions" {:type :duplicate-session-lease :handle handle})) (assoc known handle lease))) known))) {} rows))

(defn- all-session-status! [port now]
  (if (not (and (integer? now) (not (neg? now)))) (do
  (throw (ex-info "session status requires a non-negative observation time" {:type :invalid-session-observation-time :now now}))))
  (let [result (session-lease-scan! port nil)
   sessions (into {} (map (fn [[handle lease]] [handle (assoc lease :online? (> (:exp lease) now))])) (session-lease-index! (:rows result)))]
  {:version (:served-version result) :sessions sessions}))

(defn online-session-leases! [port now]
  (->> (:sessions (all-session-status! port now)) vals (filter (fn [status] (:online? status))) (sort-by (fn [status] (:handle status))) (mapv (fn [status] (select-keys status [:handle :exp]))) vec))

(defn sessions-status! [port handles]
  (let [handles (vec (distinct handles))]
  (if (not (every? (fn [%1] (and (string? %1) (not (str/blank? %1)))) handles)) (do
  (throw (ex-info "session handles must be nonblank strings" {:type :invalid-session-handles}))))
  (let [{:keys [version sessions]} (all-session-status! port (System/currentTimeMillis))]
  {:version version :sessions (into {} (map (fn [handle] [handle (get sessions handle {:holder nil :online? false :exp nil})]) handles))})))

(defn online-session-handles!
  ([port]
    (online-session-handles! port (System/currentTimeMillis)))
  ([port now]
    (let [{:keys [version sessions]} (all-session-status! port now)]
  {:version version :handles (into (sorted-set) (keep (fn [[handle status]] (if (:online? status) (do
  handle)))) sessions)})))

(defn session-lease-status! [port handle]
  (get-in (sessions-status! port [handle]) [:sessions handle]))

(defn session-online?! [port handle]
  (boolean (:online? (session-lease-status! port handle))))

(def distinct-reducer {:init #{} :step (fn [values row] (conj values (first row))) :final identity})

(def sum-reducer {:init 0 :step (fn [total row] (+ total (double (or (parse-double (str (second row))) 0)))) :final identity})

(defn reduce-rows [{:keys [init step final]} rows]
  (final (reduce step init rows)))

(defn sum-rows [rows]
  (reduce-rows sum-reducer rows))

(defn distinct-rows [rows]
  (reduce-rows distinct-reducer rows))

(defn agg-rows! [port project body]
  (query-rows! port {:find "north_aggregate" :rules [{:head {:rel "north_aggregate" :args (mapv (fn [name] {:var name}) project)} :body body}]}))

(defn aggregate! [port project body reducer]
  (reduce-rows reducer (agg-rows! port project body)))

(defn distinct-of! [port project body]
  (aggregate! port project body distinct-reducer))

(defn count-distinct! [port project body]
  (count (distinct-of! port project body)))

(defn sum-of! [port project body]
  (aggregate! port project body sum-reducer))

(defn quorum-met?! [port k project body]
  (>= (count-distinct! port project body) k))

(defn pending-cmds! [port]
  (query-rows! port {:find "pending" :strata [[{:head {:rel "settled" :args [{:var "command"}]} :body [{:rel "triple" :args [{:var "command"} "acked_by" {:var "agent"}]}]} {:head {:rel "settled" :args [{:var "command"}]} :body [{:rel "triple" :args [{:var "command"} "failed_by" {:var "agent"}]}]}] [{:head {:rel "pending" :args [{:var "command"} {:var "operation"} {:var "target"}]} :body [{:rel "triple" :args [{:var "command"} "op" {:var "operation"}]} {:rel "triple" :args [{:var "command"} "target" {:var "target"}]} {:rel "settled" :args [{:var "command"}] :neg true}]}]]}))

(defn -main [& $beagle$rest$host]
  (let [args (vec $beagle$rest$host)]
  (let [port (parse-port! (or (first args) PORT))
   domain (case (second args)
    nil :coordination
    "coordination" :coordination
    "telemetry" :telemetry
    (throw (ex-info "coord status domain must be coordination or telemetry" {:type :invalid-domain :domain (second args)})))]
  (prn (status-in-domain! port domain)))))

(if (and (= *file* (System/getProperty "babashka.file")) (not (contains? #{"-e" "-m"} (first *command-line-args*)))) (do
  (apply -main *command-line-args*)))
