#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.set :as set]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def trace-cli (str root "/cli/trace-cli.clj"))
(let [caller-file (System/getProperty "babashka.file")]
  (try
    (System/setProperty "north.trace.lib" "1")
    ;; trace-cli resolves its sibling sources from babashka.file when executed.
    ;; Preserve that execution context while loading its pure lifecycle helper.
    (System/setProperty "babashka.file" trace-cli)
    (load-file trace-cli)
    (finally
      (System/clearProperty "north.trace.lib")
      (if caller-file
        (System/setProperty "babashka.file" caller-file)
        (System/clearProperty "babashka.file")))))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(def result
  (proc/shell {:out :string :err :string :continue true
               :extra-env {"NORTH_PORT" "59999"}}
              "bb" trace-cli "load-probe"))
(def output (str (:out result) (:err result)))
(def ok? (and (not (str/includes? output "Unable to resolve symbol"))
              (not (str/includes? output "EOF while reading"))
              (str/includes? output "coordinator :59999 unreachable")))

(check "trace CLI loads before its unavailable-coordinator boundary" ok?)
(when-not ok?
  (println output))
(check "identity rendering keeps model and effort as separate exact fields"
       (= "model=gpt-5.6-luna effort=low"
          (identity-route-detail {"model" "gpt-5.6-luna" "effort" "low"})))

