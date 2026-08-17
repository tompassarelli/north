#!/usr/bin/env bb
;; Retry-idempotent exact WireEvent publication. Every fact projection retains
;; the canonical event bytes; Beagle Store serializes each append and rejects forks.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[clojure.string :as str])

(def cli-dir (.getParent (io/file (or *file* (System/getProperty "babashka.file")))))
(load-file (str cli-dir "/coord.clj"))
(load-file (str cli-dir "/run-ledger.clj"))
(def wire-validator
  (.getCanonicalPath (io/file cli-dir "../sdk/src/wire-ledger-validator.ts")))

(defn fail! [message data] (throw (ex-info message data)))
(defn checked! [result operation]
  (when (:reject result) (fail! "coordinator rejected wire event publication" {:operation operation}))
  result)

(defn bounded-stdin []
  (let [buffer (byte-array 8192)
        output (java.io.ByteArrayOutputStream.)]
    (loop [total 0]
      (let [read (.read System/in buffer)]
        (if (neg? read)
          (.toString output "UTF-8")
          (let [next-total (+ total read)]
            (when (> next-total north.run-ledger/max-projection-batch-bytes)
              (fail! "wire event batch exceeds its encoded byte bound"
                     {:limit north.run-ledger/max-projection-batch-bytes}))
            (.write output buffer 0 read)
            (recur next-total)))))))

