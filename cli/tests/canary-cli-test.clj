#!/usr/bin/env bb
(ns north.canary-cli-test)

(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file")))
            "../..")))
(load-file (str root "/cli/canary-cli.clj"))

(def checks (atom []))
(defn check [label ok?]
  (swap! checks conj [label (boolean ok?)])
  (println (str (if ok? "ok:   " "FAIL: ") label))
  (when-not ok? (System/exit 1)))

(defn refused? [thunk]
  (try (thunk) false (catch Exception _ true)))

(def issued (java.time.Instant/parse "2026-07-26T01:02:03Z"))
(let [evidence (north.canary-cli/pin-evidence
                "openai" "codex-personal" "thread-canary" issued)]
  (check
   "pin evidence exactly pins provider + account with calibration provenance and UTC expiry"
   (= {:policyVersion "north-routing-pin-v1"
       :issuedAt "2026-07-26T01:02:03Z"
       :expiresAt "2026-07-26T01:17:03Z"
       :reasonCode "calibration-experiment"
       :detail "recurring-cross-provider-canary:@thread-canary"
       :pins [{:kind "provider" :value "openai"}
              {:kind "account" :value "codex-personal"}]}
      evidence))
  (let [argv (north.canary-cli/delegate-argv
              "openai" "codex-personal" "thread-canary" evidence)]
    (check
     "the canary dispatches through the production north delegate surface"
     (and (= "delegate" (second argv))
          (= ["--role" "executor" "--thread" "thread-canary"
              "--provider" "openai" "--target" "codex-personal"]
             (subvec argv 3 11))
          (= "--pin-evidence" (nth argv 11))
          (str/includes? (nth argv 2) "north evidence record")))))

(check
 "control IDs are parsed only from the delegate machine-control line"
 (= "lane-ms1-canary-1234"
    (north.canary-cli/parse-control-id
     "spawned canary\ncontrol: lane-ms1-canary-1234\nwatch: north watch ...\n")))

(let [targets [{:providerTarget "claude-a" :provider "anthropic"}
               {:providerTarget "codex-a" :provider "openai"}]]
  (check "matrix selection preserves the configured sequential account order"
         (= targets (north.canary-cli/selected-targets :matrix nil targets)))
  (check "target selection resolves one exact configured account"
         (= [(second targets)]
            (north.canary-cli/selected-targets :target "codex-a" targets))))

(defn canary-row
  [at target process delivery reason marker]
  {:at at
   :entity (str "@run-" target)
   :thread (str "thread-" target)
   :agent (str "lane-" target)
   :providerTarget target
   :provider (if (str/starts-with? target "claude") "anthropic" "openai")
   :processOutcome process
   :processOutcomeObserved true
   :deliveryOutcome delivery
   :deliveryOutcomeObserved true
   :deliveryReason reason
   :deliveryReasonObserved true
   :routingPinReasonCode "calibration-experiment"
   :routingPinDetail (str north.canary-cli/CANARY-PIN-DETAIL-PREFIX
                          "@thread-" target)
   :canaryOutcome marker
   ;; A pinned canary is still routed through the same production composition
   ;; path as any other managed lane, so it must also satisfy the all-managed
   ;; fold's "complete current managed run" predicate.
   :compositionKind "preset"
   :compositionId "canary-role"
   :tier "standard"
   :role "canary-role"
   :taskGrade "mid"
   :model "test-model"
   :effort "medium"
   :legacyDebtReasons []})

(defn managed-row
  "A production-path managed run that is NOT a calibration-pinned canary —
  the shape of a real managed lane death (stall, hook-seam error, ...)."
  [at target process delivery reason]
  {:at at
   :entity (str "@run-" target)
   :thread (str "thread-" target)
   :agent (str "lane-" target)
   :providerTarget target
   :provider (if (str/starts-with? target "claude") "anthropic" "openai")
   :processOutcome process
   :processOutcomeObserved true
   :deliveryOutcome delivery
   :deliveryOutcomeObserved true
   :deliveryReason reason
   :deliveryReasonObserved true
   :compositionKind "preset"
   :compositionId "director"
   :tier "frontier"
   :role "director"
   :taskGrade "staff"
   :model "test-model"
   :effort "xhigh"
   :legacyDebtReasons []})

