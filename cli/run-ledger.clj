(ns north.run-ledger
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def ^:private repo-root
  (some-> (or *file* (System/getProperty "babashka.file")) io/file .getCanonicalFile
          .getParentFile .getParentFile str))
(def contract
  (json/parse-string
   (slurp (str repo-root "/contracts/agent-run-ledger-v2.json"))))
(def version (get contract "version"))
(def wire-version (get contract "wireVersion"))
(def max-events (get-in contract ["bounds" "maxEventsPerRun"]))
(def max-event-bytes (get-in contract ["bounds" "maxCanonicalEventBytes"]))
(def max-batch-events (get-in contract ["bounds" "maxBatchEvents"]))
(def max-projection-batch-bytes (get-in contract ["bounds" "maxProjectionBatchBytes"]))
(def max-telemetry-projection-bytes
  (get-in contract ["bounds" "maxTelemetryProjectionBytes"]))
(def event-predicates (set (get contract "predicates")))

(def ^:private required-event-predicates
  #{"kind" "wire_ledger_version" "wire_version" "wire_run_id" "thread" "agent"
    "wire_event_id" "wire_event_sequence" "wire_event_at" "wire_event_kind"
    "wire_event_essential" "wire_event_json" "wire_event_sha256"})
(def ^:private identifier-pattern #"^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$")
(def ^:private wire-id-pattern #"^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$")
(def ^:private entity-pattern #"^@?[A-Za-z0-9][A-Za-z0-9_.:-]{0,255}$")
(def ^:private digest-pattern #"^[a-f0-9]{64}$")

(defn fail! [message data] (throw (ex-info message data)))

(defn- canonical-value [value]
  (cond
    (map? value) (into (sorted-map) (map (fn [[k v]] [k (canonical-value v)])) value)
    (sequential? value) (mapv canonical-value value)
    :else value))

(defn canonical-json [value]
  (json/generate-string (canonical-value value)))

(defn sha256 [value]
  (let [digest (.digest (java.security.MessageDigest/getInstance "SHA-256")
                        (.getBytes (str value) java.nio.charset.StandardCharsets/UTF_8))]
    (apply str (map #(format "%02x" (bit-and 0xff %)) digest))))

(defn canonical-entity [value label]
  (when-not (and (string? value) (re-matches entity-pattern value))
    (fail! (str "invalid wire ledger " label) {:value value}))
  (if (str/starts-with? value "@") value (str "@" value)))

(defn event-subject [run sequence]
  (str "@run:wire-event-"
       (sha256 (str "north-wire-event-subject:v2\u0000" run "\u0000" sequence))))

(defn run-summary-subject [run]
  (when-not (and (string? run) (re-matches wire-id-pattern run))
    (fail! "invalid wire run id" {:value run}))
  (if (re-matches #"^run:[A-Za-z0-9][A-Za-z0-9._:-]*$" run)
    (str "@" run)
    (str "@run:wire-summary-"
         (sha256 (str "north-wire-run-summary-subject:v2\u0000" run)))))

(defn- singleton-map [facts]
  (let [grouped (group-by first facts)]
    (doseq [[predicate entries] grouped]
      (when (> (count entries) 1)
        (fail! "wire event predicates must be singleton"
               {:predicate predicate :values (mapv second entries)})))
    (into {} (map (fn [[predicate entries]] [predicate (second (first entries))])) grouped)))

(defn- parse-sequence [value]
  (let [sequence (parse-long (or value ""))]
    (when-not (and sequence (<= 0 sequence) (< sequence max-events))
      (fail! "invalid wire event sequence" {:value value}))
    sequence))

(defn- parse-instant! [value]
  (try
    (java.time.Instant/parse value)
    (catch Exception _ (fail! "invalid wire event timestamp" {:value value}))))

(defn- parse-event-json! [raw]
  (when-not (and (string? raw)
                 (<= (alength (.getBytes raw java.nio.charset.StandardCharsets/UTF_8))
                     max-event-bytes)
                 (not (str/includes? raw "\n"))
                 (not (str/includes? raw "\r")))
    (fail! "wire event JSON is missing, multiline, or oversized" {}))
  (let [event (try
                (json/parse-string raw)
                (catch Exception error
                  (fail! "invalid wire event JSON" {:cause (.getMessage error)})))]
    (when-not (map? event) (fail! "wire event JSON must encode an object" {}))
    ;; TypeScript's encodeWireJsonlLine is the canonical-byte authority. Do not
    ;; reserialize here: Cheshire and ECMAScript format exponent numbers
    ;; differently. The exact bytes are retained and authenticated below.
    event))

(defn validate-event-facts! [subject facts]
  (let [unknown (seq (remove event-predicates (map first facts)))
        scalar (singleton-map facts)
        missing (seq (remove #(contains? scalar %) required-event-predicates))
        raw (get scalar "wire_event_json")
        event (parse-event-json! raw)
        sequence (parse-sequence (get scalar "wire_event_sequence"))
        run (get scalar "wire_run_id")
        thread (if (= "(ad-hoc)" (get scalar "thread"))
                 "(ad-hoc)"
                 (canonical-entity (get scalar "thread") "thread"))
        agent (get scalar "agent")
        event-id (get scalar "wire_event_id")
        event-kind (get scalar "wire_event_kind")
        event-at (get scalar "wire_event_at")
        essential (get scalar "wire_event_essential")
        digest (sha256 raw)
        expected-subject (event-subject run sequence)]
    (when unknown (fail! "wire event contains unknown predicates" {:predicates unknown}))
    (when missing (fail! "wire event is missing required predicates" {:predicates missing}))
    (when-not (= "wire_event" (get scalar "kind"))
      (fail! "wire event requires kind=wire_event" {}))
    (when-not (= version (get scalar "wire_ledger_version"))
      (fail! "unsupported wire ledger version" {:version (get scalar "wire_ledger_version")}))
    (when-not (= wire-version (get scalar "wire_version") (get event "version"))
      (fail! "wire event version mismatch" {}))
    (when-not (and (string? agent) (re-matches identifier-pattern agent))
      (fail! "invalid wire event agent" {}))
    (when-not (and (string? event-id) (re-matches wire-id-pattern event-id))
      (fail! "invalid wire event id" {}))
    (when-not (and (string? run) (re-matches wire-id-pattern run))
      (fail! "invalid wire run id" {}))
    (when-not (and (string? event-kind) (not (str/blank? event-kind))
                   (<= (count event-kind) 128))
      (fail! "invalid wire event kind" {}))
    (when-not (#{"true" "false"} essential)
      (fail! "invalid wire event essential flag" {}))
    (parse-instant! event-at)
    (when-let [parent-thread (get scalar "parent_thread")]
      (canonical-entity parent-thread "parent_thread"))
    (when-let [coordinator (get scalar "run_coordinator")]
      (when-not (re-matches identifier-pattern coordinator)
        (fail! "invalid wire event coordinator" {})))
    (when-not (and (= run (get event "runId"))
                   (= sequence (get event "sequence"))
                   (= event-id (get event "id"))
                   (= event-kind (get event "kind"))
                   (= event-at (get event "at"))
                   (= (= essential "true") (get event "essential")))
      (fail! "wire event envelope differs from its indexed facts" {}))
    (when-not (and (vector? (get event "requiredSemantics"))
                   (every? #(and (string? %) (not (str/blank? %)))
                           (get event "requiredSemantics")))
      (fail! "wire event requiredSemantics is malformed" {}))
    (when-not (and (re-matches digest-pattern (or (get scalar "wire_event_sha256") ""))
                   (= digest (get scalar "wire_event_sha256")))
      (fail! "wire event digest mismatch" {:expected digest}))
    (when-not (= expected-subject (canonical-entity subject "event subject"))
      (fail! "wire event subject does not match run and sequence"
             {:expected expected-subject :actual subject}))
    {"subject" expected-subject
     "run" run
     "thread" thread
     "agent" agent
     "parentThread" (get scalar "parent_thread")
     "coordinator" (get scalar "run_coordinator")
     "sequence" sequence
     "id" event-id
     "at" event-at
     "kind" event-kind
     "essential" (= essential "true")
     "json" raw
     "digest" digest
     "event" event}))

(defn ledger-digest [events]
  (sha256 (canonical-json (mapv #(get % "digest") events))))

(defn timeline [run-id events]
  (let [run run-id
        ordered (vec (sort-by #(get % "sequence") events))
        sequences (mapv #(get % "sequence") ordered)
        expected (vec (range (count ordered)))
        terminal (last ordered)]
    {:run run
     :thread (get (first ordered) "thread")
     :agent (get (first ordered) "agent")
     :parent-thread (get (first ordered) "parentThread")
     :coordinator (get (first ordered) "coordinator")
     :events ordered
     :valid-order? (= expected sequences)
     :finalized? (and (= "run.terminated" (get terminal "kind"))
                      (= (dec (count ordered)) (get terminal "sequence")))
     :digest (when (seq ordered) (ledger-digest ordered))}))
