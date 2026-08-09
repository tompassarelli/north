#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn pin-evidence-json [pins]
  (let [issued (java.time.Instant/now)]
    (json/generate-string
     {:policyVersion "north-routing-pin-v1"
      :issuedAt (str issued)
      :expiresAt (str (.plusSeconds issued 3600))
      :reasonCode "explicit-human-request"
      :detail "agents CLI fixture"
      :pins pins})))
(defn economy-assessment-json []
  (json/generate-string
   {:version "minimum-sufficient-v1"
    :signals {:decisionOwnership "none"
              :seamScope "none"
              :errorExposure "contained-reversible"
              :oracleStrength "objective-local"
              :foundationalImpact "none"
              :dependencyShape "atomic-cohesive"
              :reasoningShape "deterministic"}
    :derived {:minimumTier "economy"
              :minimumReasoning "low"
              :ruleCodes ["reasoning-shape:deterministic"]}
    :selected {:tier "economy" :reasoning "low"}}))

(let [scratch (.toFile (java.nio.file.Files/createTempDirectory
                        "north-watch-test-"
                        (make-array java.nio.file.attribute.FileAttribute 0)))
      stream-dir (io/file scratch "streams")
      control-dir (io/file scratch "control")
      id "lane-watch-probe-1234"
      stream-file (io/file stream-dir (str "agent-" id ".stream.jsonl"))
      control-file (io/file control-dir (str id ".log"))]
  (.mkdirs stream-dir)
  (.mkdirs control-dir)
  (spit stream-file "{\"type\":\"assistant\",\"message\":\"working\"}\n")
  ;; The production failure this guards against had a healthy canonical stream
  ;; while its process/control log was effectively silent.
  (spit control-file "")
  (let [plan (watch-plan [id] (str stream-dir) (str control-dir))
        rendered (str/join "\n" (watch-status-lines plan))]
    (check "watch defaults to the active canonical stream when the control log is silent"
           (and (= :stream (:mode plan))
                (= (str stream-file) (get-in plan [:stream :path]))
                (pos? (get-in plan [:stream :bytes]))
                (zero? (get-in plan [:control :bytes]))
                (str/includes? rendered "watch target: canonical SDK event stream")
                (str/includes? rendered "event data present")
                (str/includes? rendered "present but empty")
                (str/includes? rendered "silence is not evidence that a worker stalled or died"))))
  (let [plan (watch-plan [id "--control"] (str stream-dir) (str control-dir))]
    (check "control-log following requires explicit opt-in and retains both paths"
           (and (= :control (:mode plan))
                (= (str stream-file) (get-in plan [:stream :path]))
                (= (str control-file) (get-in plan [:control :path])))))
  (check "watch rejects path traversal and extra arguments before resolving files"
         (and (:error (watch-plan ["../lane"] (str stream-dir) (str control-dir)))
              (:error (watch-plan [id "--control" "extra"]
                                  (str stream-dir) (str control-dir))))))

(defn managed [facts]
  (let [base (merge {"kind" "lane" "goal" "fixture" "repo" "~/code/north"
                     "spawned_at" "2026-07-17T00:00:00Z"
                     "display_handle" "fixture" "display_name" "fixture"
                     "provider_target" (get facts "provider")
                     "live_input" (if (= "anthropic" (get facts "provider"))
                                    "streaming"
                                    "unsupported")
                     "live_input_state" (if (= "anthropic" (get facts "provider"))
                                          "armed"
                                          "frozen")
                     "live_input_epoch" "00000000-0000-4000-8000-000000000010"}
                    facts)]
    (assoc base "identity_manifest_sha256"
           (north.agent-provenance/manifest-sha256 base))))
(defn marked-terminal
  ([facts] (marked-terminal facts "ran" "unverified"))
  ([facts process delivery]
   (let [terminal {"outcome" process
                   "process_outcome" process
                   "delivery_outcome" delivery
                   "delivery_reason" (if (= delivery "unverified")
                                       "provider_terminal_success_without_external_verification"
                                       "execution_did_not_reach_success_terminal")}]
     (merge facts terminal
            {"terminal_manifest_sha256"
             (north.terminal-projection/terminal-manifest-sha256 terminal)}))))
(defn fold-observed [facts]
  (reduce-kv north.agent-provenance/fold-fact {} facts))

