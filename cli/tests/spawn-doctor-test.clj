#!/usr/bin/env bb
;; Fixture-driven rows for `north spawn --doctor`: every wall class pinned, so a
;; regression that downgrades a wall to a pass fails here, not in a dead lane.
(require '[clojure.java.io :as io]
         '[clojure.string :as str]
         '[cheshire.core :as json])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))

(load-file (str root "/cli/spawn-doctor.clj"))
(alias 'doctor 'north.spawn-doctor)

(def checks (atom []))
(defn check [label pass?]
  (swap! checks conj [label (boolean pass?)])
  (println (if pass? "PASS" "FAIL") label))

(defn- find-row [rows id]
  (first (filter #(= id (:check %)) rows)))

;; ---- (1) closure integrity: a hook path outside the closure --------------------

(def broken-closure-probe
  {:managedCodexHooks
   {:requirements {:path "/etc/codex/requirements.toml" :ok true}
    :runtime [{:hook "runtime/env" :path "/etc/codex/hooks/runtime/env" :supply "nix"}]
    :hooks [{:hook "beagle-session-start.sh"
             :path "/etc/codex/hooks/beagle-session-start.sh"
             :supply "unavailable"
             :detail "/etc/codex/hooks/beagle-session-start.sh is neither Nix-supplied nor a proven sealed hook"}
            {:hook "tripwire-guard.sh" :path "/etc/codex/hooks/tripwire-guard.sh" :supply "sealed"}]}})

(let [rows (doctor/closure-rows broken-closure-probe)
      summary (find-row rows "closure.managed-codex-hooks")
      named (find-row rows "closure.hook:beagle-session-start.sh")]
  (check "a hook outside the verified closure fails the closure summary row"
         (= :fail (:status summary)))
  (check "the failing closure summary names the OpenAI preflight consequence"
         (str/includes? (:why summary) "managed OpenAI lane"))
  (check "the exact hook that fell out gets its own row"
         (and named (= :fail (:status named))
              (str/includes? (:why named) "neither Nix-supplied nor a proven sealed hook")))
  (check "every failing closure row carries a fix command"
         (every? :fix (filter #(= :fail (:status %)) rows))))

(let [rows (doctor/closure-rows
            {:managedCodexHooks
             {:requirements {:path "/etc/codex/requirements.toml" :ok true}
              :runtime [{:hook "runtime/env" :path "/e/env" :supply "nix"}]
              :hooks [{:hook "tripwire-guard.sh" :path "/e/tripwire-guard.sh" :supply "sealed"}]}})]
  (check "a fully closed hook surface passes with no per-hook rows"
         (and (= 2 (count rows)) (every? #(= :pass (:status %)) rows))))

(let [rows (doctor/closure-rows {:error "bun run failed"})]
  (check "an unreadable preflight inventory is a FAIL, never a silent pass"
         (= :fail (:status (find-row rows "closure.managed-codex-hooks")))))

;; ---- (2) provider accounts -----------------------------------------------------

(let [rows (doctor/provider-rows
            {:allocationMode "balanced"
             :providers [{:targets [{:id "a" :provider "openai" :routing "eligible"
                                     :headroom "plenty" :available true}
                                    {:id "b" :provider "anthropic" :routing "exhausted"
                                     :headroom "exhausted" :available true}
                                    {:id "c" :provider "anthropic" :routing "blocked"
                                     :headroom "unknown" :available false
                                     :availabilityReason "unauthenticated"}]}]})]
  (check "an eligible account passes" (= :pass (:status (find-row rows "providers.a"))))
  (check "an exhausted-but-authenticated account warns rather than failing"
         (= :warn (:status (find-row rows "providers.b"))))
  (check "an unavailable account fails and names its login fix"
         (let [row (find-row rows "providers.c")]
           (and (= :fail (:status row)) (str/includes? (str (:fix row)) "login")))))

(let [rows (doctor/provider-rows {:allocationMode "balanced"
                                  :providers [{:targets [{:id "a" :provider "openai"
                                                          :routing "exhausted" :headroom "exhausted"
                                                          :available true}]}]})]
  (check "no routing-eligible account at all is a FAIL"
         (= :fail (:status (find-row rows "providers.accounts")))))

;; ---- (6) sandbox expectations --------------------------------------------------

(let [rows (doctor/sandbox-rows
            {:presets [{:role "scout" :openai "read-only" :sandboxNetwork "closed"
                        :directShellLoopback "closed" :coordinationTransport "not-granted"}
                       {:role "integrator" :openai "workspace-write" :sandboxNetwork "open"
                        :directShellLoopback "open" :coordinationTransport "not-granted"}
                       {:role "director" :openai "read-only" :sandboxNetwork "closed"
                        :directShellLoopback "closed" :coordinationTransport "north-mcp-host"}]})
      constraint (find-row rows "sandbox.openai")]
  (check "the sandbox expectation always prints as INFO, never a wall"
         (= :info (:status constraint)))
  (check "the sandbox expectation preserves the read-only no-network fact"
         (str/includes? (:why constraint) "read-only/no-network=director,scout"))
  (check "the sandbox expectation is derived, listing each preset's real sandbox"
         (and (str/includes? (:why constraint) "read-only/no-network=director,scout")
              (str/includes? (:why constraint) "workspace-write=integrator")))
  (check "the sandbox expectation names closed direct shell loopback"
         (str/includes? (:why constraint) "direct-shell-loopback-closed=director,scout"))
  (check "host-side North MCP coordination is a named PASS, not a sandbox failure"
         (let [row (find-row rows "sandbox.openai-coordination")]
           (and (= :pass (:status row))
                (str/includes? (:why row) "host-side North MCP")
                (str/includes? (:why row) "director")))))

;; ---- (4) composition dry-run ---------------------------------------------------

(let [row (with-redefs [doctor/shell (fn [& _] {:ok false :exit 1 :out ""
                                                :err "unknown posture: vigorous"})]
            (#'doctor/dry-run-row "dryrun.preset" "canonical preset" ["integrator" "p" "--dry-run"]))]
  (check "a rejected composition surfaces the vocabulary refusal without spawning"
         (and (= :fail (:status row)) (str/includes? (:why row) "unknown posture"))))

(let [row (with-redefs [doctor/shell (fn [& _] {:ok true :exit 0 :err ""
                                                :out "# orchestration dials for role integrator -> grade=senior\n"})]
            (#'doctor/dry-run-row "dryrun.preset" "canonical preset" ["integrator" "p" "--dry-run"]))]
  (check "an accepted composition passes and echoes its resolved dials"
         (and (= :pass (:status row)) (str/includes? (:why row) "grade=senior"))))

;; ---- (3) listener --------------------------------------------------------------

(let [rows (with-redefs [north.message-routing/require-live-address (fn [_ _] {:live false})]
             (with-redefs [doctor/invoking-agent-id (fn [] "lane-probe")]
               (doctor/listener-rows)))
      row (find-row rows "listener.armed")]
  (check "an unarmed invoking listener fails with the arm command"
         (and (= :fail (:status row)) (= "north listen lane-probe" (:fix row)))))

(let [rows (with-redefs [north.message-routing/require-live-address (fn [_ _] {:live true})]
             (with-redefs [doctor/invoking-agent-id (fn [] "lane-probe")]
               (doctor/listener-rows)))]
  (check "an armed invoking listener passes"
         (= :pass (:status (find-row rows "listener.armed")))))

(let [rows (with-redefs [doctor/invoking-agent-id (fn [] nil)] (doctor/listener-rows))]
  (check "no agent identity skips the listener check instead of failing it"
         (= :skip (:status (find-row rows "listener.armed")))))

;; ---- the machine contract ------------------------------------------------------

(let [rows [(doctor/row "closure" "closure.x" :pass "fine")
            (doctor/row "dispatch" "dispatch.y" :impossible "no session can comply" "north providers")]
      payload (json/parse-string (str/trim (with-out-str (doctor/render! rows {:json? true}))) true)]
  (check "--json emits the versioned row contract"
         (= "north:spawn-doctor:v1" (:schema payload)))
  (check "--json counts an IMPOSSIBLE row as failing"
         (= 1 (:failing payload)))
  (check "--json carries the fix command for machine consumers"
         (= "north providers" (-> payload :rows second :fix))))

(let [rendered (with-out-str
                 (doctor/render! [(doctor/row "dispatch" "dispatch.y" :impossible "no session can comply"
                                              "north providers")]
                                 {:json? false}))]
  (check "the table prints the IMPOSSIBLE status and its fix line"
         (and (str/includes? rendered "IMPOSSIBLE")
              (str/includes? rendered "fix: north providers"))))

(let [pass (count (filter second @checks))]
  (println (format "spawn doctor: %d / %d PASS" pass (count @checks)))
  (System/exit (if (= pass (count @checks)) 0 1)))
