#!/usr/bin/env bb
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root (.getCanonicalPath
           (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/spawn-process.clj"))

(def checks (atom []))
(defn check [label ok?] (swap! checks conj [label (boolean ok?)]))
(defn eventually [pred timeout-ms]
  (let [deadline (+ (System/currentTimeMillis) timeout-ms)]
    (loop []
      (cond (pred) true
            (>= (System/currentTimeMillis) deadline) false
            :else (do (Thread/sleep 20) (recur))))))

(def temp-dir (.toFile (java.nio.file.Files/createTempDirectory "north-spawn-process-" (make-array java.nio.file.attribute.FileAttribute 0))))
(defn temp-file [name] (io/file temp-dir name))
(def base-env (into {} (System/getenv)))

(def ready-base
  {"kind" "lane"
   "role" "startup-verification-owner"
   "goal" "verify startup identity"
   "provider" "openai"
   "provider_target" "codex-personal"
   "live_input" "unsupported"
   "live_input_state" "frozen"
   "live_input_epoch" "00000000-0000-4000-8000-000000000101"
   "model" "gpt-5.6-sol"
   "effort" "high"
   "composition_kind" "template"
   "composition_id" "verifier"
   "composition_overrides" "[]"
   "repo" "north"
   "spawned_at" "2026-07-17T00:00:00Z"
   "display_handle" "openai-sol-high-verifier-probe"
   "display_name" "openai:codex-personal · sol · high · orchestration:verifier · verify startup identity"})
(defn committed [facts]
  (assoc facts "identity_manifest_sha256"
         (north.agent-provenance/manifest-sha256 facts)))
(defn fold-observed [facts]
  (reduce-kv north.agent-provenance/fold-fact {} facts))
(def ready-facts (committed ready-base))

(try
  (let [uuid (java.util.UUID/fromString "123e4567-e89b-42d3-a456-426614174000")
        id (north.spawn-process/create-agent-id "lane" 1720000000000 uuid)
        ids (repeatedly 200 #(north.spawn-process/create-agent-id "lane"))]
    (check "managed lane id carries base36 time and the complete RFC 4122 UUID"
           (re-matches #"^lane-[0-9a-z]+-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$" id))
    (check "managed lane ids do not truncate their collision domain"
           (= (count ids) (count (set ids)))))

  (check "startup identity requires every route and lifecycle axis"
         (and (north.spawn-process/identity-ready? ready-facts)
              (not (north.spawn-process/identity-ready? (dissoc ready-facts "provider_target")))
              (not (north.spawn-process/identity-ready? (assoc ready-facts "kind" "session")))
              (not (north.spawn-process/identity-ready? (dissoc ready-facts "role")))
              (not (north.spawn-process/identity-ready? (dissoc ready-facts "goal")))
              (not (north.spawn-process/identity-ready? (dissoc ready-facts "composition_id")))
              (not (north.spawn-process/identity-ready? (assoc ready-facts "composition_kind" "invalid")))
              (not (north.spawn-process/identity-ready? (assoc ready-facts "composition_id" "not safe")))))

  (check "managed startup accepts independent role and template provenance"
         (and (north.spawn-process/identity-ready? ready-facts)
              (= "verifier" (get ready-facts "composition_id"))
              (= "startup-verification-owner" (get ready-facts "role"))))

  (check "managed startup defects name invalid composition provenance"
         (= ["composition_kind(template|bespoke)"]
            (north.spawn-process/identity-defects
             (committed (assoc ready-base "composition_kind" "invalid")))))

  (let [turn-messages
        (committed
         (assoc ready-base
                "live_input" "turn-messages"
                "live_input_state" "frozen"))]
    (check "canonical SDK turn-messages identity crosses the startup gate"
           (and (north.spawn-process/identity-ready? turn-messages)
                (not (north.spawn-process/identity-ready?
                      (committed
                       (assoc ready-base
                              "live_input" "invalid"
                              "live_input_state" "frozen")))))))

  (check "all managed lanes share the default startup acknowledgement budget"
         (= 180000
            (north.spawn-process/default-startup-timeout-for-capabilities
             ["filesystem.write"])))

  (let [pending-facts
        (committed
         (assoc ready-base
                "live_input" "streaming"
                "live_input_state" "pending"))
        armed-facts
        (committed
         (assoc ready-base
                "live_input" "streaming"
                "live_input_state" "armed"))
        log (temp-file "pending-route.log")
        process (north.spawn-process/launch-detached! ["sleep" "10"] base-env log)
        probes (atom 0)
        startup
        (north.spawn-process/await-startup
         process "lane-pending-route" log
         (fn [_] (if (< (swap! probes inc) 4) pending-facts armed-facts))
         (constantly true)
         :timeout-ms 1000 :poll-ms 10)]
    (check "online presence plus pending streaming identity cannot acknowledge startup"
           (and (not (north.spawn-process/identity-ready? pending-facts))
                (= ["effective_live_input_route"]
                   (north.spawn-process/startup-defects pending-facts))
                (= :ready (:status startup))
                (>= @probes 4)))
    (north.spawn-process/stop-process! process))

  (let [pending-facts
        (committed
         (assoc ready-base
                "live_input" "streaming"
                "live_input_state" "pending"))
        log (temp-file "pending-fast-terminal.log")
        process (north.spawn-process/launch-detached! ["bash" "-c" "exit 0"] base-env log)
        startup
        (north.spawn-process/await-startup
         process "lane-pending-fast-terminal" log
         (constantly (assoc pending-facts "process_outcome" "died"))
         (constantly false)
         :timeout-ms 1000 :poll-ms 10)]
    (check "valid fast terminal remains completed even when no route was armed"
           (and (= :completed (:status startup))
                (= "died" (:outcome startup)))))

  (let [log (temp-file "ready.log")
        process (north.spawn-process/launch-detached! ["sleep" "10"] base-env log)
        startup (north.spawn-process/await-startup
                 process "lane-ready" log (constantly ready-facts) (constantly true)
                 :timeout-ms 1000 :poll-ms 10)]
    (check "live acknowledgement requires structured identity plus online presence"
           (and (= :ready (:status startup))
                (= "openai-sol-high-verifier-probe" (:handle startup))
                (north.spawn-process/process-alive? process)))
    (north.spawn-process/stop-process! process))

  (let [log (temp-file "completed.log")
        process (north.spawn-process/launch-detached! ["bash" "-c" "exit 0"] base-env log)
        startup (north.spawn-process/await-startup
                 process "lane-completed" log
                 (constantly (assoc ready-facts "process_outcome" "ran")) (constantly false)
                 :timeout-ms 1000 :poll-ms 10)]
    (check "fast terminal outcome is reported as completed, never falsely running"
           (and (= :completed (:status startup)) (= "ran" (:outcome startup)))))

  (let [marker (temp-file "await-detached-exit.marker")
        log (temp-file "await-detached-exit.log")
        process (north.spawn-process/launch-detached!
                 ["bash" "-c"
                  "sleep 0.25; printf child-complete > \"$NORTH_AWAIT_EXIT_MARKER\""]
                 (assoc base-env "NORTH_AWAIT_EXIT_MARKER" (str marker)) log)
        terminal (future (north.spawn-process/await-process-exit process :poll-ms 10))
        _ @process]
    (check "detached exit waiter ignores the completed setsid launcher"
           (= :waiting (deref terminal 100 :waiting)))
    (check "detached exit waiter resolves from the actual child receipt"
           (and (= 0 (deref terminal 2000 :timeout))
                (= "child-complete" (slurp marker)))))

  (let [log (temp-file "exit-race.log")
        process (north.spawn-process/launch-detached! ["bash" "-c" "exit 0"] base-env log)
        _ @process
        probes (atom 0)
        startup (north.spawn-process/await-startup
                 process "lane-exit-race" log
                 (fn [_]
                   (if (= 1 (swap! probes inc))
                     ready-facts
                     (assoc ready-facts "process_outcome" "ran")))
                 (constantly false)
                 :timeout-ms 1000 :poll-ms 10 :exit-grace-ms 100)]
    (check "final fact read closes the synchronous-outcome versus process-exit race"
           (and (= :completed (:status startup))
                (= "ran" (:outcome startup))
                (>= @probes 2))))

  (let [log (temp-file "terminal-publication-race.log")
        process (north.spawn-process/launch-detached! ["sleep" "10"] base-env log)
        probes (atom 0)
        partial (assoc ready-facts
                       "process_outcome" "ran"
                       "delivery_outcome" "unverified"
                       "delivery_reason" "provider_terminal_success_without_external_verification")
        complete (assoc partial "terminal_manifest_sha256"
                        (north.terminal-projection/terminal-manifest-sha256 partial))
        startup (north.spawn-process/await-startup
                 process "lane-terminal-publication-race" log
                 (fn [_] (if (< (swap! probes inc) 4) partial complete))
                 (constantly false)
                 :timeout-ms 1000 :poll-ms 10)]
    (check "partial new terminal publication cannot win the startup race"
           (and (= :completed (:status startup))
                (= "ran" (:outcome startup))
                (>= @probes 4)))
    (north.spawn-process/stop-process! process))

  (let [log (temp-file "partial-terminal-exit.log")
        process (north.spawn-process/launch-detached! ["bash" "-c" "exit 0"] base-env log)
        _ @process
        partial (assoc ready-facts "process_outcome" "ran")
        startup (north.spawn-process/await-startup
                 process "lane-partial-terminal-exit" log
                 (constantly partial) (constantly false)
                 :timeout-ms 1000 :poll-ms 10 :exit-grace-ms 50)]
    (check "process_outcome without terminal marker stays partial"
           (= :failed (:status startup))))

  (let [log (temp-file "conflicting-terminal-exit.log")
        terminal {"process_outcome" "ran"
                  "delivery_outcome" "unverified"
                  "delivery_reason" "provider_terminal_success_without_external_verification"}
        complete (merge ready-facts terminal
                        {"terminal_manifest_sha256"
                         (north.terminal-projection/terminal-manifest-sha256 terminal)})
        conflicted (north.agent-provenance/fold-fact
                    (fold-observed complete) "process_outcome" "died")
        process (north.spawn-process/launch-detached! ["bash" "-c" "exit 0"] base-env log)
        _ @process
        startup (north.spawn-process/await-startup
                 process "lane-conflicting-terminal-exit" log
                 (constantly conflicted) (constantly false)
                 :timeout-ms 1000 :poll-ms 10 :exit-grace-ms 50)]
    (check "conflicting multi-valued terminal cannot acknowledge a completed startup"
           (= :failed (:status startup))))

  (let [log (temp-file "failed.log")
        process (north.spawn-process/launch-detached!
                 ["bash" "-c" "printf 'provider construction failed\\n' >&2; exit 23"] base-env log)
        startup (north.spawn-process/await-startup
                 process "lane-failed" log (constantly {}) (constantly false)
                 :timeout-ms 2000 :poll-ms 10)
        message (north.spawn-process/failure-message startup)]
    (check "pre-identity child exit is synchronous and preserves its real status"
           (and (= :failed (:status startup)) (= 23 (:exit startup))))
    (check "failed startup never fabricates a semantic handle"
           (and (nil? (:handle startup))
                (str/includes? message "missing identity")
                (not (str/includes? message "unknown-unknown"))))
    (check "early failure points to durable evidence and includes bounded log context"
           (and (str/includes? message (str log))
                (str/includes? message "provider construction failed"))))

  (let [log (temp-file "timeout.log")
        process (north.spawn-process/launch-detached! ["sleep" "10"] base-env log)
        startup (north.spawn-process/await-startup
                 process "lane-timeout" log (constantly {}) (constantly false)
                 :timeout-ms 100 :poll-ms 10)]
    (check "missing acknowledgement times out and tears down the detached process tree"
           (and (= :timeout (:status startup))
                (eventually #(not (p/alive? process)) 2000))))

  ;; True lifetime boundary: a short-lived launcher starts a detached child and
  ;; exits. The child must still run to completion after its Babashka parent is
  ;; gone; this is the exact failure shape of `north spawn` returning to a shell.
  (let [marker (temp-file "survived")
        log (temp-file "survival.log")
        child-expr "(Thread/sleep 250) (spit (System/getenv \"NORTH_DETACH_MARKER\") \"survived\")"
        launcher-expr
        (str "(load-file " (pr-str (str root "/cli/spawn-process.clj")) ") "
             "(north.spawn-process/launch-detached! "
             (pr-str ["bb" "-e" child-expr]) " "
             (pr-str {"NORTH_DETACH_MARKER" (str marker)}) " "
             (pr-str (str log)) ")")
        launcher (p/shell {:out :string :err :string :continue true} "bb" "-e" launcher-expr)]
    (check "launcher itself exits cleanly without waiting for the managed child"
           (zero? (:exit launcher)))
    (check "detached lane survives invoking CLI process exit"
           (and (eventually #(.isFile marker) 3000)
                (= "survived" (slurp marker)))))

  ;; The production parent does not merely return: its provider supervisor
  ;; reaps the MCP/CLI process tree. A waitable `setsid --fork --wait` wrapper
  ;; kept the lane discoverable as a descendant, so that reap propagated TERM
  ;; into an admitted lane. The daemonized lane must already be reparented.
  (let [ready (temp-file "tree-reap-ready")
        marker (temp-file "tree-reap-survived")
        log (temp-file "tree-reap.log")
        child-command
        ["bash" "-c"
         "printf ready > \"$NORTH_DETACH_READY\"; sleep 0.4; printf survived > \"$NORTH_DETACH_MARKER\""]
        child-env (assoc base-env
                         "NORTH_DETACH_READY" (str ready)
                         "NORTH_DETACH_MARKER" (str marker))
        launcher-expr
        (str "(load-file " (pr-str (str root "/cli/spawn-process.clj")) ") "
             "(north.spawn-process/launch-detached! "
             (pr-str child-command) " "
             (pr-str child-env) " "
             (pr-str (str log)) ") "
             "(Thread/sleep 10000)")
        launcher (p/process ["bb" "-e" launcher-expr]
                            {:out :string :err :string})]
    (check "tree-reap probe reaches the admitted child before killing its launcher"
           (eventually #(.isFile ready) 3000))
    (p/destroy-tree launcher)
    (deref launcher 2000 nil)
    (check "daemonized lane survives ancestor process-tree cleanup"
           (and (eventually #(.isFile marker) 3000)
                (= "survived" (slurp marker)))))

  ;; Full daemonized lane ownership boundary. The lane first unrefs its owned
  ;; coordinator, then crosses an asynchronous pre-provider gap. It must retain
  ;; itself, keep writing the inherited durable log after the dispatcher dies,
  ;; launch the provider, and reap the coordinator on provider death.
  (let [log (temp-file "daemonized-lane-lifecycle.log")
        coordinator-pid-file (temp-file "daemonized-coordinator.pid")
        provider-pid-file (temp-file "daemonized-provider.pid")
        terminal-file (temp-file "daemonized-terminal")
        child-env (assoc base-env
                         "NORTH_TEST_COORDINATOR_PID_FILE" (str coordinator-pid-file)
                         "NORTH_TEST_PROVIDER_PID_FILE" (str provider-pid-file)
                         "NORTH_TEST_TERMINAL_FILE" (str terminal-file))
        child-command ["bun" (str root "/sdk/test/fixtures/daemonized-lane-lifecycle.ts")]
        launcher-expr
        (str "(load-file " (pr-str (str root "/cli/spawn-process.clj")) ") "
             "(north.spawn-process/launch-detached! "
             (pr-str child-command) " "
             (pr-str child-env) " "
             (pr-str (str log)) ") "
             "(Thread/sleep 10000)")
        launcher (p/process ["bb" "-e" launcher-expr]
                            {:out :string :err :string})]
    (try
      (check "daemonized lifecycle reaches coordinator boot before dispatcher exit"
             (eventually #(and (.isFile coordinator-pid-file)
                               (str/includes? (slurp log) "coordinator-boot"))
                         3000))
      (p/destroy-tree launcher)
      (deref launcher 2000 nil)
      (check "daemonized lifecycle launches its provider after dispatcher exit"
             (eventually #(.isFile provider-pid-file) 3000))
      (let [provider-pid (Long/parseLong (str/trim (slurp provider-pid-file)))
            coordinator-pid (Long/parseLong (str/trim (slurp coordinator-pid-file)))]
        (check "detached lane keeps its durable log writable through provider launch"
               (eventually #(let [contents (slurp log)]
                              (and (str/includes? contents "starting provider=fixture")
                                   (str/includes? contents "provider-process-started")))
                           3000))
        (check "dispatcher exit does not kill the launched provider"
               (north.spawn-process/process-alive?
                {:north.spawn-process/pid-file provider-pid-file}))
        @(p/process ["kill" "-KILL" (str provider-pid)]
                    {:out :string :err :string :continue true})
        (check "provider death reaps the lane coordinator and writes a terminal line"
               (and (eventually #(.isFile terminal-file) 3000)
                    (= "provider_died coordinator_reaped"
                       (str/trim (slurp terminal-file)))
                    (eventually #(not
                                  (north.spawn-process/process-alive?
                                   {:north.spawn-process/pid-file coordinator-pid-file}))
                                3000)
                    (str/includes? (slurp log)
                                   "terminal provider_died coordinator_reaped"))))
      (finally
        (doseq [pid-file [provider-pid-file coordinator-pid-file]]
          (when (.isFile pid-file)
            (let [pid (Long/parseLong (str/trim (slurp pid-file)))
                  handle (java.lang.ProcessHandle/of pid)]
              (when (and (.isPresent handle) (.isAlive (.get handle)))
                @(p/process ["kill" "-KILL" (str pid)]
                            {:out :string :err :string :continue true})))))
        (try (p/destroy-tree launcher) (catch Exception _ nil))
        (deref launcher 2000 nil))))

  (finally
    (north.spawn-process/stop-process!
     ;; stop-process! is intentionally tolerant; exercise the no-op boundary.
     nil)
    (doseq [file (reverse (file-seq temp-dir))] (io/delete-file file true))))

(let [results @checks pass (count (filter second results))]
  (doseq [[label ok?] results]
    (println (format "  [%s] %s" (if ok? "PASS" "FAIL") label)))
  (println (format "\nspawn process integration: %d / %d PASS" pass (count results)))
  (System/exit (if (= pass (count results)) 0 1)))