(let [rows [(canary-row "2026-07-26T01:00:00Z" "codex-old"
                        "ran" "reported"
                        "complete_run_scoped_done_bar_evidence_self_reported"
                        "full-green")
            (canary-row "2026-07-26T02:00:00Z" "codex-green"
                        "ran" "reported"
                        "complete_run_scoped_done_bar_evidence_self_reported"
                        "full-green")
            (canary-row "2026-07-26T03:00:00Z" "codex-provider"
                        "died" "blocked" "provider_process_died" "failure")
            (canary-row "2026-07-26T04:00:00Z" "claude-north"
                        "blocked_preflight" "blocked"
                        "execution_preflight_blocked" "failure")
            (canary-row "2026-07-26T05:00:00Z" "codex-unverified"
                        "ran" "unverified"
                        "delivery_bar_evidence_incomplete" "failure")
            (canary-row "2026-07-26T06:00:00Z" "claude-lapse"
                        "died" "blocked"
                        "presence_lapsed_without_committed_terminal" nil)
            ;; Same reason code, but not this recurring canary.
            (assoc (canary-row "2026-07-26T07:00:00Z" "codex-other"
                               "ran" "reported"
                               "complete_run_scoped_done_bar_evidence_self_reported"
                               "full-green")
                   :routingPinDetail "another-calibration")]
      report (north.canary-cli/canary-report rows 5)]
  (check
   "the rolling report takes the latest N production canaries and splits existing failure semantics"
   (and (= 5 (:runs report))
        (= 1 (:fullGreen report))
        (= 4 (:failures report))
        (= 1 (:providerCausedFailures report))
        (= 2 (:northCausedFailures report))
        (= 1 (:suspectLapseFailures report))
        (zero? (:unattributedFailures report))
        (= 0.4 (:northCausedFailureRate report))
        (= 1 (:recordingMissing report))
        (zero? (:recordingMismatched report))
        (= "claude-lapse" (get-in report [:runsDetail 0 :providerTarget]))
        (= "codex-green" (get-in report [:runsDetail 4 :providerTarget]))))
  (check
   "the fixed reliability bar stays provisional below one hundred runs"
   (false? (get-in report [:reliabilityBar :met]))))

;; Real managed lane deaths (lane-ms1wgkjg stalled, lane-ms1ww8tl hook-seam
;; death) are production-path managed runs but were never a pinned
;; calibration-experiment canary, so canary-run? never saw them. The
;; all-managed fold must still count them.
(let [canary-rows [(canary-row "2026-07-26T01:00:00Z" "codex-c1"
                               "ran" "reported"
                               "complete_run_scoped_done_bar_evidence_self_reported"
                               "full-green")
                   (canary-row "2026-07-26T02:00:00Z" "codex-c2"
                               "ran" "reported"
                               "complete_run_scoped_done_bar_evidence_self_reported"
                               "full-green")]
      real-deaths [(managed-row "2026-07-26T14:49:21Z" "codex-analyst-death"
                                "provider_error" "blocked" "provider_terminal_error")
                   (managed-row "2026-07-26T14:55:25Z" "codex-director-stall"
                                "stalled" "blocked" "provider_process_stalled")]
      rows (into canary-rows real-deaths)
      canary-only (north.canary-cli/canary-report rows 10)
      all-managed (north.canary-cli/all-managed-report rows 10)
      full (north.canary-cli/full-report rows 10)]
  (check
   "canary-only report stays blind to non-canary-pinned managed lane deaths"
   (and (= 2 (:runs canary-only))
        (= 2 (:fullGreen canary-only))
        (zero? (:failures canary-only))))
  (check
   "the all-managed fold counts every production-path managed run, canary or not"
   (and (= 4 (:runs all-managed))
        (= 2 (:fullGreen all-managed))
        (= 2 (:failures all-managed))
        (= 2 (:providerCausedFailures all-managed))
        (zero? (:northCausedFailures all-managed))
        (= #{"codex-c1" "codex-c2" "codex-analyst-death" "codex-director-stall"}
           (set (map :providerTarget (:runsDetail all-managed))))))
  (check
   "full-report keeps the existing canary-only shape unchanged and nests the all-managed fold under it"
   (and (= canary-only (dissoc full :allManaged))
        (= all-managed (:allManaged full))))
  (check
   "the all-managed section carries the signed exit-bar number, distinct from the canary-only one"
   (and (contains? (:allManaged full) :reliabilityBar)
        (= (:reliabilityBar all-managed)
           (get-in full [:allManaged :reliabilityBar])))))

