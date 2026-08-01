#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/routing-report.clj"))

(defn fact [file l p r]
  (spit file (str (pr-str {:op "assert" :l l :p p :r r}) "\n") :append true))

(defn check [label ok]
  (println (str (if ok "ok:   " "FAIL: ") label))
  (when-not ok (System/exit 1)))

(defn waste-cli [coord-log telemetry-log & flags]
  (let [result (apply proc/shell
                      {:out :string :err :string
                       :extra-env {"FRAM_LOG" (.getPath coord-log)
                                   "FRAM_TELEMETRY_LOG" (.getPath telemetry-log)}}
                      (into ["bb" (str root "/cli/routing-report.clj")
                             "report" "waste"] flags))]
    (when-not (zero? (:exit result)) (throw (ex-info (:err result) result)))
    result))

(defn waste-run!
  [coord-log telemetry-log suffix at process delivery reason tokens
   & {:keys [preflight-cause retry-of-run]}]
  (let [run (str "@run:waste-" suffix)
        agent (str "waste-" suffix)
        lane (str "@agent:" agent)]
    (fact coord-log lane "kind" "lane")
    (fact coord-log lane "spawned_at" at)
    (doseq [[predicate value]
            [["agent" agent] ["thread" "@thread-waste"]
             ["at" at] ["outcome" process] ["process_outcome" process]
             ["delivery_outcome" delivery] ["delivery_reason" reason]]]
      (fact telemetry-log run predicate value))
    (when tokens (fact telemetry-log run "tokens" (str tokens)))
    (when preflight-cause (fact telemetry-log run "preflight_cause" preflight-cause))
    (when retry-of-run
      (fact telemetry-log run "retry_of_run" retry-of-run)
      (fact telemetry-log run "retry_attempt" "1"))
    (fact telemetry-log run "kind" "run")
    run))

(defn died-unreported-lane! [coord-log suffix at]
  (let [lane (str "@agent:waste-" suffix)
        terminal {"outcome" "died-unreported"
                  "process_outcome" "died-unreported"
                  "delivery_outcome" "blocked"
                  "delivery_reason" "presence_lapsed_without_committed_terminal"}]
    (fact coord-log lane "kind" "lane")
    (fact coord-log lane "spawned_at" at)
    (doseq [[predicate value] terminal]
      (fact coord-log lane predicate value))
    (fact coord-log lane "terminal_manifest_sha256"
          (north.terminal-projection/terminal-manifest-sha256 terminal))))

