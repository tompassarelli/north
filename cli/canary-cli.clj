#!/usr/bin/env bb
(ns north.canary-cli)

(require '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

;; Resolved from this file only, exactly like the routing report. A managed lane
;; exports NORTH_HOME/NORTH_BIN pointing at the deployed package; honoring them
;; here would silently fold with a different tree's projection than the adapter
;; that is running, so the canary always uses its own checkout end to end.
(def NORTH
  (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
(def NORTH-CLI (str NORTH "/bin/north"))
(defn delegate-cwd
  ([] (delegate-cwd (System/getenv)))
  ([env]
   ;; The adapter's own tree is authoritative for projection reads, but an
   ;; installed adapter lives in the Nix store and cannot seed a worktree.
   ;; Delegate from a real checkout instead; callers may select another one.
   (or (get env "NORTH_CANARY_TARGET_REPO")
       (str (System/getProperty "user.home") "/code/north"))))
(def CANARY-BAR
  "Probe: write one canary file in the managed worktree, commit it, and record this exact bar with north evidence record. Expected: the write, commit, and evidence recording all succeed through the deployed production delegate path.")
(def CANARY-PIN-DETAIL-PREFIX "recurring-cross-provider-canary:")
(def CANARY-PIN-LIFETIME-SECONDS (* 15 60))
(def CANARY-REPORT-MINIMUM-RUNS 100)
(def CANARY-NORTH-FAILURE-LIMIT (/ 1.0 100.0))
(def DEFAULT-WAIT-MILLISECONDS (* 30 60 1000))
;; Each poll refolds the whole coordination + telemetry log (tens of seconds on a
;; production log), so a sub-second poll only burns CPU without observing sooner.
(def DEFAULT-POLL-MILLISECONDS 15000)
(def safe-id-pattern #"^[a-z0-9][a-z0-9_-]{0,63}$")

;; Reuse the production routing fold and its blk-pv/blk-us reason taxonomy.
;; The loaded file's executable guard does not fire because babashka.file still
;; names this canary adapter.
(load-file (str NORTH "/cli/routing-report.clj"))
(load-file (str NORTH "/cli/topology-authority.clj"))

(def usage
  (str "usage:\n"
       "  north canary run (--target ID | --matrix)\n"
       "  north canary report --window N [--json]"))

(defn- process-result [argv options timeout-ms]
  (try
    (let [child (proc/process argv
                              (merge {:out :string :err :string}
                                     options))
          result (deref child timeout-ms ::timeout)]
      (if (= result ::timeout)
        (do
          (proc/destroy-tree child)
          {:ok false :timeout true :exit 124 :out "" :err "timed out"})
        {:ok (zero? (:exit result))
         :exit (:exit result)
         :out (or (:out result) "")
         :err (or (:err result) "")}))
    (catch Exception error
      {:ok false :exit 127 :out ""
       :err (or (not-empty (.getMessage error)) (.getName (class error)))})))

(defn- run-command
  ([argv] (run-command argv {} 30000))
  ([argv options timeout-ms] (process-result argv options timeout-ms)))

(defn- failure-detail [result fallback]
  (or (not-empty (str/trim (:err result)))
      (not-empty (str/trim (:out result)))
      fallback))

(defn- bare-subject [value]
  (str/replace-first (str value) #"^@" ""))

(defn- parse-facts [raw]
  (try
    (let [facts (json/parse-string (str/trim raw) true)]
      (when (and (sequential? facts)
                 (every? #(and (map? %)
                               (= #{:predicate :value} (set (keys %)))
                               (string? (:predicate %))
                               (string? (:value %)))
                         facts))
        (vec facts)))
    (catch Exception _ nil)))

(defn- tell-fact! [subject predicate value]
  (let [result (run-command [NORTH-CLI "tell" (bare-subject subject)
                             predicate value]
                            {} 30000)]
    (when-not (:ok result)
      (throw (ex-info
              (str "could not record " predicate " on @" (bare-subject subject)
                   ": " (failure-detail result "North tell failed"))
              {:stage :tell :subject subject :predicate predicate
               :result result})))
    true))

(defn- read-thread! [thread]
  (let [result (run-command [NORTH-CLI "json" "show" (bare-subject thread)]
                            {} 30000)
        facts (when (:ok result) (parse-facts (:out result)))]
    (when-not facts
      (throw (ex-info (str "could not read captured canary thread @" thread)
                      {:stage :thread-read :thread thread :result result})))
    facts))

(defn- capture-thread! [title]
  (let [env (assoc (into {} (System/getenv))
                   "NORTH_CAPTURE_STRUCTURED" "1")
        result (run-command [NORTH-CLI "capture" title] {:env env} 30000)
        receipt (when (:ok result)
                  (try (json/parse-string (str/trim (:out result)) true)
                       (catch Exception _ nil)))
        id (:id receipt)]
    (when-not (and (map? receipt)
                   (string? id)
                   (= (str "@" id) (:thread receipt))
                   (= title (:title receipt))
                   (= true (:complete receipt))
                   (= "captured" (:reason receipt))
                   (pos-int? (:committed receipt))
                   (= (:expected receipt) (:committed receipt)))
      (throw (ex-info
              (str "North could not capture a complete canary thread: "
                   (failure-detail result "invalid structured capture receipt"))
              {:stage :capture :result result :receipt receipt})))
    id))

(defn configured-canary-targets []
  (let [targets (configured-targets)]
    (when (empty? targets)
      (throw (ex-info "no configured provider accounts are available for a canary"
                      {:stage :targets})))
    (doseq [{:keys [providerTarget provider]} targets]
      (when-not (and (re-matches safe-id-pattern providerTarget)
                     (#{"anthropic" "openai"} provider))
        (throw (ex-info
                (str "invalid configured canary target " (pr-str providerTarget)
                     " / " (pr-str provider))
                {:stage :targets :target providerTarget :provider provider}))))
    (when-not (= (count targets) (count (set (map :providerTarget targets))))
      (throw (ex-info "configured canary account IDs are not unique"
                      {:stage :targets})))
    (vec targets)))

(defn selected-targets [mode target targets]
  (case mode
    :matrix targets
    :target
    (if-let [selected (first (filter #(= target (:providerTarget %)) targets))]
      [selected]
      (throw (ex-info (str "unknown configured account: " target)
                      {:stage :targets :target target})))
    (throw (ex-info "choose exactly one of --target ID or --matrix"
                    {:stage :options}))))

(defn pin-evidence
  ([provider target thread] (pin-evidence provider target thread
                                          (java.time.Instant/now)))
  ([provider target thread ^java.time.Instant issued]
   {:policyVersion "north-routing-pin-v1"
    :issuedAt (str issued)
    :expiresAt (str (.plusSeconds issued CANARY-PIN-LIFETIME-SECONDS))
    :reasonCode "calibration-experiment"
    :detail (str CANARY-PIN-DETAIL-PREFIX "@" thread)
    :pins [{:kind "provider" :value provider}
           {:kind "account" :value target}]}))

(defn canary-task [target thread]
  (let [path (str "north-canary-" thread ".txt")]
    (str "In the supplied managed worktree, write exactly one line naming account "
         target " to " path ", stage only that file, and commit it with message "
         "\"canary: " thread "\". Then run the injected exact done_when probe and "
         "record it with north evidence record; include the commit SHA in the "
         "observed result. Do not edit any other file.")))

(defn delegate-argv [provider target thread evidence]
  [NORTH-CLI "delegate" (canary-task target thread)
   "--role" "executor"
   "--thread" thread
   "--provider" provider
   "--target" target
   "--pin-evidence" (json/generate-string evidence)])

(defn parse-control-id [output]
  (some->> (re-find #"(?m)^control:\s+([A-Za-z0-9][A-Za-z0-9._:-]*)\s*$"
                    (str output))
           second))

(defn- delegate! [provider target thread]
  (let [evidence (pin-evidence provider target thread)
        env (assoc (into {} (System/getenv)) "AGENT_WORKTREE" "1")
        ;; Admission alone; the lane detaches. Comfortably above the spawn
        ;; startup deadline so a slow provider probe is not misread as a
        ;; North-caused admission failure.
        result (run-command (delegate-argv provider target thread evidence)
                            {:env env :dir (delegate-cwd env)} 180000)
        control (when (:ok result) (parse-control-id (:out result)))]
    (when-not (and (:ok result) control)
      (throw (ex-info
              (str "production delegate admission failed for " target ": "
                   (failure-detail result "missing control ID"))
              {:stage :delegate :target target :thread thread :result result})))
    {:control control :pin evidence :delegate result}))

(defn current-run-rows []
  (-> (default-paths) read-ops fold-facts run-rows vec))

(defn terminal-row-for [control thread]
  ;; Row threads are projected through `thread-ref`, so they always carry the @.
  (let [thread-key (thread-ref (bare-subject thread))]
   (->> (current-run-rows)
       (filter #(and (= control (:agent %))
                     (= thread-key (:thread %))
                     (:processOutcomeObserved %)
                     (:deliveryOutcomeObserved %)
                     (:deliveryReasonObserved %)
                     (:at %)))
       (sort-by :at)
       last)))

(defn- wait-terminal! [control thread]
  (let [timeout-ms (or (some-> (System/getenv "NORTH_CANARY_TIMEOUT_MS")
                               parse-long)
                       DEFAULT-WAIT-MILLISECONDS)
        poll-ms (or (some-> (System/getenv "NORTH_CANARY_POLL_MS")
                            parse-long)
                    DEFAULT-POLL-MILLISECONDS)
        deadline (+ (System/currentTimeMillis) timeout-ms)]
    (loop []
      (if-let [row (terminal-row-for control thread)]
        row
        (if (>= (System/currentTimeMillis) deadline)
          (throw (ex-info
                  (str "timed out waiting for terminal run from " control)
                  {:stage :wait :control control :thread thread}))
          (do (Thread/sleep (max 1 poll-ms)) (recur)))))))

;; `verified` is strictly stronger than `reported` (independent attestation
;; rather than self-report). Today only `reported` is reachable, but treating a
;; verified canary as a failure would be a false North-caused alarm the day
;; verifier lanes land.
(def full-green-delivery-outcomes #{"reported" "verified"})

(defn derived-canary-outcome [row]
  (if (and (= "ran" (:processOutcome row))
           (full-green-delivery-outcomes (:deliveryOutcome row)))
    "full-green"
    "failure"))

(defn canary-failure-category [row]
  (when (= "failure" (derived-canary-outcome row))
    (cond
      (= "blocked" (:deliveryOutcome row))
      (blocked-failure-category (:deliveryReason row))

      (= "unverified" (:deliveryOutcome row))
      :north-caused

      :else :unattributed)))

(defn- thread-outcome [row outcome]
  (str outcome
       ": run=" (:entity row)
       " account=" (:providerTarget row)
       " process=" (:processOutcome row)
       " delivery=" (:deliveryOutcome row)
       " reason=" (:deliveryReason row)))

(defn run-one! [{:keys [providerTarget provider]}]
  (let [title (str "Production canary " provider "/" providerTarget " "
                   (java.time.Instant/now))
        thread (capture-thread! title)]
    (try
      (tell-fact! thread "done_when" CANARY-BAR)
      (let [bars (->> (read-thread! thread)
                      (filter #(= "done_when" (:predicate %)))
                      (mapv :value))]
        (when-not (= [CANARY-BAR] bars)
          (throw (ex-info
                  (str "canary thread @" thread
                       " did not read back exactly one immutable bar")
                  {:stage :bar-readback :thread thread :bars bars}))))
      (let [{:keys [control]} (delegate! provider providerTarget thread)
            row (wait-terminal! control thread)
            outcome (derived-canary-outcome row)]
        (tell-fact! (:entity row) "canary_outcome" outcome)
        (tell-fact! thread "outcome" (thread-outcome row outcome))
        (assoc row :canaryOutcome outcome :control control))
      (catch Exception error
        (try
          (tell-fact! thread "outcome"
                      (str "failure before terminal canary recording: "
                           (.getMessage error)))
          (catch Exception _ nil))
        (throw error)))))

(defn- canary-run? [row]
  (and (= "calibration-experiment" (:routingPinReasonCode row))
       (str/starts-with? (or (:routingPinDetail row) "")
                         CANARY-PIN-DETAIL-PREFIX)))

;; Every production-path managed run, canary-pinned or not. Reuses the same
;; "complete current managed run" predicate the routing performance report
;; holds itself to (composition-routed, fully attributed, no legacy debt) so a
;; real lane death (stall, hook-seam death, whatever) that never happened to be
;; a pinned calibration canary is not silently invisible to the reliability
;; window.
(defn- all-managed-run? [row]
  (complete-current-managed-run? row))

(defn- fold-report [report-name scope selector rows window]
  (let [selected (->> rows
                      (filter selector)
                      (filter #(parse-instant (:at %)))
                      (sort-by :at #(compare %2 %1))
                      (take window)
                      vec)
        with-derived
        (mapv (fn [row]
                (let [derived (derived-canary-outcome row)
                      category (canary-failure-category row)
                      recorded (vec (or (:canaryOutcomes row)
                                        (when-let [value (:canaryOutcome row)]
                                          [value])
                                        []))]
                  (assoc row
                         :derivedCanaryOutcome derived
                         :failureCategory (some-> category name)
                         :canaryRecordingStatus
                         (cond
                           (empty? recorded) "missing"
                           (= [derived] recorded) "matched"
                           :else "mismatched"))))
              selected)
        frequencies' (frequencies (keep :failureCategory with-derived))
        full-green (count (filter #(= "full-green"
                                      (:derivedCanaryOutcome %))
                                  with-derived))
        failures (- (count with-derived) full-green)
        provider-failures (get frequencies' "provider-caused" 0)
        north-failures (get frequencies' "north-caused" 0)
        suspect-failures (get frequencies' "suspect-lapse" 0)
        unattributed-failures (get frequencies' "unattributed" 0)
        runs (count with-derived)
        rate (fn [n] (if (zero? runs) 0.0 (/ (double n) runs)))]
    {:report report-name
     :scope scope
     :windowRequested window
     :runs runs
     :windowComplete (= runs window)
     :fullGreen full-green
     :failures failures
     :providerCausedFailures provider-failures
     :northCausedFailures north-failures
     :suspectLapseFailures suspect-failures
     :unattributedFailures unattributed-failures
     :providerCausedFailureRate (rate provider-failures)
     :northCausedFailureRate (rate north-failures)
     :recordingMissing (count (filter #(= "missing"
                                           (:canaryRecordingStatus %))
                                     with-derived))
     :recordingMismatched (count (filter #(= "mismatched"
                                              (:canaryRecordingStatus %))
                                        with-derived))
     :reliabilityBar
     {:minimumRuns CANARY-REPORT-MINIMUM-RUNS
      :maximumNorthCausedFailureRate CANARY-NORTH-FAILURE-LIMIT
      :met (and (>= runs CANARY-REPORT-MINIMUM-RUNS)
                (<= (rate north-failures) CANARY-NORTH-FAILURE-LIMIT))}
     :runsDetail
     (mapv #(select-keys %
                         [:at :entity :thread :providerTarget :provider
                          :processOutcome :deliveryOutcome :deliveryReason
                          :canaryOutcome :canaryOutcomes
                          :derivedCanaryOutcome :failureCategory
                          :canaryRecordingStatus])
           with-derived)}))

(defn canary-report [rows window]
  (fold-report "canary" "production-delegate-calibration-runs"
               canary-run? rows window))

;; The all-managed fold is the signed exit-bar number: it is not scoped to the
;; calibration-experiment pin, so a real production lane death that was never
;; a pinned canary still counts against the reliability window.
(defn all-managed-report [rows window]
  (fold-report "canary-all-managed" "all-production-path-managed-runs"
               all-managed-run? rows window))

(defn full-report [rows window]
  (assoc (canary-report rows window)
         :allManaged (all-managed-report rows window)))

(defn- percent-label [rate]
  (format "%.2f%%" (* 100.0 rate)))

(defn- print-section [report label]
  (println (str label " — last " (:windowRequested report)
                " production-path run(s)"))
  (println (format "runs %d/%d · full-green %d · failures %d"
                   (:runs report) (:windowRequested report)
                   (:fullGreen report) (:failures report)))
  (println (format "provider-caused %d (%s) · North-caused %d (%s) · suspect-lapse %d · unattributed %d"
                   (:providerCausedFailures report)
                   (percent-label (:providerCausedFailureRate report))
                   (:northCausedFailures report)
                   (percent-label (:northCausedFailureRate report))
                   (:suspectLapseFailures report)
                   (:unattributedFailures report)))
  (println (str "exit bar >=100 runs and North-caused <=1/100: "
                (if (get-in report [:reliabilityBar :met]) "MET" "NOT YET")))
  (when (or (pos? (:recordingMissing report))
            (pos? (:recordingMismatched report)))
    (println (format "canary outcome recording anomalies: missing=%d mismatched=%d"
                     (:recordingMissing report)
                     (:recordingMismatched report))))
  (println)
  (println (format "%-25s %-34s %-10s %-12s %-12s %-14s %s"
                   "AT" "ACCOUNT" "PROVIDER" "OUTCOME" "PROCESS" "DELIVERY"
                   "REASON / CATEGORY"))
  (doseq [row (:runsDetail report)]
    (println
     (format "%-25s %-34s %-10s %-12s %-12s %-14s %s / %s"
             (:at row) (:providerTarget row) (:provider row)
             (:derivedCanaryOutcome row) (:processOutcome row)
             (:deliveryOutcome row) (:deliveryReason row)
             (or (:failureCategory row) "full-green")))))

;; Unchanged for existing consumers when given a bare `canary-report` result
;; (no :allManaged key): same header, same lines, same table. `full-report`
;; adds the all-managed section below it; that second section's exit-bar line
;; is the signed reliability number, since it is not scoped to the
;; calibration-experiment pin the canary-only section is.
(defn print-report [report]
  (print-section report "CANARY PERFORMANCE")
  (when-let [all-managed (:allManaged report)]
    (println)
    (print-section all-managed "ALL-MANAGED PERFORMANCE")))

(defn parse-run-options [args]
  (loop [remaining (vec args) parsed {}]
    (if (empty? remaining)
      parsed
      (let [[arg value & more] remaining]
        (case arg
          "--matrix"
          (if (or (:mode parsed) value)
            (throw (ex-info "--matrix cannot be combined with other values"
                            {:stage :options}))
            (recur [] (assoc parsed :mode :matrix)))

          "--target"
          (if (or (:mode parsed) (nil? value) (str/starts-with? value "--"))
            (throw (ex-info "--target requires exactly one account ID"
                            {:stage :options}))
            (recur more (assoc parsed :mode :target :target value)))

          (throw (ex-info (str "unknown canary run option: " arg)
                          {:stage :options})))))))

(defn parse-report-options [args]
  (loop [remaining (vec args) parsed {:json? false}]
    (if (empty? remaining)
      parsed
      (let [[arg value & more] remaining]
        (case arg
          "--json"
          (if (:json? parsed)
            (throw (ex-info "duplicate --json" {:stage :options}))
            (recur (vec (rest remaining)) (assoc parsed :json? true)))

          "--window"
          (let [window (when (and value (not (str/starts-with? value "--")))
                         (try (parse-long value) (catch Exception _ nil)))]
            (if-not (and window (pos? window))
              (throw (ex-info "--window requires a positive integer"
                              {:stage :options}))
              (recur more (assoc parsed :window window))))

          (throw (ex-info (str "unknown canary report option: " arg)
                          {:stage :options})))))))

(defn cmd-run [args]
  (north.topology-authority/require-coordination! "canary run")
  (let [{:keys [mode target]} (parse-run-options args)
        selected (selected-targets mode target (configured-canary-targets))
        results
        (mapv
         (fn [{:keys [providerTarget] :as account}]
           (println (str "CANARY " providerTarget " — dispatching through north delegate"))
           (try
             (let [row (run-one! account)]
               (println (format "  %s process=%s delivery=%s reason=%s run=%s"
                                (:canaryOutcome row) (:processOutcome row)
                                (:deliveryOutcome row) (:deliveryReason row)
                                (:entity row)))
               row)
             (catch Exception error
               (binding [*out* *err*]
                 (println (str "  failure: " (.getMessage error))))
               {:providerTarget providerTarget
                :canaryOutcome "failure"
                :error (.getMessage error)})))
         selected)]
    (println (format "CANARY MATRIX — %d/%d full-green"
                     (count (filter #(= "full-green" (:canaryOutcome %)) results))
                     (count results)))
    (if (every? #(= "full-green" (:canaryOutcome %)) results) 0 1)))

(defn cmd-report [args]
  (let [{:keys [window json?]} (parse-report-options args)]
    (when-not window
      (throw (ex-info "canary report requires --window N"
                      {:stage :options})))
    (let [report (full-report (current-run-rows) window)]
      (if json?
        (println (json/generate-string report))
        (print-report report)))
    0))

(defn -main [& args]
  (let [[command & rest] args]
    (try
      (let [status (case command
                     "run" (cmd-run rest)
                     "report" (cmd-report rest)
                     ("help" "--help" "-h") (do (println usage) 0)
                     (throw (ex-info usage {:stage :options})))]
        (System/exit status))
      (catch Exception error
        (binding [*out* *err*]
          (println (.getMessage error))
          (when (= :options (:stage (ex-data error)))
            (println usage)))
        (System/exit (if (north.topology-authority/denial? error) 1 2))))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