(let [output (with-out-str
              (north.canary-cli/print-report
               (north.canary-cli/canary-report
                [(canary-row "2026-07-26T01:00:00Z" "codex-c1"
                             "ran" "reported"
                             "complete_run_scoped_done_bar_evidence_self_reported"
                             "full-green")]
                5)))]
  (check
   "print-report keeps the canary-only console shape unchanged for a bare canary report"
   (and (str/includes? output "CANARY PERFORMANCE")
        (not (str/includes? output "ALL-MANAGED PERFORMANCE")))))

(let [output (with-out-str
              (north.canary-cli/print-report
               (north.canary-cli/full-report
                [(canary-row "2026-07-26T01:00:00Z" "codex-c1"
                             "ran" "reported"
                             "complete_run_scoped_done_bar_evidence_self_reported"
                             "full-green")
                 (managed-row "2026-07-26T14:55:25Z" "codex-director-stall"
                              "stalled" "blocked" "provider_process_stalled")]
                5)))]
  (check
   "print-report appends the all-managed section, with its own signed exit bar, alongside the unchanged canary section"
   (and (str/includes? output "CANARY PERFORMANCE")
        (str/includes? output "ALL-MANAGED PERFORMANCE")
        (str/includes? output "codex-director-stall"))))

(let [events (atom [])
      terminal {:entity "@run-canary"
                :thread "thread-canary"
                :providerTarget "codex-personal"
                :processOutcome "ran"
                :deliveryOutcome "reported"
                :deliveryReason
                "complete_run_scoped_done_bar_evidence_self_reported"}]
  (with-redefs-fn
    {#'north.canary-cli/capture-thread!
     (fn [_] (swap! events conj :capture) "thread-canary")
     #'north.canary-cli/tell-fact!
     (fn [subject predicate value]
       (swap! events conj [:tell subject predicate value])
       true)
     #'north.canary-cli/read-thread!
     (fn [_]
       (swap! events conj :bar-readback)
       [{:predicate "done_when" :value north.canary-cli/CANARY-BAR}])
     #'north.canary-cli/delegate!
     (fn [_provider _target _thread]
       (swap! events conj :delegate)
       {:control "lane-canary"})
     #'north.canary-cli/wait-terminal!
     (fn [_control _thread]
       (swap! events conj :wait-terminal)
       terminal)}
    (fn []
      (let [result
            (north.canary-cli/run-one!
             {:providerTarget "codex-personal" :provider "openai"})]
        (check
         "one run captures one bar, delegates, waits terminal, then records run + thread outcomes"
         (and (= "full-green" (:canaryOutcome result))
              (= [:capture
                  [:tell "thread-canary" "done_when"
                   north.canary-cli/CANARY-BAR]
                  :bar-readback
                  :delegate
                  :wait-terminal
                  [:tell "@run-canary" "canary_outcome" "full-green"]]
                 (vec (butlast @events)))
              (= "outcome" (nth (last @events) 2))
              (str/includes? (nth (last @events) 3)
                             "process=ran delivery=reported")))))))

;; A managed lane exports NORTH_HOME/NORTH_BIN at the deployed package. If the
;; adapter honored them it would fold with a different tree's row projection
;; than the code under test, and every canary row would silently disappear.
(check "the adapter resolves North from its own checkout, not the ambient package env"
       (and (= root north.canary-cli/NORTH)
            (= (str root "/bin/north") north.canary-cli/NORTH-CLI)))

(check "the composed delegate runs from a target checkout, not the adapter tree"
       (and (= (str (System/getProperty "user.home") "/code/north")
               (north.canary-cli/delegate-cwd {}))
            (= "/tmp/canary-target"
               (north.canary-cli/delegate-cwd
                {"NORTH_CANARY_TARGET_REPO" "/tmp/canary-target"}))))

