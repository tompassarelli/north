#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn managed [facts]
  (let [base (merge {"kind" "lane" "goal" "fixture" "repo" "~/code/north"
                     "spawned_at" "2026-07-17T00:00:00Z"
                     "display_handle" "fixture" "display_name" "fixture"
                     "provider_target" (get facts "provider")}
                    facts)]
    (assoc base "identity_manifest_sha256"
           (north.agent-provenance/manifest-sha256 base))))

(check "preset roster line uses canonical structured axes"
       (= "anthropic:ambient · opus · xhigh · gaffer:designer · working: build the roster"
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
        "gaffer:bespoke:migration-forensics"))

(check "account target and Gaffer template are first-class in the roster"
       (= "openai:codex-work · sol · high · gaffer:designer · working: trace schema"
          (agent-primary-line {:online true}
                              (managed
                               {"kind" "lane" "provider" "openai" "provider_target" "codex-work"
                                "model" "gpt-5.6-sol" "effort" "high" "composition_kind" "preset"
                                "role" "designer" "composition_id" "designer"
                                "composition_overrides" "[]" "goal" "trace schema"}))))

(check "preset overrides are a compact projection of structured facts"
       (= "openai:ambient · sol · xhigh · gaffer:integrator+override(tier,reasoning) · working: cross-seam repair"
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
        "gaffer:legacy-debt"))

(check "default managed target is displayed as ambient"
       (str/starts-with?
        (agent-primary-line {:online true}
                            (managed
                             {"kind" "lane" "provider" "anthropic" "provider_target" "anthropic"
                              "model" "opus" "effort" "high" "composition_kind" "preset"
                              "role" "integrator" "composition_id" "integrator"
                              "composition_overrides" "[]"}))
        "anthropic:ambient · opus · high · gaffer:integrator"))

(check "historical native gaps are explicit provenance labels, never model names"
       (= "provider:historical-unrecorded · model:historical-unrecorded · effort:historical-unrecorded · gaffer:not-selected · working: unknown"
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
        "openai · sol · effort:unobserved · gaffer:not-selected"))

(check "roster lifecycle categories do not call terminal TTL rows active"
       (and (= :active-agent (roster-category {"kind" "lane"}))
            (= :native-session (roster-category {"kind" "session"}))
            (= :recently-finished (roster-category {"kind" "lane" "outcome" "ran"}))
            (= :unclassified (roster-category {}))))

(check "uncomposed role remains visible without inventing Gaffer provenance"
       (let [facts {"kind" "lane" "provider" "anthropic" "model" "opus"
                    "effort" "xhigh"
                    "role" "orchestrator" "goal" "coordinate work"}]
         (and (str/includes? (agent-primary-line {:online true} facts)
                             "gaffer:legacy-debt · role:orchestrator")
              (= "anthropic-opus-xhigh-gaffer-legacy-debt-legacy"
                 (semantic-handle "lane-legacy" facts)))))

(check "only provider-native sessions receive the native handle segment"
       (= "openai-sol-unobserved-gaffer-not-selected-native"
          (semantic-handle "session-native"
                           {"kind" "session" "provider" "openai" "model" "gpt-5.6-sol"
                            "effort" "unobserved"})))

(check "display labels are never reverse-parsed into missing structured facts"
       (let [facts {"kind" "lane" "display_name" "anthropic opus xhigh designer"}]
         (and (str/starts-with? (agent-primary-line {:online true} facts)
                                "unknown · unknown · unknown · gaffer:legacy-debt")
              (str/starts-with? (semantic-handle "sdk-a205e9ce" facts)
                                "unknown-unknown-unknown-gaffer-legacy-debt-"))))

(check "current structured effort overrides a stale stored handle"
       (= "openai-ambient-sol-xhigh-gaffer-designer-a205e9ce"
          (semantic-handle "sdk-a205e9ce"
                           (managed
                            {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                             "effort" "xhigh" "composition_kind" "preset"
                             "role" "designer" "composition_id" "designer"
                             "composition_overrides" "[]"
                             "display_handle" "openai-sol-high-designer-a205e9ce"}))))

(check "CLI Fable window matches the canonical Eastern-time exclusive boundary"
       (and (= (java.time.Instant/parse "2026-07-20T04:00:00Z") FABLE-WINDOW-END)
            (fable-window-open? (java.time.Instant/parse "2026-07-20T03:59:59.999Z"))
            (not (fable-window-open? (java.time.Instant/parse "2026-07-20T04:00:00Z")))))

(let [bulk [(str root "/bin/north") "json" "agents"]
      show [(str root "/bin/north") "json" "show" "agent:sdk-recovered"]
      calls (atom [])
      facts (with-redefs [run (fn [argv & _]
                                (swap! calls conj argv)
                                (cond
                                  (= argv bulk) {:ok false :exit 1 :err "malformed warm row"}
                                  (= argv show) {:ok true :exit 0
                                                 :out "[{\"predicate\":\"provider\",\"value\":\"openai\"},{\"predicate\":\"model\",\"value\":\"gpt-5.6-sol\"}]"}
                                  :else {:ok true :exit 0 :out "[]"}))]
              (agent-facts ["sdk-recovered" "legacy-session"]))]
  (check "a failed bulk projection recovers structured identity per live agent"
         (= {"provider" "openai" "model" "gpt-5.6-sol"}
            (get facts "sdk-recovered")))
  (check "a fact-less legacy row stays honestly unknown"
         (and (= {} (get facts "legacy-session"))
              (str/starts-with? (agent-primary-line {:online true} (get facts "legacy-session" {}))
                                "unknown · unknown · unknown · gaffer:legacy-debt")))
  (check "fallback is structured and scoped only to missing live ids"
         (and (= bulk (first @calls))
              (= 3 (count @calls)))))

