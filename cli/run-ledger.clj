(ns north.run-ledger
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def repo-root (some-> (or *file* (System/getProperty "babashka.file")) io/file .getCanonicalFile .getParentFile .getParentFile str))

(def contract (json/parse-string (slurp (str repo-root "/contracts/agent-run-ledger-v2.json"))))

(def version (get contract "version"))

(def wire-version (get contract "wireVersion"))

(def max-events (get-in contract ["bounds" "maxEventsPerRun"]))

(def max-event-bytes (get-in contract ["bounds" "maxCanonicalEventBytes"]))

(def max-batch-events (get-in contract ["bounds" "maxBatchEvents"]))

(def max-projection-batch-bytes (get-in contract ["bounds" "maxProjectionBatchBytes"]))

(def max-telemetry-projection-bytes (get-in contract ["bounds" "maxTelemetryProjectionBytes"]))

(def event-predicates (set (get contract "predicates")))

(def required-event-predicates #{"kind" "wire_ledger_version" "wire_version" "wire_run_id" "thread" "agent" "wire_event_id" "wire_event_sequence" "wire_event_at" "wire_event_kind" "wire_event_essential" "wire_event_json" "wire_event_sha256"})

(def identifier-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$")

(def wire-id-pattern #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$")

(def entity-pattern #"^@?[A-Za-z0-9][A-Za-z0-9_.:-]{0,255}$")

(def digest-pattern #"^[a-f0-9]{64}$")

(defn fail! [message data]
  (throw (ex-info message data)))

(defn- canonical-value [value]
  (cond
  (map? value) (into (sorted-map) (map (fn [[k v]] [k (canonical-value v)])) value)
  (sequential? value) (mapv canonical-value value)
  :else value))

(defn canonical-json [value]
  (json/generate-string (canonical-value value)))

(defn sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8))]
  (apply str (map (fn [__north_anon_1] (format "%02x" (bit-and 255 __north_anon_1))) digest))))

(defn canonical-entity! [value label]
  (if (not (and (string? value) (re-matches entity-pattern value))) (do
  (fail! (str "invalid wire ledger " label) {:value value})))
  (if (str/starts-with? value "@") value (str "@" value)))

(defn event-subject [run sequence]
  (str "@run:wire-event-" (sha256 (str "north-wire-event-subject:v2\u0000" run "\u0000" sequence))))

(defn run-summary-subject! [run]
  (if (not (and (string? run) (re-matches wire-id-pattern run))) (do
  (fail! "invalid wire run id" {:value run})))
  (if (re-matches #"^run:[A-Za-z0-9][A-Za-z0-9._:-]*$" run) (str "@" run) (str "@run:wire-summary-" (sha256 (str "north-wire-run-summary-subject:v2\u0000" run)))))

(defn- singleton-map! [facts]
  (let [grouped (group-by first facts)]
  (doseq [[predicate entries] grouped]
  (if (> (count entries) 1) (do
  (fail! "wire event predicates must be singleton" {:predicate predicate :values (mapv second entries)}))))
  (into {} (map (fn [[predicate entries]] [predicate (second (first entries))])) grouped)))

(defn- parse-sequence! [value]
  (let [sequence (parse-long (or value ""))]
  (if (not (and sequence (<= 0 sequence) (< sequence max-events))) (do
  (fail! "invalid wire event sequence" {:value value})))
  sequence))

(defn- parse-instant! [value]
  (try
  (java.time.Instant/parse value)
  (catch Exception _
    (fail! "invalid wire event timestamp" {:value value}))))

(defn- parse-event-json! [raw]
  (if (not (and (string? raw) (<= (alength (.getBytes raw java.nio.charset.StandardCharsets/UTF_8)) max-event-bytes) (not (str/includes? raw "\n")) (not (str/includes? raw "\r")))) (do
  (fail! "wire event JSON is missing, multiline, or oversized" {})))
  (let [event (try
  (json/parse-string raw)
  (catch Exception error
    (fail! "invalid wire event JSON" {:cause (.getMessage error)})))]
  (if (not (map? event)) (do
  (fail! "wire event JSON must encode an object" {})))
  event))

(defn validate-event-facts! [subject facts]
  (let [unknown (seq (remove event-predicates (map first facts)))
   scalar (singleton-map! facts)
   missing (seq (remove (fn [__north_anon_1] (contains? scalar __north_anon_1)) required-event-predicates))
   raw (get scalar "wire_event_json")
   event (parse-event-json! raw)
   sequence (parse-sequence! (get scalar "wire_event_sequence"))
   run (get scalar "wire_run_id")
   thread (if (= "(ad-hoc)" (get scalar "thread")) "(ad-hoc)" (canonical-entity! (get scalar "thread") "thread"))
   agent (get scalar "agent")
   event-id (get scalar "wire_event_id")
   event-kind (get scalar "wire_event_kind")
   event-at (get scalar "wire_event_at")
   essential (get scalar "wire_event_essential")
   digest (sha256 raw)
   expected-subject (event-subject run sequence)]
  (if unknown (do
  (fail! "wire event contains unknown predicates" {:predicates unknown})))
  (if missing (do
  (fail! "wire event is missing required predicates" {:predicates missing})))
  (if (not (= "wire_event" (get scalar "kind"))) (do
  (fail! "wire event requires kind=wire_event" {})))
  (if (not (= version (get scalar "wire_ledger_version"))) (do
  (fail! "unsupported wire ledger version" {:version (get scalar "wire_ledger_version")})))
  (if (not (and (= wire-version (get scalar "wire_version")) (= wire-version (get event "version")))) (do
  (fail! "wire event version mismatch" {})))
  (if (not (and (string? agent) (re-matches identifier-pattern agent))) (do
  (fail! "invalid wire event agent" {})))
  (if (not (and (string? event-id) (re-matches wire-id-pattern event-id))) (do
  (fail! "invalid wire event id" {})))
  (if (not (and (string? run) (re-matches wire-id-pattern run))) (do
  (fail! "invalid wire run id" {})))
  (if (not (and (string? event-kind) (not (str/blank? event-kind)) (<= (count event-kind) 128))) (do
  (fail! "invalid wire event kind" {})))
  (if (not (#{"true" "false"} essential)) (do
  (fail! "invalid wire event essential flag" {})))
  (parse-instant! event-at)
  (let [bind__5 (get scalar "parent_thread")]
  (if bind__5 (let [parent-thread bind__5]
  (do
  (canonical-entity! parent-thread "parent_thread")))))
  (let [bind__6 (get scalar "run_coordinator")]
  (if bind__6 (let [coordinator bind__6]
  (do
  (if (not (some? (re-matches identifier-pattern coordinator))) (do
  (fail! "invalid wire event coordinator" {})))))))
  (if (not (and (= run (get event "runId")) (= sequence (get event "sequence")) (= event-id (get event "id")) (= event-kind (get event "kind")) (= event-at (get event "at")) (= (= essential "true") (get event "essential")))) (do
  (fail! "wire event envelope differs from its indexed facts" {})))
  (if (not (and (vector? (get event "requiredSemantics")) (every? (fn [__north_anon_1] (and (string? __north_anon_1) (not (str/blank? __north_anon_1)))) (get event "requiredSemantics")))) (do
  (fail! "wire event requiredSemantics is malformed" {})))
  (if (not (and (re-matches digest-pattern (or (get scalar "wire_event_sha256") "")) (= digest (get scalar "wire_event_sha256")))) (do
  (fail! "wire event digest mismatch" {:expected digest})))
  (if (not (= expected-subject (canonical-entity! subject "event subject"))) (do
  (fail! "wire event subject does not match run and sequence" {:expected expected-subject :actual subject})))
  {"subject" expected-subject "run" run "thread" thread "agent" agent "parentThread" (get scalar "parent_thread") "coordinator" (get scalar "run_coordinator") "sequence" sequence "id" event-id "at" event-at "kind" event-kind "essential" (= essential "true") "json" raw "digest" digest "event" event}))

(defn ledger-digest [events]
  (sha256 (canonical-json (mapv (fn [__north_anon_1] (get __north_anon_1 "digest")) events))))

(defn timeline [run-id events]
  (let [run run-id
   ordered (vec (sort-by (fn [__north_anon_1] (get __north_anon_1 "sequence")) events))
   sequences (mapv (fn [__north_anon_1] (get __north_anon_1 "sequence")) ordered)
   expected (vec (range (count ordered)))
   terminal (last ordered)]
  {:run run :thread (get (first ordered) "thread") :agent (get (first ordered) "agent") :parent-thread (get (first ordered) "parentThread") :coordinator (get (first ordered) "coordinator") :events ordered :valid-order? (= expected sequences) :finalized? (and (= "run.terminated" (get terminal "kind")) (= (dec (count ordered)) (get terminal "sequence"))) :digest (if (seq ordered) (do
  (ledger-digest ordered)))}))