(let [fixtures (json/parse-string
                (slurp (io/file root "sdk/test/fixtures/agent-roster-contract.json")))]
  (check
   "shared roster fixtures preserve semantic identity in the CLI projection"
   (every?
    true?
    (for [{:strs [name id facts expected]} fixtures
          :let [line (agent-primary-line {:online true} facts)
                axes (str (get expected "providerLabel")
                          " · " (get expected "modelDisplay")
                          " · " (get expected "effortDisplay")
                          " · " (get expected "orchestrationProvenance"))]]
      (and (= (get expected "orchestrationProvenance") (orchestration-provenance facts))
           (= (get expected "semanticHandle") (semantic-handle id facts))
           (= (get expected "primaryLine") line)
           (str/starts-with? line axes)
           (not= line (get facts "display_name"))
           ;; Display text is output only. It cannot repair missing structured axes.
           (or (not= name "missing-managed-axes")
               (and (str/includes? line "orchestration:legacy-debt")
                    (not (str/includes? line "designer")))))))))

(let [native (first (json/parse-string
                     (slurp (io/file root "sdk/test/fixtures/agent-roster-contract.json"))))
      cases (get native "observationCases")]
  (check
   "native observation conflicts are deterministic and match the shared roster golden"
   (every?
    true?
    (for [{:strs [observations expected]} cases
          :let [facts (reduce (fn [acc [predicate value]]
                                (north.agent-provenance/fold-fact acc predicate value))
                              {} observations)
                line (agent-primary-line {:online true} facts)]]
      (and (= (get expected "providerLabel") (provider-target-label facts))
           (= (get expected "orchestrationProvenance") (orchestration-provenance facts))
           (= (get expected "semanticHandle")
              (semantic-handle "session-native-7841e6b2" facts))
           (= (get expected "primaryLine") line))))))

(check "session current task outranks stale lane goal and stored presentation receipts"
       (= "anthropic:ambient · opus · xhigh · orchestration:designer · working: Current session task"
          (agent-primary-line
           {:online true}
           (managed
            {"kind" "lane" "provider" "anthropic" "model" "opus" "effort" "xhigh"
             "composition_kind" "preset" "role" "designer" "composition_id" "designer"
             "composition_overrides" "[]" "goal" "Stale lane goal"
             "display_name" "STALE presentation receipt"})
           {"current_thread" "Current session task"})))

(check "preset roster line uses canonical structured axes"
       (= "anthropic:ambient · opus · xhigh · orchestration:designer · working: build the roster"
          (agent-primary-line {:online true}
                              (managed
                               {"kind" "lane" "provider" "anthropic" "model" "claude-opus-4-8"
                                "effort" "xhigh" "composition_kind" "preset"
                                "role" "designer" "composition_id" "designer"
                                "composition_overrides" "[]" "goal" "build the roster"}))))

(check "bespoke provenance is explicit"
       (str/includes?
        (agent-primary-line {:online true}
                            (managed
                             {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                              "effort" "high" "composition_kind" "bespoke"
                              "role" "migration-forensics" "composition_id" "migration-forensics"
                              "bespoke_reason" "one-off provenance analysis"
                              "promotion_candidate" "false"
                              "composition_contract_sha256" (apply str (repeat 64 "a"))
                              "composition_contract_fingerprint_version" "v1"
                              "composition_contract_fingerprint_domain" "north:bespoke-contract:v1"
                              "goal" "trace schema"}))
        "orchestration:bespoke:migration-forensics"))

(check "account target and Orchestration template are first-class in the roster"
       (= "openai:codex-work · sol · high · orchestration:designer · working: trace schema"
          (agent-primary-line {:online true}
                              (managed
                               {"kind" "lane" "provider" "openai" "provider_target" "codex-work"
                                "model" "gpt-5.6-sol" "effort" "high" "composition_kind" "preset"
                                "role" "designer" "composition_id" "designer"
                                "composition_overrides" "[]" "goal" "trace schema"}))))

(check "preset overrides are a compact projection of structured facts"
       (= "openai:ambient · sol · xhigh · orchestration:integrator+override(tier,reasoning) · working: cross-seam repair"
          (agent-primary-line {:online true}
                              (managed
                               {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                                "effort" "xhigh" "composition_kind" "preset"
                                "role" "integrator" "composition_id" "integrator"
                                "composition_overrides" "[\"tier\",\"reasoning\"]"
                                "composition_override_reason" "high leverage seam"
                                "goal" "cross-seam repair"}))))

(check "malformed managed override provenance is explicit legacy debt"
       (str/includes?
        (agent-primary-line {:online true}
                            (managed
                             {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                              "effort" "xhigh" "composition_kind" "preset"
                              "role" "integrator" "composition_id" "integrator"
                              "composition_overrides" "[\"tier\"]"}))
        "orchestration:legacy-debt"))

