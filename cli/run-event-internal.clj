#!/usr/bin/env bb
;; Append-only AgentRun event publication. Body facts are acknowledged first;
;; kind=run_event is the last commit marker, matching run header publication.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def cli-dir (.getParent (io/file (System/getProperty "babashka.file"))))
(load-file (str cli-dir "/coord.clj"))
(load-file (str cli-dir "/run-ledger.clj"))

(defn fail! [message data] (throw (ex-info message data)))
(defn checked! [result operation]
  (when (:reject result) (fail! "coordinator rejected run event publication" {:operation operation}))
  result)

(defn payload [raw]
  (let [parsed (try (json/parse-string (str raw))
                    (catch Exception error
                      (fail! "invalid run event JSON" {:cause (.getMessage error)})))]
    (when-not (sequential? parsed) (fail! "run event payload must be an array" {}))
    (mapv (fn [entry]
            (when-not (and (sequential? entry) (= 2 (count entry))
                           (every? string? entry) (every? #(not (str/blank? %)) entry))
              (fail! "run event facts must be nonblank string pairs" {:entry entry}))
            (vec entry))
          parsed)))

(defn facts-of [port subject]
  (let [rows (:ok (north.coord/send-op
                   port {:op :query
                         :query {:find "run_event_writer_fact"
                                 :rules [{:head {:rel "run_event_writer_fact"
                                                 :args [{:var "p"} {:var "r"}]}
                                          :body [{:rel "triple"
                                                  :args [subject {:var "p"} {:var "r"}]}]}]}}))]
    (reduce (fn [acc [predicate value]] (update acc predicate (fnil conj #{}) value)) {} rows)))

(let [[port-s subject-s raw] *command-line-args*
      port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
      subject (north.run-ledger/canonical-entity subject-s "event subject")
      facts (payload raw)
      kind-facts (filterv #(= "kind" (first %)) facts)
      body-facts (filterv #(not= "kind" (first %)) facts)
      event (north.run-ledger/validate-event-facts! subject facts)
      sequence (get event "sequence")
      previous-subject (when (pos? sequence)
                         (str (subs subject 0 (- (count subject) 8))
                              (format "%08d" (dec sequence))))
      previous (when previous-subject (facts-of port previous-subject))]
  (when-not (= [["kind" "run_event"]] kind-facts)
    (fail! "run event requires exactly kind=run_event" {:kind-facts kind-facts}))
  (when (seq (facts-of port subject))
    (fail! "run event subject reuse or partial prior publication is forbidden" {:subject subject}))
  (when (and previous-subject (not= #{"run_event"} (get previous "kind")))
    (fail! "run event publication requires its committed predecessor"
           {:subject subject :previous previous-subject}))
  (when (= #{"terminal_cleanup"} (get previous "run_event_type"))
    (fail! "run event publication cannot append after terminal_cleanup"
           {:subject subject :previous previous-subject}))
  (doseq [[predicate value] body-facts]
    (checked! (north.coord/put! port subject predicate value) [:put subject predicate value]))
  (checked!
   (north.coord/assert-after-read!
    port subject "kind" "run_event"
    (fn []
      (let [stored (facts-of port subject)]
        (when (contains? stored "kind") (fail! "run event became committed during publication" {}))
        (doseq [[predicate value] body-facts]
          (when-not (= #{value} (get stored predicate))
            (fail! "run event readback conflicts with submitted projection"
                   {:predicate predicate})))
        (north.run-ledger/validate-event-facts! subject facts))))
   [:assert-after-read subject "kind" "run_event"])
  (when-not (= #{"run_event"} (get (facts-of port subject) "kind"))
    (fail! "run event commit marker lost singleton race" {:subject subject}))
  (println (json/generate-string {:ok true :subject subject
                                  :sequence (get event "sequence")})))