;; Poll one exact terminal subject. The report command keeps the corpus fold,
;; but a live canary must never repeat that global read while it waits.
(let [subject "@run:lane-canary-1234"
      rows [["kind" "run"]
            ["agent" "lane-canary"]
            ["thread" "@019f-thread"]
            ["at" "2026-07-26T08:00:00Z"]
            ["outcome" "ran"]
            ["process_outcome" "ran"]
            ["provider_target" "codex-a"]
            ["delivery_outcome" "unverified"]
            ["delivery_reason" "delivery_bar_evidence_incomplete"]]
      query-calls (atom [])
      show-calls (atom [])]
  (with-redefs-fn {#'north.coord/indexed-query-in-domain
                   (fn [port domain query limit]
                     (swap! query-calls conj [port domain query limit])
                     {:ok [[subject]] :version 1 :engine "index"})
                   #'north.coord/show-envelope
                   (fn [port actual-subject]
                     (swap! show-calls conj [port actual-subject])
                     {:version 1 :rows rows})
                   #'north.canary-cli/current-run-rows
                   (fn [] (throw (ex-info "global fold reached" {})))}
    (fn []
      (let [row (north.canary-cli/terminal-row-for "lane-canary" "019f-thread")]
        (check "a live canary discovers one committed run through a bounded telemetry query"
               (and (= 1 (count @query-calls))
                    (= [7977 :telemetry] (subvec (first @query-calls) 0 2))
                    (= north.canary-cli/max-terminal-run-candidates
                       (nth (first @query-calls) 3))
                    (= [[7977 subject]] @show-calls)))
        (check "an exact terminal row normalizes its thread and preserves its failure result"
               (and (= subject (:entity row))
                    (= "@019f-thread" (:thread row))
                    (= "ran" (:processOutcome row))
                    (= "unverified" (:deliveryOutcome row))))
        (check "a terminal row is not matched across a different lane"
               (nil? (north.canary-cli/terminal-canary-row
                      subject
                      (north.canary-cli/exact-run-facts 7977 subject)
                      "lane-other" "019f-thread")))))))

(check "an independently verified delivery is full-green, not a North-caused alarm"
       (and (= "full-green"
               (north.canary-cli/derived-canary-outcome
                {:processOutcome "ran" :deliveryOutcome "verified"}))
            (= "failure"
               (north.canary-cli/derived-canary-outcome
                {:processOutcome "ran" :deliveryOutcome "unverified"}))
            (= "failure"
               (north.canary-cli/derived-canary-outcome
                {:processOutcome "died" :deliveryOutcome "reported"}))))

(check "canary run demands exactly one selection mode"
       (and (= {:mode :matrix} (north.canary-cli/parse-run-options ["--matrix"]))
            (= {:mode :target :target "codex-a"}
               (north.canary-cli/parse-run-options ["--target" "codex-a"]))
            ;; No mode at all is refused at selection, never defaulted.
            (refused? #(north.canary-cli/selected-targets
                        nil nil [{:providerTarget "codex-a" :provider "openai"}]))
            (refused? #(north.canary-cli/parse-run-options
                        ["--matrix" "--target" "codex-a"]))
            (refused? #(north.canary-cli/parse-run-options ["--target"]))
            (refused? #(north.canary-cli/parse-run-options ["--window" "3"]))))

(check "canary report demands a positive window"
       (and (= {:json? false :window 100}
               (north.canary-cli/parse-report-options ["--window" "100"]))
            (= {:json? true :window 5}
               (north.canary-cli/parse-report-options ["--window" "5" "--json"]))
            (refused? #(north.canary-cli/parse-report-options ["--window" "0"]))
            (refused? #(north.canary-cli/parse-report-options ["--window"]))))

;; `canary run` starts five managed lanes; it is an orchestrating operation and
;; must be refused at this executable boundary when a managed worker invokes it.
(check "canary run is refused for a managed worker topology"
       (binding [north.topology-authority/*topology* "worker"]
         (refused? #(north.canary-cli/cmd-run ["--matrix"]))))

(check "all canary CLI checks passed" (every? second @checks))
