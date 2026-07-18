#!/usr/bin/env bb
;; Harness-owned run telemetry publication. A fresh @run subject is invisible to
;; every run consumer until `kind=run` lands LAST. All preceding fact writes are
;; acknowledged durable coordinator operations in one writer process; a crash or
;; rejection leaves an undiscoverable partial subject instead of a false run row.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
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

(defn validate-reported-run! [port subject scalar delivery-facts]
  (when (= "reported" (get delivery-facts "delivery_outcome"))
    (let [run-facts (facts-of port subject)
          evidence (json/parse-string (get delivery-facts "delivery_evidence"))
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
      before (facts-of port subject)
      reservation-keys
      (conj (set north.terminal-projection/run-reservation-predicates)
            "run_bar_evidence")
      unknown-before (seq (remove reservation-keys (keys before)))
      reserved? (north.terminal-projection/run-reservation-valid? before)]
  (when-not (= [["kind" "run"]] kind-facts)
    (fail! "run telemetry requires exactly kind=run" {:kind-facts kind-facts}))
  (when unknown-before
    (fail! "run subject reuse or partial prior publication is forbidden"
           {:subject subject :predicates unknown-before}))
  (when (and (seq before) (not reserved?))
    (fail! "run subject has a conflicting or incomplete reservation"
           {:subject subject}))
  (when (contains? before "kind")
    (fail! "run subject is already committed" {:subject subject}))
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
    (validate-reported-run! port subject scalar delivery-facts))
  (doseq [[predicate value] body-facts]
    (checked! (north.coord/put! port subject predicate value)
              [:put subject predicate value]))
  ;; `kind` is the publication/commit marker. Capture the coordinator version
  ;; before all load-bearing reads; if any graph write races those reads, the
  ;; marker rejects and the whole validation callback runs again.
  (checked!
   (north.coord/assert-after-read!
    port subject "kind" "run"
    (fn []
      (let [stored (facts-of port subject)]
        (when (contains? stored "kind")
          (fail! "run subject became committed during publication"
                 {:subject subject}))
        (doseq [[predicate entries] grouped
                :let [expected (set (map second entries))
                      actual (get stored predicate #{})]]
          (when-not (= expected actual)
            (fail! "run telemetry readback conflicts with the submitted projection"
                   {:subject subject :predicate predicate
                    :expected expected :actual actual}))))
      (validate-reported-run! port subject scalar delivery-facts)))
   [:assert-after-read subject "kind" "run"])
  (when-not (= #{"run"} (get (facts-of port subject) "kind"))
    (fail! "run commit marker lost singleton race" {:subject subject}))
  (println (json/generate-string {:ok true :subject subject :facts (count facts)})))
