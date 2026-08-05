#!/usr/bin/env bb
;; Harness-owned pre-provider learning assignment publication. The complete
;; projection lands in one coordinator transaction and exact replay is the only
;; accepted reuse. No provider boundary may run if this writer fails.
(require '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/terminal-projection.clj"))

(def assignment-predicates
  ["learning_assignment_version"
   "learning_policy_version"
   "learning_policy_sha256"
   "learning_mode"
   "learning_evidence_mode"
   "learning_experiment_id"
   "learning_episode_id"
   "learning_task_signature_sha256"
   "learning_task_signature_coverage"
   "learning_risk"
   "learning_arm"
   "learning_axis"
   "learning_arm_id"
   "learning_propensity"
   "learning_explore_propensity"
   "learning_narrowing_reason"
   "learning_baseline_sha256"
   "learning_options_sha256"
   "learning_assignment_sha256"])

;; Must outlive one bounded fleet terminal-write convoy (global-version CAS).
(def assignment-publication-deadline-ms 60000)

(def assignment-predicate-set (set assignment-predicates))
(def sha256? #(boolean (re-matches #"[a-f0-9]{64}" (or % ""))))
(def identifier? #(boolean (re-matches #"[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}" (or % ""))))

(defn fail! [message data] (throw (ex-info message data)))

(defn checked! [result operation]
  (when (:reject result)
    (fail! "coordinator rejected learning assignment publication" {:operation operation}))
  result)

(defn entity [subject]
  (let [raw (str subject)
        canonical (if (str/starts-with? raw "@") raw (str "@" raw))]
    (when-not (north.terminal-projection/valid-run-entity? canonical)
      (fail! "invalid learning assignment run subject" {:subject subject}))
    canonical))

(defn payload [raw]
  (let [parsed (try (json/parse-string (str raw))
                    (catch Exception error
                      (fail! "invalid learning assignment JSON"
                             {:cause (.getMessage error)})))]
    (when-not (sequential? parsed)
      (fail! "learning assignment payload must be an array" {}))
    (mapv (fn [entry]
            (when-not (and (sequential? entry) (= 2 (count entry))
                           (every? string? entry)
                           (every? #(not (str/blank? %)) entry))
              (fail! "learning assignment facts must be nonblank string pairs"
                     {:entry entry}))
            (vec entry))
          parsed)))

(defn facts-of [port subject]
  (let [rows (north.coord/query-rows
              port {:find "learning_assignment_fact"
                    :rules [{:head {:rel "learning_assignment_fact"
                                    :args [{:var "p"} {:var "r"}]}
                             :body [{:rel "triple"
                                     :args [subject {:var "p"} {:var "r"}]}]}]})]
    (reduce (fn [acc [predicate value]]
              (update acc predicate (fnil conj #{}) value))
            {}
            rows)))

(defn validate-projection! [facts]
  (let [grouped (group-by first facts)
        predicates (set (keys grouped))
        scalar (into {} (map (fn [[predicate entries]]
                               [predicate (second (first entries))])) grouped)]
    (when-not (= assignment-predicate-set predicates)
      (fail! "learning assignment requires the exact v1 predicate set"
             {:missing (vec (remove predicates assignment-predicates))
              :unknown (vec (remove assignment-predicate-set predicates))}))
    (doseq [[predicate entries] grouped]
      (when-not (= 1 (count entries))
        (fail! "learning assignment predicates must be singleton"
               {:predicate predicate :values (mapv second entries)})))
    (doseq [predicate ["learning_policy_sha256" "learning_task_signature_sha256"
                       "learning_baseline_sha256" "learning_options_sha256"
                       "learning_assignment_sha256"]]
      (when-not (sha256? (get scalar predicate))
        (fail! "learning assignment contains an invalid digest" {:predicate predicate})))
    (doseq [predicate ["learning_assignment_version" "learning_policy_version"
                       "learning_experiment_id" "learning_episode_id"
                       "learning_arm_id" "learning_narrowing_reason"]]
      (when-not (identifier? (get scalar predicate))
        (fail! "learning assignment contains an invalid identifier" {:predicate predicate})))
    (when-not (= "north-learning-assignment:v1"
                 (get scalar "learning_assignment_version"))
      (fail! "learning assignment has unsupported assignment version" {}))
    (when-not (= "north-learning-policy:v1" (get scalar "learning_policy_version"))
      (fail! "learning assignment has unsupported policy version" {}))
    (when-not (#{"frozen" "learning"} (get scalar "learning_mode"))
      (fail! "learning assignment has invalid mode" {}))
    (when-not (#{"discovery" "evaluation"}
                (get scalar "learning_evidence_mode"))
      (fail! "learning assignment has invalid evidence mode" {}))
    (when-not (#{"control" "explore"} (get scalar "learning_arm"))
      (fail! "learning assignment has invalid arm" {}))
    (when-not (#{"control" "model-tier" "effort" "prompt" "authoring" "history"}
                (get scalar "learning_axis"))
      (fail! "learning assignment has invalid axis" {}))
    (when-not (#{"p0" "p1" "p2" "p3" "unknown"} (get scalar "learning_risk"))
      (fail! "learning assignment has invalid risk" {}))
    (let [arm (get scalar "learning_arm")
          axis (get scalar "learning_axis")
          arm-id (get scalar "learning_arm_id")]
      (when-not (or (and (= "control" arm) (= "control" axis) (= "control" arm-id))
                    (and (= "explore" arm) (not= "control" axis)
                         (not= "control" arm-id)))
        (fail! "learning assignment arm, axis, and arm id are inconsistent" {})))
    (when-not (#{"exact" "partial" "unknown"}
                (get scalar "learning_task_signature_coverage"))
      (fail! "learning assignment has invalid task signature coverage" {}))
    (doseq [predicate ["learning_propensity" "learning_explore_propensity"]
            :let [value (try (Double/parseDouble (get scalar predicate))
                             (catch Exception _ ##NaN))]]
      (when-not (and (Double/isFinite value) (<= 0.0 value 1.0))
        (fail! "learning assignment has invalid propensity" {:predicate predicate})))
    scalar))

(let [[port-s subject-s raw] *command-line-args*
      port (Integer/parseInt (or port-s (or (System/getenv "NORTH_PORT") "7977")))
      subject (entity subject-s)
      facts (payload raw)
      _ (validate-projection! facts)
      expected (into {} (map (fn [[predicate value]] [predicate #{value}])) facts)
      outcome
      (north.coord/assert-batch-after-read!
       port subject
       (fn []
         (let [stored (facts-of port subject)
               current (select-keys stored assignment-predicate-set)]
           (cond
             (contains? stored "kind")
             (fail! "learning assignment cannot be added after run commit"
                    {:subject subject})

             (= expected current)
             {:done :idempotent-replay}

             (seq current)
             (fail! "learning assignment identity is immutable"
                    {:subject subject :expected expected :actual current})

             :else
             {:facts (mapv (fn [[predicate value]] {:p predicate :r value}) facts)})))
       Integer/MAX_VALUE
       (north.coord/retry-deadline-ns assignment-publication-deadline-ms))]
  (checked! outcome [:assert-batch-at-version subject])
  (when-not (= expected (select-keys (facts-of port subject) assignment-predicate-set))
    (fail! "learning assignment readback differs from submitted projection"
           {:subject subject}))
  (println (json/generate-string {:ok true :subject subject
                                  :replay (= :idempotent-replay (:done outcome))})))