(defn fact-payload [parsed]
  (when-not (sequential? parsed) (fail! "wire event facts must be an array" {}))
  (mapv (fn [entry]
          (when-not (and (sequential? entry) (= 2 (count entry))
                         (every? string? entry) (every? #(not (str/blank? %)) entry))
            (fail! "wire event facts must be nonblank string pairs" {:entry entry}))
          (vec entry))
        parsed))

(defn event-entry [subject-s facts]
  (let [subject (north.run-ledger/canonical-entity subject-s "event subject")
        event (north.run-ledger/validate-event-facts! subject facts)]
    {:subject subject :facts facts :event event}))

(defn validate-batch-semantics! [entries]
  (let [wire-jsonl (str (str/join "\n" (map #(get-in % [:event "json"]) entries)) "\n")
        bun (or (System/getenv "NORTH_BUN") "bun")
        {:keys [exit]} (shell/sh bun wire-validator :in wire-jsonl)]
    (when-not (zero? exit)
      (fail! "wire event batch violates the canonical reducer contract" {}))))

(defn batch-payload [raw]
  (let [parsed (try (json/parse-string (str raw))
                    (catch Exception error
                      (fail! "invalid wire event batch JSON" {:cause (.getMessage error)})))]
    (when-not (and (sequential? parsed) (seq parsed)
                   (<= (count parsed) north.run-ledger/max-batch-events))
      (fail! "wire event batch must be nonempty and bounded" {}))
    (let [entries
          (mapv
           (fn [entry]
             (when-not (and (map? entry) (= #{"subject" "facts"} (set (keys entry))))
               (fail! "wire event batch entries require only subject and facts" {:entry entry}))
             (event-entry (get entry "subject") (fact-payload (get entry "facts"))))
           parsed)
          events (mapv :event entries)
          sequences (mapv #(get % "sequence") events)
          lineage-keys ["run" "thread" "agent" "parentThread" "coordinator"]]
      (when-not (= sequences (vec (range (count entries))))
        (fail! "wire event batch requires a complete zero-based sequence" {:sequences sequences}))
      (when-not (= 1 (count (set (map #(select-keys % lineage-keys) events))))
        (fail! "wire event batch lineage must remain constant" {}))
      (when-not (= "run.started" (get (first events) "kind"))
        (fail! "wire event sequence zero must be run.started" {}))
      (when-not (= "run.terminated" (get (last events) "kind"))
        (fail! "wire event batch requires run.terminated last" {}))
      (when (some #(= "run.terminated" (get % "kind")) (butlast events))
        (fail! "wire event batch cannot continue after run.terminated" {}))
      (validate-batch-semantics! entries)
      entries)))

(defn facts-of [port subject]
  (let [rows (north.coord/query-rows
              port {:find "wire_event_writer_fact"
                    :rules [{:head {:rel "wire_event_writer_fact"
                                    :args [{:var "p"} {:var "r"}]}
                             :body [{:rel "triple"
                                     :args [subject {:var "p"} {:var "r"}]}]}]})]
    (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))

(defn fact-map [facts]
  (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} facts))

(defn stored-event [port subject]
  (let [stored (facts-of port subject)]
    (when (seq stored)
      (north.run-ledger/validate-event-facts!
       subject
       (mapv (fn [[predicate values]]
               (when-not (= 1 (count values))
                 (fail! "stored wire event predicate is not singleton"
                        {:subject subject :predicate predicate}))
               [predicate (first values)])
             stored)))))

(defn previous-subject [event]
  (when (pos? (get event "sequence"))
    (north.run-ledger/event-subject (get event "run") (dec (get event "sequence")))))

(defn preflight! [port entries]
  (doseq [{:keys [subject facts]} entries
          :let [stored (facts-of port subject)]
          :when (and (seq stored) (not= stored (fact-map facts)))]
    (fail! "wire event subject conflicts with an existing projection" {:subject subject}))
  (let [first-entry (first entries)
        first-event (:event first-entry)
        predecessor-subject (previous-subject first-event)]
    (when predecessor-subject
      (let [predecessor (stored-event port predecessor-subject)]
        (when-not predecessor
          (fail! "wire event batch requires its committed predecessor"
                 {:subject (:subject first-entry) :previous predecessor-subject}))
        (when-not (= (get predecessor "run") (get first-event "run"))
          (fail! "wire event predecessor belongs to another run" {}))
        (when (= "run.terminated" (get predecessor "kind"))
          (fail! "wire event publication cannot append after run.terminated" {}))))))

(defn publish-event! [port {:keys [subject facts event]}]
  (let [expected (fact-map facts)
        predecessor-subject (previous-subject event)]
    (checked!
     (north.coord/assert-batch-after-read!
      port subject
      (fn []
        (let [stored (facts-of port subject)]
          (cond
            (= stored expected) {:done true}
            (seq stored)
            (fail! "wire event subject conflicts with an existing projection" {:subject subject})
            :else
            (do
              (when predecessor-subject
                (let [predecessor (stored-event port predecessor-subject)]
                  (when-not predecessor
                    (fail! "wire event publication requires its committed predecessor"
                           {:subject subject :previous predecessor-subject}))
                  (when (= "run.terminated" (get predecessor "kind"))
                    (fail! "wire event publication cannot append after run.terminated"
                           {:subject subject :previous predecessor-subject}))))
              (north.run-ledger/validate-event-facts! subject facts)
              {:facts (mapv (fn [[predicate value]] {:p predicate :r value}) facts)})))))
     [:assert-batch-after-read subject])
    (when-not (= expected (facts-of port subject))
      (fail! "wire event readback conflicts with submitted projection" {:subject subject}))
    {:subject subject :sequence (get event "sequence")}))

(defn publish-events! [port entries]
  ;; Preflight protects a clean batch from deterministic partial mutation.
  ;; If a process dies during publication, exact retry resumes the remaining
  ;; suffix because already-committed subjects compare byte-for-byte equal.
  (preflight! port entries)
  (mapv #(publish-event! port %) entries))

(defn -main [& args]
  (let [[port-s] args
        _ (when-not (= 1 (count args))
            (fail! "usage: run-event-internal.clj PORT < BATCH_JSON" {:argc (count args)}))
        port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
        entries (batch-payload (bounded-stdin))
        published (publish-events! port entries)]
    (println (json/generate-string
              {:ok true
               :count (count published)
               :firstSequence (:sequence (first published))
               :lastSequence (:sequence (last published))}))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