(check "default managed target is displayed as ambient"
       (str/starts-with?
        (agent-primary-line {:online true}
                            (managed
                             {"kind" "lane" "provider" "anthropic" "provider_target" "anthropic"
                              "model" "opus" "effort" "high" "composition_kind" "preset"
                              "role" "integrator" "composition_id" "integrator"
                              "composition_overrides" "[]"}))
        "anthropic:ambient · opus · high · orchestration:integrator"))

(check "historical native gaps are explicit provenance labels, never model names"
       (= "provider:historical-unrecorded · model:historical-unrecorded · effort:historical-unrecorded · orchestration:not-selected · working: unknown"
          (agent-primary-line {:online true :focus "CONTEXT BRIEF:"} {"kind" "session"})))

(check "native session with a repo has an honest useful activity fallback"
       (str/ends-with?
        (agent-primary-line {:online true} {"kind" "session" "provider" "openai"
                                            "model" "gpt-5.6-sol" "effort" "unobserved"
                                            "repo" "north"})
        "working: native session in north"))

(check "new native missing effort is an observation boundary, not a preset failure"
       (str/includes?
        (agent-primary-line {:online true} {"kind" "session" "provider" "openai"
                                            "model" "gpt-5.6-sol" "effort" "unobserved"})
        "openai · sol · effort:unobserved · orchestration:not-selected"))

(check "roster lifecycle categories do not call terminal TTL rows active"
       (and (= :active-agent (roster-category {"kind" "lane"}))
            (= :native-session (roster-category {"kind" "session"}))
            (= :recently-finished (roster-category {"kind" "lane" "outcome" "ran"}))
            (= :inconsistent
               (roster-category {"kind" "lane" "process_outcome" "ran" "outcome" "ran"}))
            (= :recently-finished
               (roster-category (marked-terminal {"kind" "lane"})))
            (= :unclassified (roster-category {}))))

(let [control "lane-run-terminal"
      terminal (dissoc (marked-terminal {}) "terminal_manifest_sha256")
      committed-run
      {:subject "@run:lane-run-terminal"
       :facts (merge terminal
                     {"kind" "run" "agent" control
                      "at" "2026-07-20T09:00:00Z"})}
      no-run
      (attach-lane-resolutions
       [control] {control {"kind" "lane"}}
       {:ok true :by-agent {control []}})
      resolved
      (attach-lane-resolutions
       [control] {control {"kind" "lane"}}
       {:ok true :by-agent {control [committed-run]}})
      malformed
      (attach-lane-resolutions
       [control] {control {"kind" "lane"}}
       {:ok true
        :by-agent
        {control [(update committed-run :facts dissoc "kind")]}})]
  (check "canonical run resolution controls roster active, finished, and inconsistent categories"
         (and (= :active-agent (roster-category (get no-run control)))
              (= :recently-finished (roster-category (get resolved control)))
              (= :inconsistent (roster-category (get malformed control)))
              (= "finished"
                 (get (roster-json-row {:id control :online true :expires-s 10}
                                       (get resolved control) {})
                      "state"))
              (= "inconsistent"
                 (get (roster-json-row {:id control :online true :expires-s 10}
                                       (get malformed control) {})
                      "lifecycle")))))

