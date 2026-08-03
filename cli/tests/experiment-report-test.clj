#!/usr/bin/env bb
(require '[babashka.process :as process]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def scratch (.toFile (java.nio.file.Files/createTempDirectory
                       "north-experiment-report-"
                       (make-array java.nio.file.attribute.FileAttribute 0))))
(def coord (io/file scratch "coordination.log"))
(def telem (io/file scratch "telemetry.log"))
(def assignment-digest (apply str (repeat 64 "a")))
(def checks (atom []))

(defn check! [label pass?] (swap! checks conj [label (boolean pass?)]))
(defn fact! [file subject predicate object]
  (spit file (str (pr-str {:op "assert" :l subject :p predicate :r object}) "\n") :append true))

(defn run! [thread index arm operation duration outcome]
  (let [run (format "@run:%08d-0000-4000-8000-%012d" index index)]
    (doseq [[predicate value]
            (concat
             [["kind" "run"] ["agent" (str "lane-" index)] ["thread" thread]
             ["at" (format "2026-08-03T00:%02d:00Z" (mod index 60))]
             ["outcome" outcome] ["process_outcome" outcome]
             ["duration_ms" (str duration)]
             ["graph_text_experiment_version" "north-graph-text-assignment:v1"]
             ["graph_text_experiment_status" "assigned"]
             ["graph_text_experiment_arm" arm]
             ["graph_text_experiment_applied" "true"]
             ["graph_text_experiment_reason" "deterministic-balanced-assignment"]
             ["graph_text_experiment_assignment_sha256" assignment-digest]]
             (if (= arm "graph")
               [["mcp_activity_coverage" "exact"]
                ["mcp_operation_receipt"
                 (json/generate-string {:tool "fram/show" :operation operation
                                        :durationMs 10 :outcome (if (= outcome "ran") "ok" "typed_failure")
                                        :resultSize 1})]
                ["mcp_operation_aggregate"
                 (json/generate-string {:operation operation :count 1
                                        :totalDurationMs 10 :meanDurationMs 10.0
                                        :failureCount (if (= outcome "ran") 0 1)})]]
               [["native_command_activity_coverage" "exact"]
                ["native_command_completion"
                 (json/generate-string {:commandSha256 assignment-digest
                                        :outputSha256 assignment-digest
                                        :status (if (= outcome "ran") "completed" "failed")
                                        :exitCode (if (= outcome "ran") 0 1)
                                        :shape (if (str/starts-with? operation "reasoning.") "read" "edit")
                                        :durationMs 10})]]))]
      (fact! telem run predicate value))))

(try
  (doseq [pair-index (range 20)
          :let [thread (str "task-reasoning-" pair-index)]]
    (run! thread (+ 1 (* pair-index 2)) "graph" "reasoning.inspect" 100 "ran")
    (run! thread (+ 2 (* pair-index 2)) "text" "reasoning.inspect" 200
          (if (< pair-index 5) "provider_error" "ran")))
  (doseq [pair-index (range 2)
          :let [thread (str "task-authoring-" pair-index)
                base (+ 100 (* pair-index 2))]]
    (run! thread base "graph" "authoring.edit" 80 "ran")
    (run! thread (inc base) "text" "authoring.edit" 90 "provider_error"))
  (run! "task-authoring-unpaired" 200 "graph" "authoring.edit" 70 "ran")

  (let [result (process/shell
                {:out :string :err :string :continue true
                 :extra-env {"FRAM_LOG" (.getPath coord)
                             "FRAM_TELEMETRY_LOG" (.getPath telem)}}
                (str root "/bin/north") "experiment" "report" "--json")
        data (when (zero? (:exit result))
               (json/parse-string (str/trim (:out result)) true))
        operations (into {} (map (juxt :operationType identity) (:operations data)))]
    (check! "north experiment report exits zero" (zero? (:exit result)))
    (check! "twenty paired wins mechanically recommend graph"
            (= {:pairs 20 :verdict "flip-to-graph"}
               (select-keys (get operations "reasoning") [:pairs :verdict])))
    (check! "below-threshold evidence is explicitly insufficient"
            (= {:pairs 2 :sampleCounts {:graph 3 :text 2}
                :verdict "insufficient data"}
               (select-keys (get operations "authoring")
                            [:pairs :sampleCounts :verdict])))
    (let [row (get operations "reasoning")]
      (check! "paired medians and failure rates remain arm-specific"
              (and (== 100.0 (get-in row [:graph :medianWallMs]))
                   (== 200.0 (get-in row [:text :medianWallMs]))
                   (= 15 (:pairedGreenWallSamples row))
                   (= 0.0 (get-in row [:graph :failureRate]))
                   (= 0.25 (get-in row [:text :failureRate])))))
    (check! "historical priors are marked non-transferable"
            (every? #(= "non-transferable" (:transferability %)) (:priors data))))

  (finally
    (doseq [file (reverse (file-seq scratch))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label pass?] results]
    (println (format "  [%s] %s" (if pass? "PASS" "FAIL") label)))
  (println (format "\nexperiment report: %d / %d PASS" passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
