#!/usr/bin/env bb
(ns north.lane-report
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import [java.time Instant ZoneOffset]
           [java.time.temporal WeekFields]))

(def canonical-arms ["graph" "text" "forced-graph" "forced-text" "na"])
(def successful-outcomes #{"landed" "returned"})
(def ^:dynamic *now* #(Instant/now))

(defn bare [value] (str/replace (str value) #"^@" ""))
(defn number [value]
  (when (and value (re-matches #"[0-9]+" (str value))) (parse-long (str value))))

(defn fold-log [entities path]
  (if-not (and path (.isFile (io/file path)))
    entities
    (with-open [reader (io/reader path)]
      (reduce
       (fn [state line]
         (let [fact (try (edn/read-string line) (catch Exception _ nil))
               subject (some-> (:l fact) bare)
               predicate (some-> (:p fact) str)
               object (some-> (:r fact) str)]
           (if (and (map? fact) (string? subject)
                    (or (str/starts-with? subject "run:")
                        (str/starts-with? subject "run-")))
             (if (= "retract" (str (:op fact)))
               (if (= object (get-in state [subject :facts predicate]))
                 (update-in state [subject :facts] dissoc predicate)
                 state)
               (-> state
                   (assoc-in [subject :subject] subject)
                   (assoc-in [subject :facts predicate] object)
                   (assoc-in [subject :tx] (or (:tx fact) 0))))
             state)))
       entities (line-seq reader)))))

(defn configured-paths []
  (let [home (System/getenv "HOME")
        coordination (or (System/getenv "FRAM_LOG")
                         (str home "/.local/state/north/coordination.log"))
        telemetry (or (System/getenv "NORTH_TELEMETRY_LOG")
                      (str home "/.local/state/north/telemetry.log"))]
    (distinct [coordination telemetry])))

(defn load-data [paths]
  (let [existing (filter #(.isFile (io/file %)) paths)]
    (when (empty? existing)
      (throw (ex-info "no coordination or telemetry log is readable" {:paths paths})))
    (let [entities (->> existing (reduce fold-log {}) vals)
          with-meta (map #(assoc (:facts %) "subject" (:subject %) "__tx" (:tx %)) entities)]
      {:runs (->> with-meta
                  (filter #(and (= "run" (get % "kind")) (get % "run_start")))
                  vec)
       :estimates (->> with-meta
                       (filter #(and (= "estimate" (get % "kind"))
                                     (get % "estimate_of")
                                     (get % "estimate_by")))
                       vec)})))

(defn median [values]
  (let [ordered (vec (sort values)) n (count ordered)]
    (when (pos? n)
      (if (odd? n)
        (nth ordered (quot n 2))
        (/ (+ (nth ordered (dec (quot n 2))) (nth ordered (quot n 2))) 2.0)))))

(defn render-number [value]
  (cond
    (nil? value) "—"
    (integer? value) (str value)
    (= value (Math/floor (double value))) (str (long value))
    :else (format "%.1f" (double value))))

(defn rate [predicate runs]
  (if (seq runs)
    (format "%.1f%%" (* 100.0 (/ (count (filter predicate runs)) (double (count runs)))))
    "—"))

(defn complete? [run]
  (every? #(some? (get run %))
          ["run_end" "run_wall_ms" "run_outcome" "run_retries"]))

(defn token-total [run]
  (when (= "exact" (get run "run_token_status"))
    (let [in (number (get run "run_tokens_in"))
          out (number (get run "run_tokens_out"))]
      (when (and (some? in) (some? out)) (+ in out)))))

(defn aggregate [runs]
  (let [runs (vec runs)
        token-values (keep token-total runs)]
    {:count (count runs)
     :wall (median (keep #(number (get % "run_wall_ms")) runs))
     :tokens (median token-values)
     :token-count (count token-values)
     :first-try (rate #(and (contains? successful-outcomes (get % "run_outcome"))
                            (zero? (or (number (get % "run_retries")) -1))) runs)
     :landed (rate #(= "landed" (get % "run_outcome")) runs)}))

(defn token-cell [{:keys [tokens token-count count]}]
  (if (zero? token-count)
    (format "cannot-determine (0/%d)" count)
    (format "%s (%d/%d exact)" (render-number tokens) token-count count)))

(defn week [run]
  (try
    (let [date (.toLocalDate (.atZone (Instant/parse (get run "run_start")) ZoneOffset/UTC))
          fields WeekFields/ISO]
      (format "%04d-W%02d"
              (.get date (.weekBasedYear fields))
              (.get date (.weekOfWeekBasedYear fields))))
    (catch Exception _ "unknown")))

(defn arm-order [arm]
  [(or (first (keep-indexed #(when (= %2 arm) %1) canonical-arms)) 99) arm])

(defn family [arm]
  (case arm
    ("graph" "forced-graph") "graph"
    ("text" "forced-text") "text"
    nil))

(defn size-bucket [run]
  (let [files (number (get run "run_size_files"))
        lines (number (get run "run_size_lines"))]
    (cond
      (and (nil? files) (nil? lines)) "unknown"
      (or (> (or files 0) 20) (> (or lines 0) 1000)) "large"
      (or (> (or files 0) 5) (> (or lines 0) 250)) "medium"
      :else "small")))

(defn estimate-order [estimate]
  [(get estimate "estimate_at" "") (or (number (get estimate "__tx")) 0)
   (get estimate "subject" "")])

(defn estimate-index [estimates]
  (group-by #(bare (get % "estimate_of")) estimates))

(defn estimate-projections [run estimates]
  (let [ordered (sort-by estimate-order estimates)
        dispatcher (or (get run "run_dispatcher") (get (first ordered) "estimate_by"))]
    {:dispatcher dispatcher
     :dispatch (first (filter #(= dispatcher (get % "estimate_by")) ordered))
     :effective (last ordered)
     :per-agent (->> ordered
                     (group-by #(get % "estimate_by"))
                     (map (fn [[by events]] [by (last events)]))
                     (into {}))}))

(defn actual-wall-min [run]
  (some-> (number (get run "run_wall_ms")) (/ 60000.0)))

(defn relative-error [estimate actual]
  (when (and estimate actual (pos? estimate))
    (/ (Math/abs (- (double estimate) (double actual))) (double estimate))))

(defn calibration [run estimate]
  {:token (relative-error (number (get estimate "estimate_tokens")) (token-total run))
   :wall (relative-error (number (get estimate "estimate_wall_min")) (actual-wall-min run))})

(defn mean [values]
  (when (seq values) (/ (reduce + values) (double (count values)))))

(defn percent [value]
  (if (some? value) (format "%.1f%%" (* 100.0 value)) "—"))

(defn ratio-cell [value]
  (if (some? value) (format "%.2fx" (double value)) "—"))

(defn estimate-ratio [actual estimate]
  (when (and actual estimate (pos? estimate)) (/ (double actual) (double estimate))))

(defn print-attention [runs estimates now factor]
  (let [by-run (estimate-index estimates)
        rows
        (keep
         (fn [run]
           (let [{:keys [effective]} (estimate-projections run (get by-run (get run "subject")))
                 est-wall (number (get effective "estimate_wall_min"))
                 est-tokens (number (get effective "estimate_tokens"))
                 finished? (complete? run)
                 wall-ratio (if finished?
                              (estimate-ratio (actual-wall-min run) est-wall)
                              (when est-wall
                                (try
                                  (estimate-ratio
                                   (/ (.toMillis (java.time.Duration/between
                                                  (Instant/parse (get run "run_start")) now))
                                      60000.0)
                                   est-wall)
                                  (catch Exception _ nil))))
                 token-ratio (when finished? (estimate-ratio (token-total run) est-tokens))
                 worst (apply max 0.0 (keep identity [wall-ratio token-ratio]))
                 attention? (if finished?
                              (> worst factor)
                              (> (or wall-ratio 0.0) 1.0))]
             (when (and effective attention?)
               {:run (get run "subject") :arm (get run "run_arm" "unknown")
                :status (if finished? "finished" "open")
                :by (bare (get effective "estimate_by"))
                :wall wall-ratio :tokens token-ratio :worst worst})))
         runs)]
    (println "ATTENTION LIST")
    (println (format "open past effective wall estimate; finished above %.2fx effective estimate" factor))
    (printf "%-44s %-9s %-13s %-28s %9s %9s%n"
            "run" "status" "arm" "effective estimator" "wall" "tokens")
    (if (seq rows)
      (doseq [{:keys [run status arm by wall tokens]}
              (sort-by (juxt (comp - :worst) :run) rows)]
        (printf "%-44s %-9s %-13s %-28s %9s %9s%n"
                run status arm by (ratio-cell wall) (ratio-cell tokens)))
      (println "none"))))

(defn trend-rows [runs estimates]
  (let [by-run (estimate-index estimates)]
    (->> runs
         (filter complete?)
         (keep (fn [run]
                 (when-let [estimate (:dispatch (estimate-projections
                                                 run (get by-run (get run "subject"))))]
                   (merge {:week (week run) :arm (get run "run_arm" "unknown")
                           :bucket (size-bucket run)}
                          (calibration run estimate)))))
         (group-by (juxt :week :arm :bucket)))))

(defn print-calibration-trend [runs estimates]
  (println "\nCALIBRATION TREND — dispatcher first estimate")
  (printf "%-10s %-13s %-8s %7s %13s %7s %13s%n"
          "week" "arm" "bucket" "tok-n" "token error" "wall-n" "wall error")
  (doseq [[[week-value arm bucket] rows]
          (sort-by (fn [[[week-value arm bucket]]]
                     [week-value (arm-order arm) bucket])
                   (trend-rows runs estimates))]
    (let [token-errors (keep :token rows) wall-errors (keep :wall rows)]
      (printf "%-10s %-13s %-8s %7d %13s %7d %13s%n"
              week-value arm bucket (count token-errors) (percent (mean token-errors))
              (count wall-errors) (percent (mean wall-errors))))))

(defn context-values [samples key]
  (let [values (->> samples (keep #(get (:run %) key)) distinct sort)]
    (if (seq values) (str/join "," values) "—")))

(defn divergent-dimension? [left right]
  (and left right (pos? left) (pos? right)
       (> (/ (double (max left right)) (double (min left right))) 2.0)))

(defn combined-error [calibration]
  (mean (keep calibration [:token :wall])))

(defn divergence-result [run dispatch worker]
  (let [dispatch-tokens (number (get dispatch "estimate_tokens"))
        worker-tokens (number (get worker "estimate_tokens"))
        dispatch-wall (number (get dispatch "estimate_wall_min"))
        worker-wall (number (get worker "estimate_wall_min"))]
    (when (or (divergent-dimension? dispatch-tokens worker-tokens)
              (divergent-dimension? dispatch-wall worker-wall))
      (let [dispatch-error (combined-error (calibration run dispatch))
            worker-error (combined-error (calibration run worker))]
        (cond
          (or (nil? dispatch-error) (nil? worker-error)) :unknown
          (< worker-error dispatch-error) :worker
          (> worker-error dispatch-error) :dispatcher
          :else :tie)))))

(defn estimator-samples [runs estimates]
  (let [by-run (estimate-index estimates)]
    (mapcat
     (fn [run]
       (let [{:keys [dispatch dispatcher per-agent]}
             (estimate-projections run (get by-run (get run "subject")))]
         (for [[by estimate] per-agent]
           {:by by :run run :estimate estimate :calibration (calibration run estimate)
            :divergence (when (and dispatch (not= by dispatcher))
                          (divergence-result run dispatch estimate))})))
     (filter complete? runs))))

(defn print-estimator-calibration [runs estimates]
  (let [samples (estimator-samples runs estimates)
        grouped (group-by :by samples)]
    (println "\nPER-ESTIMATOR CALIBRATION — latest per agent/run; combined weighted by available samples")
    (printf "%-28s %7s %13s %7s %13s %13s %-16s %-20s%n"
            "estimator" "tok-n" "token error" "wall-n" "wall error"
            "combined" "roles" "models")
    (doseq [[by rows]
            (sort-by (fn [[by rows]]
                       [(or (mean (keep (comp combined-error :calibration) rows))
                            Double/POSITIVE_INFINITY)
                        (- (count rows)) by])
                     grouped)]
      (let [token-errors (keep (comp :token :calibration) rows)
            wall-errors (keep (comp :wall :calibration) rows)
            all-errors (mapcat #(keep (:calibration %) [:token :wall]) rows)]
        (printf "%-28s %7d %13s %7d %13s %13s %-16s %-20s%n"
                (bare by) (count token-errors) (percent (mean token-errors))
                (count wall-errors) (percent (mean wall-errors))
                (percent (mean all-errors)) (context-values rows "run_role")
                (context-values rows "run_model"))))
    (println "\nDIVERGENCE ANALYTICS — >2x worker/dispatcher disagreement")
    (printf "%-28s %7s %14s %18s %7s%n"
            "worker" "events" "worker righter" "dispatcher righter" "ties")
    (let [divergent (->> samples (filter :divergence) (group-by :by))]
      (if (seq divergent)
        (doseq [[by rows] (sort-by key divergent)]
          (let [counts (frequencies (map :divergence rows))]
            (printf "%-28s %7d %14d %18d %7d%n"
                    (bare by) (count rows) (get counts :worker 0)
                    (get counts :dispatcher 0) (get counts :tie 0))))
        (println "none")))))

(defn print-summary-row [label aggregate]
  (printf "%-13s %5d %14s %-28s %15s %9s%n"
          label (:count aggregate) (render-number (:wall aggregate))
          (token-cell aggregate) (:first-try aggregate) (:landed aggregate)))

(defn print-report [runs estimates now attention-factor]
  (let [complete (filterv complete? runs)
        by-arm (group-by #(get % "run_arm" "unknown") complete)]
    (print-attention runs estimates now attention-factor)
    (print-calibration-trend runs estimates)
    (print-estimator-calibration runs estimates)

    (println "\nPER-ARM")
    (printf "%-13s %5s %14s %-28s %15s %9s%n"
            "arm" "count" "median wall ms" "median tokens" "first-try pass" "landed")
    (doseq [arm (sort-by arm-order (keys by-arm))]
      (print-summary-row arm (aggregate (get by-arm arm))))

    (println "\nWEEKLY TREND")
    (printf "%-10s %-13s %5s %14s %-28s %15s %9s%n"
            "week" "arm" "count" "median wall ms" "median tokens" "first-try pass" "landed")
    (doseq [[[week-value arm] arm-runs]
            (sort-by (fn [[[week-value arm]]] [week-value (arm-order arm)])
                     (group-by (juxt week #(get % "run_arm" "unknown")) complete))]
      (let [summary (aggregate arm-runs)]
        (printf "%-10s " week-value)
        (print-summary-row arm summary)))

    (println "\nTASK SIZE — graph vs text")
    (println "buckets: small <=5 files/250 lines; medium <=20 files/1000 lines; large above; unknown unmeasured")
    (printf "%-8s %-5s %12s %-20s %-8s %12s %-20s%n"
            "bucket" "g-n" "g-wall-ms" "g-tokens" "t-n" "t-wall-ms" "t-tokens")
    (let [comparison (->> complete
                          (keep #(when-let [arm-family (family (get % "run_arm"))]
                                   (assoc % ::family arm-family ::bucket (size-bucket %))))
                          (group-by (juxt ::bucket ::family)))]
      (doseq [bucket ["small" "medium" "large" "unknown"]
              :let [graph-summary (aggregate (get comparison [bucket "graph"] []))
                    text-summary (aggregate (get comparison [bucket "text"] []))]
              :when (or (pos? (:count graph-summary)) (pos? (:count text-summary)))]
        (printf "%-8s %-5d %12s %-20s %-8d %12s %-20s%n"
                bucket (:count graph-summary) (render-number (:wall graph-summary))
                (token-cell graph-summary) (:count text-summary)
                (render-number (:wall text-summary)) (token-cell text-summary))))
    (when (< (count complete) (count runs))
      (printf "\nINCOMPLETE: %d started run(s) omitted from aggregates%n"
              (- (count runs) (count complete))))))

(defn attention-factor [args]
  (cond
    (empty? args) 1.5
    (and (= 2 (count args)) (= "--attention-factor" (first args)))
    (let [raw (second args)
          value (try (Double/parseDouble raw) (catch Exception _ nil))]
      (when-not (and value (Double/isFinite value) (pos? value))
        (throw (ex-info "--attention-factor requires a positive number" {:type :usage})))
      value)
    :else (throw (ex-info "usage: north-lane-report [--attention-factor N]" {:type :usage}))))

(defn -main [& args]
  (try
    (let [{:keys [runs estimates]} (load-data (configured-paths))]
      (print-report runs estimates (*now*) (attention-factor args)))
    (catch Exception error
      (binding [*out* *err*]
        (println (str "north-lane-report: " (.getMessage error))))
      (System/exit 2))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