(let [first-subject "@run:lane-first-run-2"
      second-subject "@run:lane-second-run-1"
      first-run-facts
      (merge (marked-terminal {}) {"agent" "lane-first" "kind" "run"
                                    "at" "2026-07-20T08:00:00Z"})
      second-run-facts
      (merge (marked-terminal {}) {"agent" "lane-second" "kind" "run"
                                    "at" "2026-07-20T09:00:00Z"})
      query-calls (atom [])
      show-calls (atom [])
      subject-by-control {"lane-first" first-subject "lane-second" second-subject}
      projection
      (with-redefs
       [north.coord/bounded-query-in-domain
        (fn [port domain query limit]
          (swap! query-calls conj [port domain query limit])
          (let [requested-controls
                (into #{}
                      (map #(get-in % [:body 0 :args 2]))
                      (:rules query))]
            {:rows (mapv vector
                         (keep #(get subject-by-control %) requested-controls))
             :served-version 1}))
        north.coord/show-many-in-domain
        (fn [port domain subjects]
          (swap! show-calls conj [port domain subjects])
          {:version 1
           :rows
           (into {}
                 (map (fn [subject]
                        [subject
                         (mapv vec
                               (if (= subject first-subject)
                                 first-run-facts
                                 second-run-facts))]))
                 subjects)})]
       (roster-run-entries ["lane-first" "lane-second"]))
      resolutions
      (attach-lane-resolutions
       ["lane-first" "lane-second"]
       {"lane-first" {"kind" "lane"} "lane-second" {"kind" "lane"}}
       projection)]
  (check "roster-run-entries resolves the full live batch"
         (and (:ok projection)
              (= :resolved
                 (get-in resolutions ["lane-first" lane-resolution-key :status]))
              (= :resolved
                 (get-in resolutions ["lane-second" lane-resolution-key :status]))))
  (check "roster run resolution performs one bounded query and one batched subject read"
         (and (= 1 (count @query-calls))
              (= [7977 :telemetry max-roster-run-candidates]
                 (let [[port domain _query limit] (first @query-calls)]
                   [port domain limit]))
              (= [[7977 :telemetry [first-subject second-subject]]] @show-calls))))

(check "terminal roster state separates process exit from delivery truth"
       (and (str/includes?
             (agent-primary-line
              {:online true}
              (marked-terminal {"kind" "lane" "goal" "attempt delivery"}))
             "finished(process:ran, delivery:unverified)")
            (str/includes?
             (agent-primary-line {:online true}
                                 {"kind" "lane" "outcome" "ran" "goal" "legacy"})
             "finished(process:ran, delivery:unrecorded)")))

(check "legacy same-UID verified projection is lifecycle-inconsistent, never finished or active"
       (let [evidence (json/generate-string
                       {"version" "north:done-bars:v1"
                        "run" "@run-worker"
                        "thread" "@thread"
                        "reporter" "@agent:worker"
                        "capturedAt" "2026-07-18T10:00:00Z"
                        "baselineEvidenceSha256"
                        (north.terminal-projection/sha256 "[]")
                        "doneWhen" ["tests pass"]
                        "matches" [{"bar" "tests pass"
                                    "evidence" ["tests pass → exit 0"]}]})
             evidence-hash (north.terminal-projection/sha256 evidence)
             attestation (json/generate-string
                          {"version" "north:delivery-attestation:v1"
                           "target" "@agent:worker"
                           "run" "@run-worker"
                           "thread" "@thread"
                           "evidenceSha256" evidence-hash
                           "actor" "@agent:verifier"
                           "role" "verifier"
                           "authority" "managed-independent-verifier"
                           "attestedAt" "2026-07-18T10:01:00Z"})
             terminal {"outcome" "ran" "process_outcome" "ran"
                       "delivery_outcome" "verified"
                       "delivery_reason" "independent_managed_verifier_attested"
                       "delivery_evidence" evidence
                       "delivery_evidence_sha256" evidence-hash
                       "delivery_attestation" attestation
                       "delivery_attestation_sha256"
                       (north.terminal-projection/sha256 attestation)}
             facts (merge {"kind" "lane" "goal" "verified delivery"}
                          terminal
                          {"terminal_manifest_sha256"
                           (north.terminal-projection/terminal-manifest-sha256 terminal)})]
         (let [line (agent-primary-line {:online true} facts)]
           (and (not (str/includes? line "delivery:verified"))
                (str/includes? line "inconsistent(lifecycle:invalid-lane-terminal)")))))

(check "folded terminal conflicts stay visible and cannot manufacture a finished lane"
       (let [committed (fold-observed
                        (marked-terminal {"kind" "lane" "goal" "conflict probe"}))
             process-conflict (north.agent-provenance/fold-fact
                               committed "process_outcome" "died")
             marker-conflict (north.agent-provenance/fold-fact
                              committed "terminal_manifest_sha256" "corrupt")]
         (and (= #{"ran"} (get committed "process_outcome"))
              (= :recently-finished (roster-category committed))
              (str/includes? (agent-primary-line {:online true} committed)
                             "finished(process:ran, delivery:unverified)")
              (= #{"ran" "died"} (get process-conflict "process_outcome"))
              (= :inconsistent (roster-category process-conflict))
              (str/includes? (agent-primary-line {:online true} process-conflict)
                             " · inconsistent(lifecycle:invalid-lane-terminal): conflict probe")
              (= :inconsistent (roster-category marker-conflict)))))

(check "uncomposed role remains visible without inventing Orchestration provenance"
       (let [facts {"kind" "lane" "provider" "anthropic" "model" "opus"
                    "effort" "xhigh"
                    "role" "orchestrator" "goal" "coordinate work"}]
         (and (str/includes? (agent-primary-line {:online true} facts)
                             "orchestration:legacy-debt · role:orchestrator")
              (= "anthropic-opus-xhigh-orchestration-legacy-debt-legacy"
                 (semantic-handle "lane-legacy" facts)))))

(check "only provider-native sessions receive the native handle segment"
       (= "openai-sol-unobserved-orchestration-not-selected-native"
          (semantic-handle "session-native"
                           {"kind" "session" "provider" "openai" "model" "gpt-5.6-sol"
                            "effort" "unobserved"})))

(check "composition_kind=none never manufactures native provenance"
       (and (str/includes?
             (agent-primary-line {:online true}
                                 {"kind" "lane" "composition_kind" "none"})
             "orchestration:legacy-debt")
            (str/includes?
             (agent-primary-line {:online true}
                                 {"kind" "session" "composition_kind" "none"})
             "orchestration:not-selected")))

(check "display labels are never reverse-parsed into missing structured facts"
       (let [facts {"kind" "lane" "display_name" "anthropic opus xhigh designer"}]
         (and (str/starts-with? (agent-primary-line {:online true} facts)
                                "unknown · unknown · unknown · orchestration:legacy-debt")
              (str/starts-with? (semantic-handle "sdk-a205e9ce" facts)
                                "unknown-unknown-unknown-orchestration-legacy-debt-"))))

(check "current structured effort overrides a stale stored handle"
       (= "openai-ambient-sol-xhigh-orchestration-designer-a205e9ce"
          (semantic-handle "sdk-a205e9ce"
                           (managed
                            {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                             "effort" "xhigh" "composition_kind" "preset"
                             "role" "designer" "composition_id" "designer"
                             "composition_overrides" "[]"
                             "display_handle" "openai-sol-high-designer-a205e9ce"}))))

(check "dry-run route: Anthropic frontier resolves to the Orchestration config model, no Fable window swap"
       (let [route (dry-resolved-route "anthropic" "frontier" nil nil)]
         (and (= "anthropic" (:provider route))
              (not= "fable" (:model route)))))

(let [calls (atom [])
      facts (with-redefs [north.coord/show-rows
                          (fn [port subject]
                            (swap! calls conj [port subject])
                            [["provider" "openai"]
                             ["model" "gpt-5.6-sol"]])]
              (agent-facts-one "sdk-current"))]
  (check "one-agent identity reads use the exact-subject FRAMRPC facade"
         (and (= {"provider" "openai" "model" "gpt-5.6-sol"}
                 (select-keys facts ["provider" "model"]))
              (= [[7977 "@agent:sdk-current"]] @calls))))

(let [calls (atom [])
      failed
      (with-redefs [north.coord/show-many-in-domain
                    (fn [port domain subjects]
                      (swap! calls conj [port domain subjects])
                      (throw (ex-info "coordination unavailable" {})))]
        (roster-facts
         (mapv #(str "lane-" %) (range max-live-controls))))
      valid-empty
      (with-redefs [north.coord/show-many-in-domain
                    (fn [& _] {:version 1 :rows {}})]
        (roster-facts ["lane-a"]))
      malformed
      (with-redefs [north.coord/show-many-in-domain
                    (fn [& _] {:version 1
                               :rows {"@agent:lane-a" [["task"]]}})]
        (roster-facts ["lane-a"]))]
  (check "coordination roster failure performs one batched call"
         (and (= 1 (count @calls))
              (= "agent subject projection unavailable" (:err failed))))
  (check "live roster identity reads only coordination-owned @agent subjects"
         (let [[port domain subjects] (first @calls)]
           (and (= 7977 port)
                (= :coordination domain)
                (= max-live-controls (count subjects))
                (every? #(str/starts-with? % "@agent:") subjects)
                (not-any? #(str/starts-with? % "@session:") subjects))))
  (check "a successful empty bulk projection remains distinguishable from failure"
         (= {:agents {} :sessions {}} valid-empty))
  (check "malformed bulk subject rows fail closed"
         (= "agent subject projection was malformed" (:err malformed))))

(let [run-ids (atom [])
      out (with-redefs [presence-rows (fn [] {:agents [{:id "lane-active" :online true :expires "10s" :expires-s 10}
                                                        {:id "session-active" :online true :expires "20s" :expires-s 20}
                                                        {:id "lane-done" :online true :expires "30s" :expires-s 30}]})
                        roster-facts
                        (fn [_]
                          {:agents
                           {"lane-active" {"kind" "lane" "provider" "openai"
                                           "model" "gpt-5.6-sol" "effort" "high"
                                           "role" "integrator" "composition_kind" "preset"
                                           "composition_id" "integrator" "composition_overrides" "[]"}
                            "session-active" {"kind" "session" "provider" "anthropic"
                                              "model" "claude-opus-4-8" "effort" "xhigh"}
                            "lane-done" {"kind" "lane" "provider" "openai"
                                         "model" "gpt-5.6-sol" "effort" "high"
                                         "role" "designer" "composition_kind" "preset"
                                         "composition_id" "designer" "composition_overrides" "[]"
                                         "outcome" "ran" "process_outcome" "ran"
                                         "delivery_outcome" "unverified"
                                         "delivery_reason" "provider_terminal_success_without_external_verification"
                                         "terminal_manifest_sha256"
                                         (north.terminal-projection/terminal-manifest-sha256
                                          {"outcome" "ran"
                                           "process_outcome" "ran"
                                           "delivery_outcome" "unverified"
                                           "delivery_reason" "provider_terminal_success_without_external_verification"})}}
                           :sessions {}})
                        roster-run-entries
                        (fn [ids]
                          (reset! run-ids ids)
                          {:ok true
                           :by-agent (into {} (map #(vector % []) ids))})]
            (with-out-str (cmd-agents [])))]
  (check "roster summary separates active and recently finished counts"
         (and (str/includes? out "3 roster entries · 2 active · 0 inconsistent · 1 recently finished")
              (str/includes? out "active agents (1)")
              (str/includes? out "native sessions (1)")
              (str/includes? out "recently finished (1)")
              (str/includes? out "finished(process:ran, delivery:unverified)")
              (not (str/includes? out "live agents"))))
  (check "native live sessions never depend on run telemetry for roster identity"
         (= ["lane-active" "lane-done"] @run-ids))
  (check "ordinary roster output hides the internal presence probe"
         (not (str/includes? out "presence-cli.clj"))))

(let [out (with-redefs [presence-rows (fn [] {:agents []})
                        roster-facts (fn [_] {:agents {} :sessions {}})]
            (with-out-str (cmd-agents ["--verbose"])))]
  (check "verbose roster output names the FRAMRPC presence projection"
         (str/includes? out "FRAMRPC presence projection :7977")))

(let [exp (+ (System/currentTimeMillis) 8000)
      calls (atom [])
      valid (with-redefs [north.coord/online-session-leases
                          (fn [port now]
                            (swap! calls conj [port now])
                            [{:handle "lane-a" :exp exp}])]
              (presence-rows))
      malformed
      (with-redefs [north.coord/online-session-leases
                    (fn [& _] [{:handle "../lane" :exp exp}])]
        (presence-rows))]
  (check "roster intake uses one canonical batched session-lease read"
         (and (= 1 (count @calls))
              (= 7977 (ffirst @calls))
              (integer? (second (first @calls)))))
  (check "roster intake projects the checked live session"
         (let [row (first (:agents valid))]
           (and (= "lane-a" (:id row))
                (:online row)
                (<= 6 (:expires-s row) 8))))
  (check "missing batched lease status fails closed"
         (= "presence projection was malformed" (:err malformed))))

(let [help (proc/shell {:out :string :err :string :continue true
                        :extra-env {"NO_COLOR" "1"}}
                       (str root "/bin/north") "agents" "--help")
      unknown (proc/shell {:out :string :err :string :continue true
                           :extra-env {"NO_COLOR" "1"}}
                          (str root "/bin/north") "agents" "--bogus")]
  (check "agents help documents the versioned JSON mode"
         (and (zero? (:exit help))
              (str/includes? (:out help) "north:agent-roster:v1")))
  (check "agents rejects unknown options without probing presence"
         (and (= 1 (:exit unknown))
              (str/includes? (:err unknown) "unknown option --bogus")
              (not (str/includes? (str (:out unknown) (:err unknown))
                                  "presence-cli.clj")))))

(let [msg (proc/shell {:out :string :err :string :continue true
                         :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                        (str root "/bin/north") "msg"
                        "probe-agent" "probe-message" "--dry-run")]
  (check "msg remains parseable and keeps the internal control key"
         (and (zero? (:exit msg))
              (str/includes? (:out msg) "send north-cli probe-agent msg probe-message")
              (str/includes? (:out msg)
                             "[dry-run] not sent; target capability and liveness were not checked."))))

(let [observed-timeout (atom nil)
      committed-writes (atom 0)
      output
      (with-redefs [run
                    (fn [_argv & {:keys [timeout]}]
                      ;; Model independent coordinator writers finishing before
                      ;; the raw producer reports its successful commit.
                      (let [writers (doall (repeatedly 32
                                                       #(future
                                                          (swap! committed-writes inc))))]
                        (doseq [writer writers] @writer))
                      (reset! observed-timeout timeout)
                      {:ok true :exit 0 :out "queued for live injection @msg:slow"})]
        (with-out-str (cmd-tell-agent ["live-lane" "commit after contention"])))]
  (check "msg wrapper keeps a successful live admission through concurrent writes"
         (and (= msg-admission-timeout-ms @observed-timeout)
              (= 30000 @observed-timeout)
              (= 32 @committed-writes)
              (str/includes? output "queued for live injection @msg:slow"))))

(let [help (proc/shell {:out :string :err :string :continue true
                        :extra-env {"NO_COLOR" "1"}}
                       (str root "/bin/north") "spawn" "--help")]
  (check "top-level spawn help explains template overrides and the bespoke contract"
         (and (zero? (:exit help))
              (str/includes? (:out help) "Stock template:")
              (str/includes? (:out help) "any changed template axis requires --override-reason WHY")
              (str/includes? (:out help) "north templates")
              (str/includes? (:out help) "Bespoke role:")
              (str/includes? (:out help) "--rationale WHY --contract JSON|@file")
              (str/includes? (:out help) "responsibility, deliverable, capabilities, mayDecide")
              (str/includes? (:out help) "--target ACCOUNT")
              (str/includes? (:out help) "--assessment JSON|@file")
              (str/includes? (:out help) "--pin-evidence JSON|@file")
              (str/includes? (:out help) "--model MODEL")
              (str/includes? (:out help) "validate pinned-provider capability authority")
              (not (str/includes? (str (:out help) (:err help)) "unknown spawn option")))))

(let [templates (proc/shell {:out :string :err :string :continue true
                             :extra-env {"NO_COLOR" "1"}}
                            (str root "/bin/north") "templates")]
  (check "north templates is a routed human view over the Orchestration catalog"
         (and (zero? (:exit templates))
              (str/includes? (:out templates) "ORCHESTRATION STOCK TEMPLATES")
              (str/includes? (:out templates)
                             "exact template → justified axis override → bespoke composition")
              (str/includes? (:out templates) "integrator")
              (str/includes? (:out templates) "grade senior · senior/high · worker · deliver")
              (str/includes? (:out templates) "composition.kind=preset"))))

(let [card (proc/shell {:out :string :err :string :continue true}
                       (str root "/bin/north") "help")]
  (check "the top-level card advertises the templates view"
         (and (zero? (:exit card))
              (str/includes? (:out card) "north templates")
              (str/includes? (:out card) "stock templates"))))

(let [dry (proc/shell {:out :string :err :string :continue true
                       :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                      (str root "/bin/north") "spawn" "designer" "probe"
                      "--provider" "openai"
                      "--pin-evidence" (pin-evidence-json [{:kind "provider" :value "openai"}])
                      "--ad-hoc"
                      "--dry-run")]
  (check "spawn dry-run leads with semantic identity and retains control key separately"
         (and (zero? (:exit dry))
              (re-find #"openai-ambient-sol-xhigh-orchestration-designer-[0-9a-f]{12}" (:out dry))
              (re-find #"control: lane-[0-9a-z]+-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}" (:out dry))
              (not (str/includes? (:out dry) "agent-id would be")))))

(let [closed (proc/shell {:out :string :err :string :continue true
                          :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                         (str root "/bin/north") "spawn" "designer" "probe"
                         "--provider" "anthropic"
                         "--pin-evidence" (pin-evidence-json [{:kind "provider" :value "anthropic"}])
                         "--ad-hoc"
                         "--dry-run")]
  (check "CLI dry route resolves anthropic frontier to the current Orchestration opus/xhigh route"
         (and (zero? (:exit closed))
              (re-find #"anthropic-ambient-opus-xhigh-orchestration-designer-[a-z0-9]+" (:out closed))
              (not (str/includes? (:out closed) "anthropic-ambient-fable")))))

(let [dry (proc/shell {:out :string :err :string :continue true
                       :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                      (str root "/bin/north") "spawn" "designer" "probe"
                      "--provider" "openai" "--target" "codex-work"
                      "--pin-evidence" (pin-evidence-json
                                        [{:kind "provider" :value "openai"}
                                         {:kind "account" :value "codex-work"}])
                      "--ad-hoc"
                      "--dry-run")]
  (check "spawn target becomes AGENT_TARGET and appears in the fallback identity"
         (and (zero? (:exit dry))
              (str/includes? (:out dry) "AGENT_TARGET=codex-work")
              (re-find #"openai-codex-work-sol-xhigh-orchestration-designer-[a-z0-9]+" (:out dry)))))

(let [missing-pin (proc/shell {:out :string :err :string :continue true
                               :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                              (str root "/bin/north") "spawn" "executor" "probe"
                              "--provider" "openai" "--ad-hoc" "--dry-run")]
  (check "new public CLI provider pins fail closed without typed current evidence"
         (and (not (zero? (:exit missing-pin)))
              (str/includes? (str (:out missing-pin) (:err missing-pin))
                             "require current typed pinEvidence")
              (not (str/includes? (:out missing-pin) "control:")))))

(let [assessed (proc/shell {:out :string :err :string :continue true
                            :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                           (str root "/bin/north") "spawn" "executor" "probe"
                           "--assessment" (economy-assessment-json) "--ad-hoc" "--dry-run")]
  (check "public CLI accepts a canonical Orchestration assessment and forwards only its recorded marker"
         (and (zero? (:exit assessed))
              (str/includes? (:out assessed) "AGENT_ROUTING_ASSESSMENT=RECORDED")
              (not (str/includes? (:out assessed) "reasoning-shape:deterministic")))))

(let [missing-max-assessment
      (proc/shell {:out :string :err :string :continue true
                   :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                  (str root "/bin/north") "spawn" "executor" "probe"
                  "--tier" "frontier" "--reasoning" "max"
                  "--override-reason" "exceptional deliberation required"
                  "--ad-hoc"
                  "--dry-run")]
  (check "public CLI rejects max reasoning before lane creation without a canonical assessment"
         (and (not (zero? (:exit missing-max-assessment)))
              (str/includes? (str (:out missing-max-assessment) (:err missing-max-assessment))
                             "reasoning=max requires a canonical routingAssessment")
              (not (str/includes? (:out missing-max-assessment) "control:")))))

(let [directory (.toFile
                 (java.nio.file.Files/createTempDirectory
                  "north-goal-fence-"
                  (make-array java.nio.file.attribute.FileAttribute 0)))
      bare "lane-fence"
      fence-file (io/file directory (str bare ".presence-fence.json"))
      valid (str "{\"resource\":\"session:" bare
                 "\",\"holder\":\"" bare "\",\"epoch\":7}\n")
      owner-only
      #{java.nio.file.attribute.PosixFilePermission/OWNER_READ
        java.nio.file.attribute.PosixFilePermission/OWNER_WRITE}]
  (try
    (spit fence-file valid)
    (java.nio.file.Files/setPosixFilePermissions (.toPath fence-file) owner-only)
    (check "goal consumes the exact canonical saved presence fence"
           (= (str/trim valid)
              (saved-presence-fence-json! bare (.getPath directory))))
    (java.nio.file.Files/setPosixFilePermissions
     (.toPath fence-file)
     #{java.nio.file.attribute.PosixFilePermission/OWNER_READ})
    (check "goal refuses a saved presence fence whose mode is not 0600"
           (try
             (saved-presence-fence-json! bare (.getPath directory))
             false
             (catch clojure.lang.ExceptionInfo _ true)))
    (java.nio.file.Files/setPosixFilePermissions (.toPath fence-file) owner-only)
    (spit fence-file
          "{\"resource\":\"session:other\",\"holder\":\"lane-fence\",\"epoch\":7}\n")
    (check "goal refuses a presence fence bound to another session"
           (try
             (saved-presence-fence-json! bare (.getPath directory))
             false
             (catch clojure.lang.ExceptionInfo _ true)))
    (spit fence-file valid)
    (java.nio.file.Files/setPosixFilePermissions (.toPath fence-file) owner-only)
    (let [captured (atom nil)]
      (with-redefs [north.topology-authority/require-coordination! (fn [_] true)
                    agent-facts-one (fn [_] {"kind" "lane"})
                    render-display-name (fn [_ _] "Goal-updated lane")
                    saved-presence-fence-json! (fn [_] (str/trim valid))
                    run (fn [argv & _]
                          (reset! captured argv)
                          {:ok true :out "" :err ""})]
        (cmd-goal [bare "new goal"])
        (check "goal passes five empty optional slots then the exact fence"
               (= ["" "" "" "" "" (str/trim valid)]
                  (subvec (vec @captured) 6 12)))))
    (finally
      (doseq [file (reverse (file-seq directory))]
        (try (io/delete-file file true) (catch Throwable _ nil))))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nagents CLI: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