;; Every operator-defined machinery bucket is a synthetic terminal. The two
;; unknown-token rows are deliberately different: one died-unreported, while
;; the other completed normally. Both must become positive gating waste.
(let [waste-tmp (.toFile (java.nio.file.Files/createTempDirectory
                          "north-routing-waste"
                          (make-array java.nio.file.attribute.FileAttribute 0)))
      waste-coord (io/file waste-tmp "coordination.log")
      waste-telem (io/file waste-tmp "telemetry.log")]
  (try
    (fact waste-coord "@thread-waste" "title" "Waste report fixture")
    (let [provider-death
          (waste-run! waste-coord waste-telem "provider-death"
                      "2026-08-01T00:00:04Z" "died" "blocked"
                      "provider_process_died" 10000)]
      (waste-run! waste-coord waste-telem "blocked-preflight"
                  "2026-08-01T00:00:01Z" "blocked_preflight" "blocked"
                  "execution_preflight_blocked" 10000)
      (waste-run! waste-coord waste-telem "reservation-transport"
                  "2026-08-01T00:00:02Z" "ran" "unverified"
                  "delivery_reservation_load_failed_at_finalize" 10000)
      (waste-run! waste-coord waste-telem "startup-ack"
                  "2026-08-01T00:00:03Z" "blocked_preflight" "blocked"
                  "execution_preflight_blocked" 10000
                  :preflight-cause "startup acknowledgement timed out after 45000ms")
      (waste-run! waste-coord waste-telem "retry-duplicate"
                  "2026-08-01T00:00:05Z" "ran" "unverified"
                  "provider_terminal_success_without_external_verification" 10000
                  :retry-of-run provider-death)
      (died-unreported-lane! waste-coord "died-unreported"
                             "2026-08-01T00:00:06Z")
      (waste-run! waste-coord waste-telem "unknown-success"
                  "2026-08-01T00:00:07Z" "ran" "unverified"
                  "provider_terminal_success_without_external_verification" nil)
      ;; Explicit non-waste classes.
      (waste-run! waste-coord waste-telem "task-quality"
                  "2026-08-01T00:00:08Z" "ran" "unverified"
                  "task_quality_rejected_after_delivery" 10000)
      (let [checkpoint-run
            (waste-run! waste-coord waste-telem "checkpoint"
                        "2026-08-01T00:00:09Z" "blocked_preflight" "blocked"
                        "execution_preflight_blocked" 10000
                        :preflight-cause "coordinator transport failure")]
        (fact waste-coord "@thread-waste" "scope_escalation"
              (json/generate-string
               {:schema "north.scope-escalation/v1"
                :kind "scope-overrun"
                :disposition "needs-replan"
                :run checkpoint-run})))
      (doseq [index (range 10 21)]
        (waste-run! waste-coord waste-telem (str "success-" index)
                    (format "2026-08-01T00:00:%02dZ" index) "ran" "unverified"
                    "provider_terminal_success_without_external_verification" 10000)))
    (let [json-result (waste-cli waste-coord waste-telem "--json")
          report (json/parse-string (str/trim (:out json-result)) true)
          buckets (into {} (map (juxt :bucket identity) (:buckets report)))
          human (:out (waste-cli waste-coord waste-telem))]
      (check "waste report covers every machinery terminal bucket exactly once"
             (and (= (set waste-bucket-order) (set (keys buckets)))
                  (every? #(= 1 (:runs (get buckets %))) waste-bucket-order)
                  (every? #(= 10000 (get-in buckets [% :exactTokens]))
                          ["blocked-preflight" "reservation-transport"
                           "startup-ack-timeout" "zero-delivery-provider-death"
                           "retry-duplicate"])
                  (= {:unknownTokenRuns 1 :exactTokens 0
                      :gatingWasteTokens 10000.0}
                     (select-keys (get buckets "died-unreported")
                                  [:unknownTokenRuns :exactTokens
                                   :gatingWasteTokens]))))
      (check "task-quality failure and checkpointed escalation tokens are not waste"
             (and (= 20 (:runCount report))
                  (= 7 (:wasteRuns report))
                  (= 70000.0 (:machineryWastedTokens report))))
      (check "unknown token evidence is positive gating waste at exactly 90% coverage"
             (and (= {:runs 2 :gatingWasteTokens 20000.0
                      :exactRuns 18 :totalRuns 20 :exactCoveragePercent 90.0
                      :requiredExactCoveragePercent 90.0}
                     (:unknownCoverage report))
                  (= 200000.0 (:windowTokenTotal report))
                  (= 35.0 (:wasteRatioPercent report))
                  (= "FAIL" (:verdict report))))
      (check "human waste report prints ratio, window, buckets, coverage, and verdict"
             (every? #(str/includes? human %)
                     ["ROUTING WASTE" "WINDOW tokens=200000" "WASTE ratio=35.00%"
                      "blocked-preflight" "retry-duplicate" "unknown-coverage"
                      "exact-coverage=90.00%" "VERDICT FAIL"])))
    (finally
      (doseq [file (reverse (file-seq waste-tmp))] (io/delete-file file true)))))

;; The oldest, very large machinery failure sits just outside the fixed window.
;; Nineteen newer 50k runs plus the whole 60k boundary run total 1.01M tokens.
(let [boundary-tmp (.toFile (java.nio.file.Files/createTempDirectory
                             "north-routing-waste-boundary"
                             (make-array java.nio.file.attribute.FileAttribute 0)))
      boundary-coord (io/file boundary-tmp "coordination.log")
      boundary-telem (io/file boundary-tmp "telemetry.log")]
  (try
    (fact boundary-coord "@thread-waste" "title" "Waste boundary fixture")
    (waste-run! boundary-coord boundary-telem "outside-window"
                "2026-08-01T00:00:00Z" "blocked_preflight" "blocked"
                "execution_preflight_blocked" 900000)
    (waste-run! boundary-coord boundary-telem "boundary-run"
                "2026-08-01T00:00:01Z" "ran" "unverified"
                "provider_terminal_success_without_external_verification" 60000)
    (doseq [index (range 2 21)]
      (waste-run! boundary-coord boundary-telem (str "newer-" index)
                  (format "2026-08-01T00:00:%02dZ" index) "ran" "unverified"
                  "provider_terminal_success_without_external_verification" 50000))
    (let [report (-> (waste-cli boundary-coord boundary-telem "--json")
                     :out str/trim (json/parse-string true))]
      (check "trailing window includes the whole boundary run and excludes older waste"
             (and (= 21 (:availableTerminalRuns report))
                  (= 20 (:runCount report))
                  (= 1010000.0 (:windowTokenTotal report))
                  (zero? (:machineryWastedTokens report))
                  (zero? (:wasteRatioPercent report))
                  (= "PASS" (:verdict report)))))
    (finally
      (doseq [file (reverse (file-seq boundary-tmp))] (io/delete-file file true)))))

(check "waste gate threshold edges and coverage floor are exact"
       (and (= "PASS" (waste-verdict 20 90.0 10.0))
            (= "PROBATION" (waste-verdict 20 90.0 10.000001))
            (= "PROBATION" (waste-verdict 20 90.0 20.0))
            (= "FAIL" (waste-verdict 20 90.0 20.000001))
            (= "FAIL" (waste-verdict 20 89.999999 0.0))
            (= "insufficient runs" (waste-verdict 19 100.0 100.0))))

(let [empty-tmp (.toFile (java.nio.file.Files/createTempDirectory
                          "north-routing-waste-empty"
                          (make-array java.nio.file.attribute.FileAttribute 0)))
      empty-coord (io/file empty-tmp "coordination.log")
      empty-telem (io/file empty-tmp "telemetry.log")]
  (try
    (let [human (:out (waste-cli empty-coord empty-telem))
          report (-> (waste-cli empty-coord empty-telem "--json")
                     :out str/trim (json/parse-string true))]
      (check "fewer than twenty terminal runs reports insufficient runs"
             (and (zero? (:runCount report))
                  (= "insufficient runs" (:verdict report))
                  (str/includes? human "VERDICT insufficient runs"))))
    (finally
      (doseq [file (reverse (file-seq empty-tmp))] (io/delete-file file true)))))