(let [out (with-redefs [presence-rows (fn [] {:agents [{:id "lane-active" :online true :expires "10s"}
                                                        {:id "session-active" :online true :expires "20s"}
                                                        {:id "lane-done" :online true :expires "30s"}]})
                        agent-facts (fn [_] {"lane-active" {"kind" "lane" "provider" "openai"
                                                            "model" "gpt-5.6-sol" "effort" "high"
                                                            "role" "integrator" "composition_kind" "preset"
                                                            "composition_id" "integrator" "composition_overrides" "[]"}
                                             "session-active" {"kind" "session" "provider" "anthropic"
                                                               "model" "claude-opus-4-8" "effort" "xhigh"}
                                             "lane-done" {"kind" "lane" "provider" "openai"
                                                          "model" "gpt-5.6-sol" "effort" "high"
                                                          "role" "designer" "composition_kind" "preset"
                                                          "composition_id" "designer" "composition_overrides" "[]"
                                                          "outcome" "ran"}})]
            (with-out-str (cmd-agents [])))]
  (check "roster summary separates active and recently finished counts"
         (and (str/includes? out "3 roster entries · 2 active · 1 recently finished")
              (str/includes? out "active agents (1)")
              (str/includes? out "native sessions (1)")
              (str/includes? out "recently finished (1)")
              (not (str/includes? out "live agents"))))
  (check "ordinary roster output hides the internal presence probe"
         (not (str/includes? out "presence-cli.clj"))))

(let [out (with-redefs [presence-rows (fn [] {:agents []})
                        agent-facts (fn [_] {})]
            (with-out-str (cmd-agents ["--verbose"])))]
  (check "verbose roster output retains the internal presence probe"
         (str/includes? out "presence-cli.clj")))

(let [steer (proc/shell {:out :string :err :string :continue true
                         :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                        "bb" (str root "/cli/agents-cli.clj") "steer"
                        "probe-agent" "probe-message" "--dry-run")]
  (check "steer remains parseable and keeps the internal control key"
         (and (zero? (:exit steer))
              (str/includes? (:out steer) "send north-cli probe-agent steer probe-message")
              (str/includes? (:out steer) "[dry-run] not sent."))))

(let [help (proc/shell {:out :string :err :string :continue true
                        :extra-env {"NO_COLOR" "1"}}
                       (str root "/bin/north") "spawn" "--help")]
  (check "top-level spawn help explains preset overrides and the bespoke contract"
         (and (zero? (:exit help))
              (str/includes? (:out help) "Preset role:")
              (str/includes? (:out help) "any changed preset axis requires --override-reason WHY")
              (str/includes? (:out help) "Bespoke role:")
              (str/includes? (:out help) "--rationale WHY --contract JSON|@file")
              (str/includes? (:out help) "responsibility, deliverable, capabilities, mayDecide")
              (str/includes? (:out help) "--target ACCOUNT")
              (not (str/includes? (str (:out help) (:err help)) "unknown spawn option")))))

(let [dry (proc/shell {:out :string :err :string :continue true
                       :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                      "bb" (str root "/cli/agents-cli.clj") "spawn" "designer" "probe"
                      "--provider" "openai" "--dry-run")]
  (check "spawn dry-run leads with semantic identity and retains control key separately"
         (and (zero? (:exit dry))
              (re-find #"openai-ambient-sol-xhigh-gaffer-designer-[0-9a-f]{12}" (:out dry))
              (re-find #"control: lane-[0-9a-z]+-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}" (:out dry))
              (not (str/includes? (:out dry) "agent-id would be")))))

(let [open (proc/shell {:out :string :err :string :continue true
                        :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"
                                    "NORTH_FABLE_NOW" "2026-07-20T03:59:59.999Z"}}
                       "bb" (str root "/cli/agents-cli.clj") "spawn" "designer" "probe"
                       "--provider" "anthropic" "--dry-run")
      closed (proc/shell {:out :string :err :string :continue true
                          :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"
                                      "NORTH_FABLE_NOW" "2026-07-20T04:00:00Z"}}
                         "bb" (str root "/cli/agents-cli.clj") "spawn" "designer" "probe"
                         "--provider" "anthropic" "--dry-run")]
  (check "CLI dry route uses fable/xhigh immediately before the cutoff"
         (and (zero? (:exit open))
              (re-find #"anthropic-ambient-fable-xhigh-gaffer-designer-[a-z0-9]+" (:out open))))
  (check "CLI dry route falls back to opus/xhigh exactly at the cutoff"
         (and (zero? (:exit closed))
              (re-find #"anthropic-ambient-opus-xhigh-gaffer-designer-[a-z0-9]+" (:out closed))
              (not (str/includes? (:out closed) "anthropic-fable")))))

(let [dry (proc/shell {:out :string :err :string :continue true
                       :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                      "bb" (str root "/cli/agents-cli.clj") "spawn" "designer" "probe"
                      "--provider" "openai" "--target" "codex-work" "--dry-run")]
  (check "spawn target becomes AGENT_TARGET and appears in the fallback identity"
         (and (zero? (:exit dry))
              (str/includes? (:out dry) "AGENT_TARGET=codex-work")
              (re-find #"openai-codex-work-sol-xhigh-gaffer-designer-[a-z0-9]+" (:out dry)))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nagents CLI: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