(let [production-files
      ["cli/agent-fact-internal.clj"
       "cli/msg-cli.clj"
       "cli/north-live-feed.clj"
       "cli/trace-cli.clj"]
      sources (mapv #(slurp (str root "/" %)) production-files)
      expected-point-reader
      {"cli/agent-fact-internal.clj"
       "north.lifecycle-projection/raw-point-facts"
       "cli/msg-cli.clj"
       "north.lifecycle-projection/folded-agent-point-facts"
       "cli/north-live-feed.clj"
       "north.lifecycle-projection/folded-agent-point-facts"
       "cli/trace-cli.clj"
       "north.lifecycle-projection/folded-agent-point-facts"}
      forbidden-query-ids
      [":find \"identity_fact\"" ":find \"steer_fact\""
       ":find \"live_route_fact\"" ":find \"trace_identity\""]
      subject-all-predicate-shape
      #":args\s*\[[^\n]*\{:var\s+\"p\"\}\s*\{:var\s+\"r\"\}\]"
      guarded?
      (every?
       (fn [[file source]]
         (and (str/includes? source (get expected-point-reader file))
              (not-any? #(str/includes? source %) forbidden-query-ids)
              (not (re-find subject-all-predicate-shape source))))
       (map vector production-files sources))]
  (check "all managed lifecycle production readers forbid subject-only all-predicate queries"
         guarded?))

(let [expected-agent
      (set/union north.agent-provenance/identity-predicates
                 (set north.agent-provenance/required-identity-predicates)
                 north.agent-provenance/terminal-predicates
                 (set north.terminal-projection/terminal-projection-predicates)
                 #{"identity_manifest_sha256" "terminal_manifest_sha256"})
      expected-run
      (set/union (set north.terminal-projection/run-reservation-predicates)
                 #{"run_bar_evidence"})
      expected-route-guard
      (set/union
       (set north.terminal-projection/terminal-projection-predicates)
       #{"identity_manifest_sha256" "terminal_manifest_sha256"
         "live_input" "live_input_state" "live_input_epoch"})]
  (check "shared lifecycle vocabularies exactly cover canonical agent/run/thread evidence"
         (and (= expected-agent
                 (set north.lifecycle-projection/managed-agent-predicates))
              (= (set/union expected-agent
                            north.lifecycle-projection/trace-session-display-predicates)
                 (set north.lifecycle-projection/trace-agent-predicates))
              (= expected-run
                 (set north.lifecycle-projection/reported-run-predicates))
              (= expected-route-guard
                 (set north.lifecycle-projection/route-guard-predicates))
              (= #{"done_when"}
                 (set north.lifecycle-projection/reported-thread-predicates)))))

(let [calls (atom [])
      raw
      (north.lifecycle-projection/raw-point-facts
       (fn [subject predicate]
         (swap! calls conj [subject predicate])
         (case predicate
           "goal" ["one" "two"]
           "process_outcome" ["ran" "died"]
           []))
       "@agent:raw-probe"
       ["goal" "process_outcome" "terminal_manifest_sha256"])]
  (check "raw lifecycle point reads retain exact conflicting and terminal value sets"
         (and (= [["@agent:raw-probe" "goal"]
                  ["@agent:raw-probe" "process_outcome"]
                  ["@agent:raw-probe" "terminal_manifest_sha256"]]
                 @calls)
              (= #{"one" "two"} (get raw "goal"))
              (= #{"ran" "died"} (get raw "process_outcome"))
              (not (contains? raw "terminal_manifest_sha256")))))

(let [calls (atom [])
      query-called? (atom false)
      facts
      (with-redefs
       [many
        (fn [port subject predicate]
          (swap! calls conj [port subject predicate])
          (case predicate
            "model" ["model-a" "model-b"]
            "process_outcome" ["ran" "died"]
            "repo" ["~/code/north"]
            []))
        send-op
        (fn [& _]
          (reset! query-called? true)
          (throw (ex-info "agent projection must not issue Datalog" {})))]
       (agent-facts "bounded-agent"))]
  (check "trace agent projection is an exact finite set of direct point reads"
         (and (not @query-called?)
              (= (set trace-agent-predicates)
                 (set (map #(nth % 2) @calls)))
              (= (count trace-agent-predicates) (count @calls))
              (every? #(= [PORT "@agent:bounded-agent"] (subvec (vec %) 0 2))
                      @calls)
              (= "~/code/north" (get facts "repo"))
              (contains? (get facts north.agent-provenance/conflict-key #{})
                         "model")
              (= #{"ran" "died"} (get facts "process_outcome")))))

(let [terminal {"outcome" "ran"
                "process_outcome" "ran"
                "delivery_outcome" "unverified"
                "delivery_reason" "provider_terminal_success_without_external_verification"}
      modern (assoc terminal "terminal_manifest_sha256"
                    (north.terminal-projection/terminal-manifest-sha256 terminal))
      partial (dissoc modern "terminal_manifest_sha256")
      folded (reduce-kv north.agent-provenance/fold-fact {} modern)
      conflict (north.agent-provenance/fold-fact folded "process_outcome" "died")
      corrupt-marker (north.agent-provenance/fold-fact
                      folded "terminal_manifest_sha256" "corrupt")
      blocked-terminal {"outcome" "blocked_preflight"
                        "process_outcome" "blocked_preflight"
                        "delivery_outcome" "blocked"
                        "delivery_reason" "execution_preflight_blocked"}
      blocked-modern
      (assoc blocked-terminal "terminal_manifest_sha256"
             (north.terminal-projection/terminal-manifest-sha256
              blocked-terminal))
      blocked-folded
      (reduce-kv north.agent-provenance/fold-fact {} blocked-modern)
      blocked-state (execution-terminal-state "blocked-agent" blocked-folded [] [])
      blocked-delivery (terminal-delivery-state blocked-folded blocked-state)
      ran-state {:outcome "ran" :source :agent :terminal? true
                 :kind :ran :death-notifications 0}
      verdict-base {:id "ran-agent" :on-roster true
                    :terminal-state ran-state :online false :lease nil
                    :lineage :sdk-lane :identity-complete true :deaths []}
      reported-verdict
      (trace-verdict
       (assoc verdict-base :delivery-state
              {:outcome "reported"
               :reason "complete_run_scoped_done_bar_evidence_self_reported"}))
      unverified-verdict
      (trace-verdict
       (assoc verdict-base :delivery-state
              {:outcome "unverified"
               :reason "provider_terminal_success_without_external_verification"}))
      unrecorded-verdict
      (trace-verdict (assoc verdict-base :delivery-state nil))
      blocked-ran-verdict
      (trace-verdict
       (assoc verdict-base :delivery-state
              {:outcome "blocked" :reason "inconsistent_terminal"}))
      inconsistent-ran-verdict
      (trace-verdict
       (assoc verdict-base :delivery-state
              {:outcome "verified" :reason "unsupported_legacy_projection"}))
      online-active-verdict
      (trace-verdict
       {:id "active-agent" :on-roster true
        :terminal-state {:outcome nil :source nil :terminal? false
                         :kind nil :death-notifications 0}
        :delivery-state nil :online true :lease {:exp (+ NOW 60000)}
        :lineage :session :identity-complete false :deaths []})
      online-inconsistent-verdict
      (trace-verdict
       {:id "inconsistent-agent" :on-roster true
        :terminal-state {:outcome nil :source nil :terminal? false
                         :kind nil :resolution-status :indeterminate
                         :resolution-reason :uncommitted-latest-run
                         :death-notifications 0}
        :delivery-state nil :online true :lease {:exp (+ NOW 60000)}
        :lineage :sdk-lane :identity-complete true :deaths []})
      blocked-verdict
      (trace-verdict
       {:id "blocked-agent" :on-roster true
        :terminal-state blocked-state :delivery-state blocked-delivery
        :online true :lease {:exp (+ NOW 60000)}
        :lineage :sdk-lane :identity-complete true :deaths []})
      run-facts (merge terminal
                       {"kind" "run" "agent" "trace-agent"
                        "at" "2026-07-20T09:00:00Z"})
      run [{:subject "@run:trace-agent-terminal" :facts run-facts}]
      death [{:reason "transport exited" :ms 0}]]
  (check "true legacy singleton outcome remains terminal"
         (= {:outcome "ran" :source :agent :terminal? true :kind :ran
             :death-notifications 0}
            (select-keys
             (execution-terminal-state "trace-agent" {"outcome" "ran"} [] [])
             [:outcome :source :terminal? :kind :death-notifications])))
  (check "valid modern terminal resolves from folded multi-cardinality rows"
         (= :ran (:kind (execution-terminal-state "trace-agent" folded [] []))))
  (check "partial modern terminal blocks secondary run fallback"
         (not (:terminal? (execution-terminal-state "trace-agent" partial run []))))
  (check "conflicting process values fail closed"
         (not (:terminal? (execution-terminal-state "trace-agent" conflict run []))))
  (check "conflicting terminal markers fail closed"
         (not (:terminal?
               (execution-terminal-state "trace-agent" corrupt-marker run []))))
  (check "committed run remains fallback only when the lane has no terminal body"
         (= {:outcome "ran" :source :run :terminal? true :kind :ran
             :death-notifications 0}
            (select-keys
             (execution-terminal-state "trace-agent" {} run [])
             [:outcome :source :terminal? :kind :death-notifications])))
  (check "blocked_preflight is a stopped terminal even with a live lease"
         (= {:outcome "blocked_preflight" :source :agent :terminal? true
             :kind :stopped :death-notifications 0}
            (select-keys blocked-state
                         [:outcome :source :terminal? :kind :death-notifications])))
  (check "completion rendering separates process from delivery"
         (= (str "process=blocked_preflight · delivery=blocked "
                 "(execution_preflight_blocked)")
            (terminal-summary blocked-state blocked-delivery)))
  (check "reported delivery is evidence-backed self-report, never independent verification"
         (= (str "execution succeeded; process=ran · delivery=reported "
                 "(complete_run_scoped_done_bar_evidence_self_reported). "
                 "Delivery is evidence-backed same-UID self-report, not independent "
                 "verification; lease lapsed as expected.")
            reported-verdict))
  (check "unverified delivery is yellow-class incomplete proof, not a done claim"
         (and (= :incomplete
                 (delivery-proof-class
                  {:outcome "unverified"}))
              (= (str "execution succeeded but delivery proof is incomplete; "
                      "process=ran · delivery=unverified "
                      "(provider_terminal_success_without_external_verification). "
                      "This is not a done claim; lease lapsed as expected.")
                 unverified-verdict)))
  (check "unrecorded delivery is incomplete proof, not success"
         (= (str "execution succeeded but delivery proof is incomplete; "
                 "process=ran · delivery=unrecorded. "
                 "This is not a done claim; lease lapsed as expected.")
            unrecorded-verdict))
  (check "ran plus blocked delivery is a red-class terminal inconsistency"
         (and (= :blocked (delivery-proof-class {:outcome "blocked"}))
              (= (str "terminal inconsistency; process=ran · delivery=blocked "
                      "(inconsistent_terminal). A ran process with blocked or "
                      "inconsistent delivery is not a done claim.")
                 blocked-ran-verdict)))
  (check "ran plus unsupported delivery state is a red-class inconsistency"
         (and (= :inconsistent
                 (delivery-proof-class {:outcome "verified"}))
              (= (str "terminal inconsistency; process=ran · delivery=verified "
                      "(unsupported_legacy_projection). A ran process with blocked or "
                      "inconsistent delivery is not a done claim.")
                 inconsistent-ran-verdict)))
  (check "online without a terminal remains healthy"
         (= "healthy — online and advancing (no terminal signal yet). No failure."
            online-active-verdict))
  (check "online presence cannot make indeterminate lifecycle evidence healthy"
         (and (str/includes? online-inconsistent-verdict
                             "lifecycle evidence is inconsistent")
              (str/includes? online-inconsistent-verdict
                             "neither active nor finished")
              (not (str/includes? online-inconsistent-verdict "healthy"))))
  (check "terminal blocked_preflight dominates live presence in the verdict"
         (and (str/includes? blocked-verdict
                             "terminal execution did not succeed")
              (str/includes? blocked-verdict "process=blocked_preflight")
              (str/includes? blocked-verdict "delivery=blocked")
              (not (str/includes? blocked-verdict "healthy —"))))
  (check "death notification alone is diagnostic and never terminal"
         (= {:outcome nil :source nil :terminal? false :kind nil
             :death-notifications 1}
            (select-keys
             (execution-terminal-state "trace-agent" {} [] death)
             [:outcome :source :terminal? :kind :death-notifications]))))

(defn ledger-event-fixture
  [{:strs [run thread agent parentRun parentThread coordinator sequence type
           observedAt source coverage payload]}]
  (let [unsigned (cond-> {"version" north.run-ledger/version
                          "run" run "thread" thread "agent" agent
                          "sequence" sequence "type" type
                          "observedAt" observedAt "source" source
                          "coverage" coverage "payload" payload}
                   parentRun (assoc "parentRun" parentRun)
                   parentThread (assoc "parentThread" parentThread)
                   coordinator (assoc "coordinator" coordinator))
        digest (north.run-ledger/sha256 (north.run-ledger/canonical-json unsigned))
        subject (format "%s:event:%08d" run sequence)
        facts (cond-> [["kind" "run_event"]
                       ["agent_run_ledger_version" north.run-ledger/version]
                       ["run" run] ["thread" thread] ["agent" agent]
                       ["run_event_sequence" (str sequence)]
                       ["run_event_type" type]
                       ["run_event_observed_at" observedAt]
                       ["run_event_source" source]
                       ["run_event_coverage" coverage]
                       ["run_event_data" (north.run-ledger/canonical-json payload)]
                       ["run_event_sha256" digest]]
                parentRun (conj ["parent_run" parentRun])
                parentThread (conj ["parent_thread" parentThread])
                coordinator (conj ["run_coordinator" coordinator]))]
    (north.run-ledger/validate-event-facts! subject facts)))

(let [parent-run "@run:parent-ledger"
      child-run "@run:child-ledger"
      thread "@019f89ac-ledger"
      parent-events
      [(ledger-event-fixture
        {"run" parent-run "thread" thread "agent" "parent-agent"
         "coordinator" "root" "sequence" 0 "type" "admission_received"
         "observedAt" "2026-07-22T01:00:00Z" "source" "north-harness"
         "coverage" "exact"
         "payload" {"receiptDigest" (apply str (repeat 64 "a"))}})
       (ledger-event-fixture
        {"run" parent-run "thread" thread "agent" "parent-agent"
         "coordinator" "root" "sequence" 1 "type" "terminal_cleanup"
         "observedAt" "2026-07-22T01:01:00Z" "source" "north-harness"
         "coverage" "exact" "payload" {"outcome" "ran" "cleanupStatus" "complete"}})]
      parent-digest
      (north.run-ledger/sha256
       (north.run-ledger/canonical-json (mapv #(get % "digest") parent-events)))
      parent-header {"kind" "run" "thread" thread "agent" "parent-agent"
                     "run_coordinator" "root" "run_event_count" "2"
                     "run_event_terminal_sequence" "1"
                     "run_event_ledger_sha256" parent-digest}
      child-events
      [(ledger-event-fixture
        {"run" child-run "thread" "@019f89ac-child" "agent" "child-agent"
         "parentRun" parent-run "parentThread" thread "coordinator" "parent-agent"
         "sequence" 0 "type" "terminal_cleanup"
         "observedAt" "2026-07-22T01:00:30Z" "source" "codex-app-server"
         "coverage" "partial" "payload" {"outcome" "ran" "cleanupStatus" "complete"}})]
      child-header {"kind" "run" "thread" "@019f89ac-child" "agent" "child-agent"
                    "parent_run" parent-run "parent_thread" thread
                    "run_coordinator" "parent-agent" "run_event_count" "1"
                    "run_event_terminal_sequence" "0"
                    "run_event_ledger_sha256"
                    (north.run-ledger/sha256
                     (north.run-ledger/canonical-json (mapv #(get % "digest") child-events)))}
      parent-timeline (forensic-run parent-run parent-header parent-events)
      child-timeline (forensic-run child-run child-header child-events)
      parent-rendered (render-forensic-run parent-timeline)
      child-rendered (render-forensic-run child-timeline)]
  (check "forensic trace validates ordered finalized event/header evidence"
         (and (:valid-order? parent-timeline) (:finalized? parent-timeline)
              (:header-count-valid? parent-timeline)
              (:header-digest-valid? parent-timeline)))
  (check "forensic trace reconstructs exact parent run/thread/coordinator lineage"
         (and (= parent-run (:parent-run child-timeline))
              (= thread (:parent-thread child-timeline))
              (= "parent-agent" (:coordinator child-timeline))
              (str/includes? child-rendered (str "parent run: " parent-run))))
  (check "forensic trace labels every absent observation unknown with source coverage"
         (and (str/includes? parent-rendered
                             "provider_routed        unknown (source coverage unavailable)")
              (str/includes? child-rendered
                             "usage_observed         unknown (source coverage unavailable)")
              (= (count north.run-ledger/event-types)
                 (count (:observations child-timeline)))))
  (check "forensic trace preserves explicit partial event source coverage"
         (str/includes? child-rendered
                        "terminal_cleanup       observed coverage=partial source=codex-app-server")))

(doseq [[label passed?] @checks]
  (println (format "  [%s] %s" (if passed? "PASS" "FAIL") label)))
(let [passed (count (filter second @checks))]
  (println (format "\ntrace CLI lifecycle: %d / %d PASS" passed (count @checks)))
  (System/exit (if (= passed (count @checks)) 0 1)))
