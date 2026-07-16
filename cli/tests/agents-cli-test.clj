#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(System/setProperty "north.agents.lib" "1")
(load-file (str root "/cli/agents-cli.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))

(check "preset roster line uses canonical structured axes"
       (= "anthropic · opus · xhigh · gaffer:designer · working: build the roster"
          (agent-primary-line {:online true}
                              {"kind" "lane" "provider" "anthropic" "model" "claude-opus-4-8"
                               "effort" "xhigh" "composition_kind" "preset"
                               "composition_id" "designer" "goal" "build the roster"})))

(check "bespoke provenance is explicit"
       (str/includes?
        (agent-primary-line {:online true}
                            {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                             "effort" "high" "composition_kind" "bespoke"
                             "composition_id" "migration-forensics" "goal" "trace schema"})
        "gaffer:bespoke(migration-forensics)"))

(check "account target and Gaffer template are first-class in the roster"
       (= "openai:codex-work · sol · high · gaffer:designer · working: trace schema"
          (agent-primary-line {:online true}
                              {"kind" "lane" "provider" "openai" "provider_target" "codex-work"
                               "model" "gpt-5.6-sol" "effort" "high" "composition_kind" "preset"
                               "composition_id" "designer" "goal" "trace schema"})))

(check "default managed target is displayed as ambient"
       (str/starts-with?
        (agent-primary-line {:online true}
                            {"kind" "lane" "provider" "anthropic" "provider_target" "anthropic"
                             "model" "opus" "effort" "high" "composition_kind" "preset"
                             "composition_id" "integrator"})
        "anthropic:ambient · opus · high · gaffer:integrator"))

(check "native session is explicit and absent provider dials stay unknown"
       (= "unknown · unknown · unknown · gaffer:none · working: unknown"
          (agent-primary-line {:online true :focus "CONTEXT BRIEF:"} {"kind" "session"})))

(check "uncomposed role remains visible without inventing Gaffer provenance"
       (str/includes?
        (agent-primary-line {:online true}
                            {"kind" "lane" "provider" "anthropic" "model" "opus"
                             "effort" "xhigh" "composition_kind" "none"
                             "role" "orchestrator" "goal" "coordinate work"})
        "gaffer:none · role:orchestrator"))

(check "display labels are never reverse-parsed into missing structured facts"
       (let [facts {"kind" "lane" "display_name" "anthropic opus xhigh designer"}]
         (and (str/starts-with? (agent-primary-line {:online true} facts)
                                "unknown · unknown · unknown · gaffer:unknown")
              (str/starts-with? (semantic-handle "sdk-a205e9ce" facts)
                                "unknown-unknown-unknown-unknown-"))))

(check "current structured effort overrides a stale stored handle"
       (= "openai-sol-xhigh-designer-a205e9ce"
          (semantic-handle "sdk-a205e9ce"
                           {"kind" "lane" "provider" "openai" "model" "gpt-5.6-sol"
                            "effort" "xhigh" "composition_kind" "preset"
                            "composition_id" "designer"
                            "display_handle" "openai-sol-high-designer-a205e9ce"})))

(let [steer (proc/shell {:out :string :err :string :continue true
                         :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                        "bb" (str root "/cli/agents-cli.clj") "steer"
                        "probe-agent" "probe-message" "--dry-run")]
  (check "steer remains parseable and keeps the internal control key"
         (and (zero? (:exit steer))
              (str/includes? (:out steer) "send north-cli probe-agent steer probe-message")
              (str/includes? (:out steer) "[dry-run] not sent."))))

(let [dry (proc/shell {:out :string :err :string :continue true
                       :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                      "bb" (str root "/cli/agents-cli.clj") "spawn" "designer" "probe"
                      "--provider" "openai" "--dry-run")]
  (check "spawn dry-run leads with semantic identity and retains control key separately"
         (and (zero? (:exit dry))
              (re-find #"openai-sol-xhigh-designer-[a-z0-9]+" (:out dry))
              (str/includes? (:out dry) "control: lane-")
              (not (str/includes? (:out dry) "agent-id would be")))))

(let [dry (proc/shell {:out :string :err :string :continue true
                       :extra-env {"NORTH_AGENTS_LIB" "" "NO_COLOR" "1"}}
                      "bb" (str root "/cli/agents-cli.clj") "spawn" "designer" "probe"
                      "--provider" "openai" "--target" "codex-work" "--dry-run")]
  (check "spawn target becomes AGENT_TARGET and appears in the fallback identity"
         (and (zero? (:exit dry))
              (str/includes? (:out dry) "AGENT_TARGET=codex-work")
              (re-find #"openai-codex-work-sol-xhigh-designer-[a-z0-9]+" (:out dry)))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nagents CLI: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
