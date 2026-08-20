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
              (str root "/bin/north") "trace" "load-probe"))
(def output (str (:out result) (:err result)))
(def ok? (and (not (str/includes? output "Unable to resolve symbol"))
              (not (str/includes? output "EOF while reading"))
              (str/includes? output "Beagle Store server :59999 unreachable")))

(check "trace CLI loads before its unavailable-Beagle Store-server boundary" ok?)
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
      [":find \"identity_fact\"" ":find \"msg_fact\""
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
         "live_input" "live_input_state" "live_input_epoch"
         "shadow_reviewer_note_capability_sha256"})]
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
       [north.coord/show-many-in-domain
        (fn [port domain subjects]
          (swap! calls conj [port domain subjects])
          {:version 17
           :rows
           {"@agent:bounded-agent"
            [["model" "model-a"]
             ["model" "model-b"]
             ["process_outcome" "ran"]
             ["process_outcome" "died"]
             ["repo" "~/code/north"]]}})
        north.coord/bounded-query
        (fn [& _]
          (reset! query-called? true)
          (throw (ex-info "agent projection must not issue Datalog" {})))]
       (agent-facts "bounded-agent"))]
  (check "trace agent projection is one exact subject batch"
         (and (not @query-called?)
              (= [[PORT :coordination ["@agent:bounded-agent"]]] @calls)
              (= "~/code/north" (get facts "repo"))
              (contains? (get facts north.agent-provenance/conflict-key #{})
                         "model")
              (= #{"ran" "died"} (get facts "process_outcome")))))

(let [terminal {"process_outcome" "ran"
                "delivery_outcome" "unverified"
                "delivery_reason" "provider_terminal_success_without_external_verification"}
      modern (assoc terminal "terminal_manifest_sha256"
                    (north.terminal-projection/terminal-manifest-sha256 terminal))
      partial (dissoc modern "terminal_manifest_sha256")
      folded (reduce-kv north.agent-provenance/fold-fact {} modern)
      conflict (north.agent-provenance/fold-fact folded "process_outcome" "died")
      corrupt-marker (north.agent-provenance/fold-fact
                      folded "terminal_manifest_sha256" "corrupt")
      blocked-terminal {"process_outcome" "blocked_preflight"
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
  (check "outcome-only historical terminal remains unresolved"
         (not (:terminal? (execution-terminal-state "trace-agent" {"outcome" "ran"} [] []))))
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
                 "verification; presence is inactive as expected.")
            reported-verdict))
  (check "unverified delivery is yellow-class incomplete proof, not a done claim"
         (and (= :incomplete
                 (delivery-proof-class
                  {:outcome "unverified"}))
              (= (str "execution succeeded but delivery proof is incomplete; "
                      "process=ran · delivery=unverified "
                      "(provider_terminal_success_without_external_verification). "
                      "This is not a done claim; presence is inactive as expected.")
                 unverified-verdict)))
  (check "unrecorded delivery is incomplete proof, not success"
         (= (str "execution succeeded but delivery proof is incomplete; "
                 "process=ran · delivery=unrecorded. "
                 "This is not a done claim; presence is inactive as expected.")
            unrecorded-verdict))
  (check "ran plus blocked delivery is a red-class terminal inconsistency"
         (and (= :blocked (delivery-proof-class {:outcome "blocked"}))
              (= (str "terminal inconsistency; process=ran · delivery=blocked "
                      "(inconsistent_terminal). A ran process with blocked or "
                      "inconsistent delivery is not a done claim.")
                 blocked-ran-verdict)))
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
  [{:strs [run thread agent parentRun parentThread coordinator sequence kind at payload]}]
  (let [event (merge {"version" north.run-ledger/wire-version
                      "id" (str "event:trace:" sequence)
                      "runId" run
                      "sequence" sequence
                      "at" at
                      "kind" kind
                      "essential" true
                      "requiredSemantics" ["north.event-order.v1"
                                           "north.tool-terminal.v1"
                                           "north.usage-split.v1"]}
                     payload
                     (when parentRun {"parentRunId" parentRun}))
        raw (north.run-ledger/canonical-json event)
        digest (north.run-ledger/sha256 raw)
        subject (north.run-ledger/event-subject run sequence)
        facts (cond-> [["kind" "wire_event"]
                       ["wire_ledger_version" north.run-ledger/version]
                       ["wire_version" north.run-ledger/wire-version]
                       ["wire_run_id" run] ["thread" thread] ["agent" agent]
                       ["wire_event_id" (get event "id")]
                       ["wire_event_sequence" (str sequence)]
                       ["wire_event_at" at]
                       ["wire_event_kind" kind]
                       ["wire_event_essential" "true"]
                       ["wire_event_json" raw]
                       ["wire_event_sha256" digest]]
                parentThread (conj ["parent_thread" parentThread])
                coordinator (conj ["run_coordinator" coordinator]))]
    (north.run-ledger/validate-event-facts! subject facts)))

(let [parent-run "run:parent-ledger"
      child-run "run:child-ledger"
      thread "@019f89ac-ledger"
      parent-events
      [(ledger-event-fixture
        {"run" parent-run "thread" thread "agent" "parent-agent"
         "coordinator" "root" "sequence" 0 "kind" "run.started"
         "at" "2026-07-22T01:00:00.000Z"
         "payload" {"lifecycle" "running"}})
       (ledger-event-fixture
        {"run" parent-run "thread" thread "agent" "parent-agent"
         "coordinator" "root" "sequence" 1 "kind" "run.terminated"
         "at" "2026-07-22T01:01:00.000Z"
         "payload" {"lifecycle" "completed" "reason" {"code" "completed"}}})]
      parent-digest (north.run-ledger/ledger-digest parent-events)
      parent-header {"kind" "run" "thread" thread "agent" "parent-agent"
                     "run_coordinator" "root" "wire_event_count" "2"
                     "wire_event_last_sequence" "1"
                     "wire_ledger_sha256" parent-digest}
      child-events
      [(ledger-event-fixture
        {"run" child-run "thread" "@019f89ac-child" "agent" "child-agent"
         "parentRun" parent-run "parentThread" thread "coordinator" "parent-agent"
         "sequence" 0 "kind" "run.started" "at" "2026-07-22T01:00:30.000Z"
         "payload" {"lifecycle" "running"}})
       (ledger-event-fixture
        {"run" child-run "thread" "@019f89ac-child" "agent" "child-agent"
         "parentThread" thread "coordinator" "parent-agent"
         "sequence" 1 "kind" "run.terminated" "at" "2026-07-22T01:00:31.000Z"
         "payload" {"lifecycle" "completed" "reason" {"code" "completed"}}})]
      child-header {"kind" "run" "thread" "@019f89ac-child" "agent" "child-agent"
                    "parent_run" (str "@" parent-run) "parent_thread" thread
                    "run_coordinator" "parent-agent" "wire_event_count" "2"
                    "wire_event_last_sequence" "1"
                    "wire_ledger_sha256" (north.run-ledger/ledger-digest child-events)}
      parent-timeline (forensic-run parent-run parent-header parent-events)
      child-timeline (forensic-run child-run child-header child-events)
      parent-rendered (render-forensic-run parent-timeline)
      child-rendered (render-forensic-run child-timeline)]
  (check "forensic trace validates ordered finalized event/header evidence"
         (and (:valid-order? parent-timeline) (:finalized? parent-timeline)
              (= parent-digest (:digest parent-timeline))))
  (check "forensic trace reconstructs exact parent run/thread/coordinator lineage"
         (and (= parent-run (get-in child-events [0 "event" "parentRunId"]))
              (= thread (:parent-thread child-timeline))
              (= "parent-agent" (:coordinator child-timeline))
              (str/includes? child-rendered "parent thread: @019f89ac-ledger")))
  (check "forensic trace renders the exact wire kinds and identities"
         (and (str/includes? parent-rendered "run.started")
              (str/includes? parent-rendered "run.terminated")
              (str/includes? child-rendered "event:trace:1")))
  (check "forensic trace compares the durable ledger digest to its run header"
         (str/includes? child-rendered "header digest: consistent")))

(doseq [[label passed?] @checks]
  (println (format "  [%s] %s" (if passed? "PASS" "FAIL") label)))
(let [passed (count (filter second @checks))]
  (println (format "\ntrace CLI lifecycle: %d / %d PASS" passed (count @checks)))
  (System/exit (if (= passed (count @checks)) 0 1)))
