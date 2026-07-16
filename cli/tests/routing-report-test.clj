#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def tmp (.toFile (java.nio.file.Files/createTempDirectory "north-routing-report" (make-array java.nio.file.attribute.FileAttribute 0))))
(def coord (io/file tmp "coordination.log"))
(def telem (io/file tmp "telemetry.log"))

(defn fact [file l p r]
  (spit file (str (pr-str {:op "assert" :l l :p p :r r}) "\n") :append true))

(defn run! [kind]
  (let [result (proc/shell {:out :string :err :string :extra-env {"FRAM_LOG" (.getPath coord)
                                                                  "FRAM_TELEMETRY_LOG" (.getPath telem)}}
                           "bb" (str root "/cli/routing-report.clj") "report" kind "--json")]
    (when-not (zero? (:exit result)) (throw (ex-info (:err result) result)))
    (json/parse-string (str/trim (:out result)) true)))

(defn check [label ok]
  (println (str (if ok "ok:   " "FAIL: ") label))
  (when-not ok (System/exit 1)))

;; Two independently evidenced threads using the same bespoke composition.
(doseq [id ["@thread-a" "@thread-b"]]
  (fact coord id "title" (str "Thread " id))
  (fact coord id "outcome" "landed")
  (fact coord id "done_when" "tests pass")
  (fact coord id "bar_evidence" "tests pass → exit 0"))

(doseq [[run thread provider outcome tokens] [["@run-a" "thread-a" "openai" "ran" "100"]
                                               ["@run-b" "thread-b" "anthropic" "ran" "200"]
                                               ["@run-c" "(ad-hoc)" "openai" "died" "50"]]]
  (fact telem run "kind" "run")
  (fact telem run "thread" thread)
  (fact telem run "provider" provider)
  (fact telem run "requested_tier" "senior")
  (fact telem run "role" "migration-forensics")
  (fact telem run "task_grade" "staff")
  (fact telem run "outcome" outcome)
  (fact telem run "tokens" tokens)
  (fact telem run "duration_ms" "1000")
  (when (not= run "@run-c")
    (fact telem run "composition_kind" "bespoke")
    (fact telem run "composition_id" "migration-forensics")
    (fact telem run "nearest_preset" "investigator")
    (fact telem run "bespoke_reason" "needs provenance plus schema recovery")
    (fact telem run "promotion_candidate" "true")))

(let [performance (run! "performance")
      usage (run! "usage")
      promotions (run! "promotions")
      cohorts (:cohorts performance)
      candidate (first (:compositions promotions))]
  (check "performance separates operational runs from verified evidence"
         (and (= 3 (:runs performance)) (= 2 (reduce + (map :verifiedEvidence cohorts)))
              (= 3 (reduce + (map :runs cohorts)))))
  (check "performance carries an explicit non-causal quality disclaimer"
         (str/includes? (:claim performance) "not causal model quality"))
  (check "usage is subscription-safe and contains no dollar measure"
         (and (= "observed work, never dollars or API credits" (:unit usage))
              (= 350 (reduce + (map :tokens (:providers usage))))
              (nil? (:cost usage))))
  (check "bespoke recurrence is surfaced only as a human review candidate"
         (and (= "migration-forensics" (:compositionId candidate))
              (= 2 (:distinctThreads candidate)) (= 2 (:verifiedEvidence candidate))
              (= "review-candidate" (:reviewStatus candidate))
              (str/includes? (:note candidate) "never promotes"))))

(try
  (let [out (:out (proc/shell {:out :string :err :string :extra-env {"FRAM_LOG" (.getPath coord)
                                                                      "FRAM_TELEMETRY_LOG" (.getPath telem)}}
                               (str root "/bin/north") "routing" "report" "performance"))]
    (check "north routing report is wired through the public CLI" (str/includes? out "ROUTING PERFORMANCE")))
  (finally
    (doseq [file (reverse (file-seq tmp))] (io/delete-file file true))))
