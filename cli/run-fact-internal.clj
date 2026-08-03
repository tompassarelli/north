#!/usr/bin/env bb
;; Harness-owned run telemetry publication. Every terminal fact, including
;; kind=run, commits in one transaction after its complete read set is validated.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))

(defn fail! [message data] (throw (ex-info message data)))

(defn checked! [result operation]
  (when (:reject result)
    (fail! "coordinator rejected run telemetry publication" {:operation operation}))
  result)

(defn entity [subject]
  (let [raw (str subject)
        canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
    (when-not (north.terminal-projection/valid-run-entity? canonical)
      (fail! "invalid run telemetry subject" {:subject subject}))
    canonical))

(defn thread-entity [raw]
  (when (and (string? raw) (not= raw "(ad-hoc)"))
    (let [canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
      (when-not (north.terminal-projection/valid-thread-entity? canonical)
        (fail! "invalid run telemetry thread" {:thread raw}))
      canonical)))

(defn payload [raw]
  (let [parsed (try (json/parse-string (str raw))
                    (catch Exception error
                      (fail! "invalid run telemetry JSON" {:cause (.getMessage error)})))]
    (when-not (sequential? parsed)
      (fail! "run telemetry payload must be an array" {}))
    (mapv (fn [entry]
            (when-not (and (sequential? entry) (= 2 (count entry))
                           (every? string? entry)
                           (every? #(not (str/blank? %)) entry))
              (fail! "run telemetry facts must be nonblank string pairs" {:entry entry}))
            (vec entry))
          parsed)))

(defn facts-of [port subject]
  (let [rows (:ok (north.coord/send-op
                   port {:op :query
                         :query {:find "run_writer_fact"
                                 :rules [{:head {:rel "run_writer_fact"
                                                 :args [{:var "p"} {:var "r"}]}
                                          :body [{:rel "triple"
                                                  :args [subject {:var "p"} {:var "r"}]}]}]}}))]
    (reduce (fn [acc [predicate value]]
              (update acc predicate (fnil conj #{}) value))
            {}
            rows)))

(defn canonical-record [record]
  (json/generate-string (into (sorted-map) record)))

(def operation-tool-pattern #"[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}")
(def operation-component-pattern #"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}")

(defn parse-operation-json! [label raw]
  (try (json/parse-string raw)
       (catch Exception error
         (fail! (str "invalid " label " JSON") {:cause (.getMessage error)}))))

(defn validate-operation-evidence! [receipt-entries aggregate-entries]
  (when (or (> (count receipt-entries) 512) (> (count aggregate-entries) 512))
    (fail! "MCP operation evidence exceeds the bounded receipt limit" {}))
  (let [receipts
        (mapv (fn [[_ raw]]
                (let [record (parse-operation-json! "MCP operation receipt" raw)
                      keys' (set (keys record))]
                  (when-not (or (= #{"tool" "operation" "durationMs" "outcome" "resultSize"} keys')
                                (= #{"tool" "operation" "durationMs" "batchSize" "outcome" "resultSize"} keys'))
                    (fail! "MCP operation receipt requires the exact v1 field set" {}))
                  (when-not (and (string? (get record "tool"))
                                 (re-matches operation-tool-pattern (get record "tool"))
                                 (every? #(and (string? %) (re-matches operation-component-pattern %))
                                         ((juxt #(get % "operation") #(get % "outcome")) record))
                                 (every? #(and (integer? %) (<= 0 %))
                                         (cond-> [(get record "durationMs") (get record "resultSize")]
                                           (contains? record "batchSize") (conj (get record "batchSize")))))
                    (fail! "MCP operation receipt contains invalid values" {:record record}))
                  record)) receipt-entries)
        aggregates
        (mapv (fn [[_ raw]]
                (let [record (parse-operation-json! "MCP operation aggregate" raw)
                      count' (get record "count")
                      total (get record "totalDurationMs")
                      mean (get record "meanDurationMs")
                      failures (get record "failureCount")]
                  (when-not (= #{"operation" "count" "totalDurationMs" "meanDurationMs" "failureCount"}
                               (set (keys record)))
                    (fail! "MCP operation aggregate requires the exact v1 field set" {}))
                  (when-not (and (string? (get record "operation"))
                                 (re-matches operation-component-pattern (get record "operation"))
                                 (integer? count') (pos? count')
                                 (integer? total) (<= 0 total)
                                 (number? mean) (= (double mean) (/ (double total) count'))
                                 (integer? failures) (<= 0 failures count'))
                    (fail! "MCP operation aggregate contains invalid values" {:record record}))
                  record)) aggregate-entries)
        derived (reduce (fn [result receipt]
                          (update result (get receipt "operation")
                                  (fnil (fn [entry]
                                          (-> entry
                                              (update "count" inc)
                                              (update "totalDurationMs" + (get receipt "durationMs"))
                                              (update "failureCount" + (if (= "ok" (get receipt "outcome")) 0 1))))
                                        {"count" 0 "totalDurationMs" 0 "failureCount" 0})))
                        {} receipts)]
    (when-not (= (set (keys derived)) (set (map #(get % "operation") aggregates)))
      (fail! "MCP operation aggregates do not cover the exact receipt operations" {}))
    (when-not (= (count derived) (count aggregates))
      (fail! "MCP operation aggregates must be unique by operation" {}))
    (doseq [aggregate aggregates
            :let [expected (get derived (get aggregate "operation"))]]
      (when-not (= expected (select-keys aggregate ["count" "totalDurationMs" "failureCount"]))
        (fail! "MCP operation aggregate does not reconcile with receipts"
               {:operation (get aggregate "operation")})))))

(defn validate-native-operation-evidence! [entries]
  (when (> (count entries) 32)
    (fail! "native command completion evidence exceeds the bounded receipt limit" {}))
  (doseq [[_ raw] entries
          :let [record (parse-operation-json! "native command completion" raw)]]
    (when-not (= #{"commandSha256" "outputSha256" "status" "exitCode" "shape" "durationMs"}
                 (set (keys record)))
      (fail! "native command completion requires the exact duration-bearing field set" {}))
    (when-not (and (every? #(boolean (re-matches #"[a-f0-9]{64}" (or % "")))
                           ((juxt #(get % "commandSha256") #(get % "outputSha256")) record))
                   (#{"completed" "failed" "declined"} (get record "status"))
                   (#{"read" "edit" "other"} (get record "shape"))
                   (integer? (get record "exitCode"))
                   (integer? (get record "durationMs"))
                   (<= 0 (get record "durationMs")))
      (fail! "native command completion contains invalid operation evidence" {}))))

(defn validate-reported-run! [port subject scalar delivery-facts run-facts]
  (when (= "reported" (get delivery-facts "delivery_outcome"))
    (let [evidence (json/parse-string (get delivery-facts "delivery_evidence"))
          expected-reporter (str "@agent:" (get scalar "agent"))
          expected-thread (thread-entity (get scalar "thread"))
          reservation-origin
          (north.terminal-projection/singleton-value
           run-facts "run_reservation_contract_origin")
          reservation-baseline
          (north.terminal-projection/run-reservation-done-when run-facts)
          current-bars
          (north.terminal-projection/canonical-done-when
           (facts-of port expected-thread))
          records
          (set
           (mapcat (fn [match]
                     (map canonical-record (get match "evidence")))
                   (get evidence "matches")))
          evidence-state
          (north.terminal-projection/run-evidence-state
           run-facts subject expected-thread expected-reporter)
          stored-records (:raws evidence-state)]
      (when-not (north.terminal-projection/run-reservation-valid? run-facts)
        (fail! "reported run lost its committed reservation"
               {:subject subject}))
      (when-not (= #{expected-reporter} (get run-facts "run_reservation_agent"))
        (fail! "run telemetry agent does not match its reservation"
               {:expected expected-reporter :subject subject}))
      (when-not (= #{expected-thread} (get run-facts "run_reservation_thread"))
        (fail! "run telemetry thread does not match its reservation"
               {:expected expected-thread :subject subject}))
      (when-not (= expected-reporter (get evidence "reporter"))
        (fail! "run evidence reporter must match its managed agent"
               {:expected expected-reporter :reporter (get evidence "reporter")}))
      (when-not (= subject (get evidence "run"))
        (fail! "run evidence must name the exact committed run subject"
               {:expected subject :run (get evidence "run")}))
      (when-not (= expected-thread (get evidence "thread"))
        (fail! "run evidence must name the exact driven thread"
               {:expected expected-thread :thread (get evidence "thread")}))
      (when-not (= reservation-origin (get evidence "contractOrigin"))
        (fail! "run delivery contract origin differs from its reservation"
               {:subject subject}))
      (when-not (= reservation-baseline (get evidence "baselineDoneWhen"))
        (fail! "run delivery baseline differs from its reservation"
               {:subject subject}))
      (when-not (= current-bars (get evidence "doneWhen"))
        (fail! "run delivery contract changed before telemetry publication"
               {:subject subject :thread expected-thread}))
      (when-not (:valid? evidence-state)
        (fail! "reported run contains malformed, cross-scoped, duplicate, or excessive evidence"
               {:subject subject}))
      (when-not (= stored-records records)
        (fail! "run delivery snapshot must cite the exact stored evidence set"
               {:subject subject
                :missing (vec (remove stored-records records))
                :uncited (vec (remove records stored-records))})))))

(let [[port-s subject-s raw] *command-line-args*
      port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
      subject (entity subject-s)
      facts (payload raw)
      kind-facts (filterv #(= "kind" (first %)) facts)
      body-facts (filterv #(not= "kind" (first %)) facts)
      grouped (group-by first body-facts)
      scalar (into {} (map (fn [[predicate entries]]
                             [predicate (second (last entries))]))
                   grouped)
      delivery-preds (set north.terminal-projection/terminal-projection-predicates)
      delivery-facts (select-keys scalar delivery-preds)
      learning-keys
      #{"learning_assignment_version" "learning_policy_version"
        "learning_policy_sha256" "learning_mode" "learning_evidence_mode"
        "learning_experiment_id" "learning_episode_id"
        "learning_task_signature_sha256" "learning_task_signature_coverage"
        "learning_risk" "learning_arm" "learning_axis" "learning_arm_id"
        "learning_propensity" "learning_explore_propensity"
        "learning_narrowing_reason" "learning_baseline_sha256"
        "learning_options_sha256" "learning_assignment_sha256"
        "graph_text_experiment_version" "graph_text_experiment_status"
        "graph_text_experiment_arm" "graph_text_experiment_applied"
        "graph_text_experiment_reason"
        "graph_text_experiment_assignment_sha256"}
      reservation-keys
      (conj (into (set north.terminal-projection/run-reservation-predicates)
                  learning-keys)
            "run_bar_evidence")
      terminal-learning-keys (set (filter learning-keys (keys grouped)))
      terminal-body-facts (filterv #(not (learning-keys (first %))) body-facts)]
  (validate-operation-evidence! (get grouped "mcp_operation_receipt" [])
                                (get grouped "mcp_operation_aggregate" []))
  (validate-native-operation-evidence! (get grouped "native_command_completion" []))
  (when-not (= [["kind" "run"]] kind-facts)
    (fail! "run telemetry requires exactly kind=run" {:kind-facts kind-facts}))
  (checked!
   (north.coord/assert-batch-after-read!
    port subject
    (fn []
      (let [before (facts-of port subject)
            unknown-before (seq (remove reservation-keys (keys before)))
            learning-before (select-keys before learning-keys)
            reserved? (north.terminal-projection/run-reservation-valid? before)]
        (when unknown-before
          (fail! "run subject reuse or partial prior publication is forbidden"
                 {:subject subject :predicates unknown-before}))
        (when (and (seq before) (not reserved?) (empty? learning-before))
          (fail! "run subject has a conflicting or incomplete reservation"
                 {:subject subject}))
        (when (contains? before "kind")
          (fail! "run subject is already committed" {:subject subject}))
        ;; The terminal payload proves the immutable pre-provider assignment but
        ;; does not publish a second occurrence of those facts.
        (when (and (seq terminal-learning-keys) (empty? learning-before))
          (fail! "terminal run cannot introduce a learning assignment after execution"
                 {:subject subject}))
        (when (seq learning-before)
          (when-not (= learning-keys (set (keys learning-before)))
            (fail! "pre-provider learning assignment is incomplete"
                   {:subject subject :predicates (keys learning-before)}))
          (when-not (= learning-keys terminal-learning-keys)
            (fail! "terminal run must repeat the complete pre-provider learning assignment"
                   {:subject subject :predicates terminal-learning-keys}))
          (doseq [predicate learning-keys
                  :let [expected (set (map second (get grouped predicate [])))
                        actual (get before predicate #{})]]
            (when-not (= expected actual)
              (fail! "terminal run learning assignment differs from pre-provider assignment"
                     {:subject subject :predicate predicate
                      :expected expected :actual actual}))))
        (when (and (= "reported" (get delivery-facts "delivery_outcome"))
                   (not reserved?))
          (fail! "reported delivery requires a committed pre-execution run reservation"
                 {:subject subject}))
        (when reserved?
          (let [expected-agent (str "@agent:" (get scalar "agent"))
                expected-thread (thread-entity (get scalar "thread"))]
            (when-not (= #{expected-agent} (get before "run_reservation_agent"))
              (fail! "run telemetry agent does not match its reservation"
                     {:expected expected-agent :subject subject}))
            (when-not (= #{expected-thread} (get before "run_reservation_thread"))
              (fail! "run telemetry thread does not match its reservation"
                     {:expected expected-thread :subject subject}))))
        (doseq [predicate delivery-preds
                :let [entries (get grouped predicate [])]
                :when (> (count entries) 1)]
          (fail! "run telemetry delivery predicates must be singleton"
                 {:predicate predicate :values (mapv second entries)}))
        (when (seq delivery-facts)
          (when-not (= (get delivery-facts "outcome")
                       (get delivery-facts "process_outcome"))
            (fail! "run legacy outcome must equal process_outcome" {}))
          (when-not (north.terminal-projection/delivery-projection-valid? delivery-facts)
            (fail! "run delivery outcome lacks a valid proof projection"
                   {:delivery-outcome (get delivery-facts "delivery_outcome")}))
          (validate-reported-run! port subject scalar delivery-facts before))
        {:facts (conj (mapv (fn [[predicate value]]
                              {:p predicate :r value})
                            terminal-body-facts)
                      {:p "kind" :r "run"})})))
   [:assert-batch-after-read subject])
  (let [stored (facts-of port subject)]
    (when-not (= #{"run"} (get stored "kind"))
      (fail! "run commit marker lost singleton race" {:subject subject}))
    (doseq [[predicate entries] grouped
            :let [expected (set (map second entries))
                  actual (get stored predicate #{})]]
      (when-not (set/subset? expected actual)
        (fail! "run telemetry readback is missing submitted facts"
               {:subject subject :predicate predicate
                :missing (remove actual expected)}))))
  (println (json/generate-string {:ok true :subject subject :facts (count facts)})))
