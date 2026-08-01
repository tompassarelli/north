#!/usr/bin/env bb
(require '[babashka.fs :as fs]
         '[babashka.process :as process]
         '[clojure.string :as str])

(def root (str (fs/parent (fs/parent (fs/parent (fs/absolutize *file*))))))
(def report (str root "/bin/north-lane-report"))
(def scratch (str (fs/create-temp-dir {:prefix "north-lane-report-"})))
(def coordination (str scratch "/coordination.log"))
(def telemetry (str scratch "/telemetry.log"))
(def checks (atom 0))
(def failures (atom 0))

(defn check! [label pass?]
  (swap! checks inc)
  (if pass?
    (println "PASS" label)
    (do (swap! failures inc) (println "FAIL" label))))

(defn run-lines [ordinal subject facts]
  (map-indexed
   (fn [index [predicate object]]
     (pr-str {:tx (+ ordinal index) :op "assert" :l (str "@" subject)
              :p predicate :r object :ts "2026-07-27T00:00:00Z"}))
   facts))

(def runs
  [{:id "run:10000000-0000-4000-8000-000000000001" :start "2026-07-27T10:00:00Z"
    :arm "graph" :files "2" :lines "100" :dispatcher "@agent:d1"
    :role "implementer" :model "model-a" :wall "120000" :tokens "150"
    :outcome "landed" :retries "0"}
   {:id "run:10000000-0000-4000-8000-000000000002" :start "2026-07-28T10:00:00Z"
    :arm "graph" :files "2" :lines "100" :dispatcher "@agent:d1"
    :role "implementer" :model "model-a" :wall "180000" :tokens "300"
    :outcome "failed" :retries "1"}
   {:id "run:10000000-0000-4000-8000-000000000003" :start "2026-07-29T10:00:00Z"
    :arm "text" :files "10" :lines "300" :dispatcher "@agent:d2"
    :role "integrator" :model "model-b" :wall "240000" :tokens "280"
    :outcome "returned" :retries "0"}
   {:id "run:10000000-0000-4000-8000-000000000004" :start "2026-07-27T00:00:00Z"
    :arm "text" :files "1" :lines "20" :dispatcher "@agent:d3"
    :role "executor" :model "model-c"}
   {:id "run:10000000-0000-4000-8000-000000000005" :start "2026-07-30T10:00:00Z"
    :arm "text" :learning-arm "graph" :files "2" :lines "100" :dispatcher "@agent:d1"
    :role "implementer" :model "model-a" :wall "120000" :tokens "250"
    :outcome "landed" :retries "0"}])

(def estimates
  [{:id "run:estimate:20000000-0000-4000-8000-000000000011" :run 1 :by "@agent:d1"
    :tokens "100" :wall "1" :at "2026-07-27T10:00:00Z"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000012" :run 1 :by "@agent:w1"
    :tokens "600" :wall "10" :at "2026-07-27T10:01:00Z" :why "first intake"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000013" :run 1 :by "@agent:w1"
    :tokens "200" :wall "2" :at "2026-07-27T10:02:00Z" :why "intake revised"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000021" :run 2 :by "@agent:d1"
    :tokens "300" :wall "4" :at "2026-07-28T10:00:00Z"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000031" :run 3 :by "@agent:d2"
    :tokens "100" :wall "1" :at "2026-07-29T10:00:00Z"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000032" :run 3 :by "@agent:w2"
    :tokens "300" :wall "4" :at "2026-07-29T10:01:00Z"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000041" :run 4 :by "@agent:d3"
    :tokens "100" :wall "10" :at "2026-07-27T00:00:00Z"}
   {:id "run:estimate:20000000-0000-4000-8000-000000000051" :run 5 :by "@agent:d1"
    :tokens "100" :wall "1" :at "2026-07-30T10:00:00Z"}])

(def fixture-lines
  (concat
   (mapcat
    (fn [index {:keys [id start arm learning-arm files lines dispatcher role model wall tokens outcome retries]}]
      (run-lines
       (* 30 (inc index)) id
       (cond-> [["run_start" start] ["run_arm" arm] ["run_size_files" files]
                ["run_size_lines" lines] ["run_dispatcher" dispatcher]
                ["run_role" role] ["run_model" model]]
         learning-arm (conj ["learning_axis" "authoring"]
                            ["learning_arm_id" learning-arm])
         wall (conj ["run_end" start] ["run_wall_ms" wall] ["run_outcome" outcome]
                    ["run_retries" retries] ["run_token_status" "exact"]
                    ["run_tokens_in" tokens] ["run_tokens_out" "0"])
         true (conj ["kind" "run"]))))
    (range) runs)
   (mapcat
    (fn [index {:keys [id run by tokens wall at why]}]
      (run-lines
       (+ 300 (* 10 index)) id
       (cond-> [["estimate_of" (str "@" (:id (nth runs (dec run))))]
                ["estimate_by" by] ["estimate_tokens" tokens]
                ["estimate_wall_min" wall] ["estimate_at" at]]
         why (conj ["estimate_why" why])
         true (conj ["kind" "estimate"]))))
    (range) estimates)))

(spit coordination "")
(spit telemetry (str (str/join "\n" fixture-lines) "\n"))

(let [result
      (process/shell
       {:out :string :err :string :continue true
        :extra-env {"FRAM_LOG" coordination "NORTH_TELEMETRY_LOG" telemetry}}
       report "--attention-factor" "1.5")
      output (:out result)]
  (check! "report exits zero" (zero? (:exit result)))
  (let [open-at (.indexOf output "run:10000000-0000-4000-8000-000000000004")
        finished-at (.indexOf output "run:10000000-0000-4000-8000-000000000005")]
    (check! "attention list is first and sorts worst first"
            (and (str/starts-with? output "ATTENTION LIST\n")
                 (pos? open-at) (> finished-at open-at)
                 (re-find #"run:10000000-0000-4000-8000-000000000005\s+finished\s+graph\s+agent:d1\s+2\.00x\s+2\.50x"
                          output))))
  (check! "calibration trend has exact arm and size errors"
          (and (re-find #"(?m)^2026-W31\s+graph\s+small\s+3\s+66\.7%\s+3\s+75\.0%$" output)
               (re-find #"(?m)^2026-W31\s+text\s+medium\s+1\s+180\.0%\s+1\s+300\.0%$" output)))
  (check! "per-estimator calibration uses latest revision"
          (and (re-find #"(?m)^agent:w1\s+1\s+25\.0%\s+1\s+0\.0%\s+12\.5%" output)
               (re-find #"(?m)^agent:d1\s+3\s+66\.7%\s+3\s+75\.0%\s+70\.8%" output)))
  (check! "divergence analytics names who was righter"
          (boolean (re-find #"(?m)^agent:w2\s+1\s+1\s+0\s+0$" output)))
  (check! "round-one per-arm aggregate remains present"
          (boolean (re-find #"(?m)^graph\s+3\s+120000\s+250 \(3/3 exact\)\s+66\.7%\s+66\.7%$"
                            output)))
  (check! "unified authoring assignment wins over compatibility-only run_arm"
          (and (re-find #"run:10000000-0000-4000-8000-000000000005\s+finished\s+graph" output)
               (not (re-find #"run:10000000-0000-4000-8000-000000000005\s+finished\s+text" output))))
  (when (pos? @failures)
    (println "--- report output ---")
    (println output)
    (println "--- report stderr ---")
    (println (:err result))))

(fs/delete-tree scratch)
(println (format "lane-report: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
