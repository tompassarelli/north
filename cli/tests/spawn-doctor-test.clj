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

;; ---- (5) graph-authoring roots: unset ------------------------------------------

(let [rows (doctor/roots-rows
            {:graphAuthoring
             {:framHome nil :beagleHome nil
              :rootsError "graph_authoring_fram_roots_unset: missing NORTH_FRAM_HOME, NORTH_BEAGLE_HOME"
              :checkouts []}})
      row (find-row rows "roots.graph-authoring")]
  (check "unset graph-authoring roots fail with the named SDK error"
         (and (= :fail (:status row)) (str/includes? (:why row) "graph_authoring_fram_roots_unset")))
  (check "the unset-roots row hands back the export that fixes it"
         (str/includes? (str (:fix row)) "NORTH_FRAM_HOME")))

(let [rows (doctor/roots-rows
            {:graphAuthoring
             {:framHome "/no/such/fram" :beagleHome "/no/such/beagle"
              :framMcpCommand "/no/such/fram/bin/fram-mcp" :framMcpExecutable false
              :checkouts [{:name "NORTH_FRAM_HOME" :path "/no/such/fram"
                           :exists false :isGitCheckout false}
                          {:name "NORTH_BEAGLE_HOME" :path "/no/such/beagle"
                           :exists true :isGitCheckout false}]}})]
  (check "a root pointing at nothing fails"
         (= :fail (:status (find-row rows "roots.NORTH_FRAM_HOME"))))
  (check "a root that is not a git checkout fails"
         (= :fail (:status (find-row rows "roots.NORTH_BEAGLE_HOME"))))
  (check "a non-executable fram-mcp fails"
         (= :fail (:status (find-row rows "roots.fram-mcp")))))

;; ---- (5, --deep) the lane-local coordinator launch smoke -----------------------

(let [rows (with-redefs [doctor/shell (fn [& _] {:ok false :exit 2 :out ""
                                                 :err "fram-daemon: serve-flat was removed"})]
             (doctor/deep-rows {:graphAuthoring {:framDaemonCommand "/f/bin/fram-daemon"
                                                 :framDaemonExecutable true}}))
      row (find-row rows "deep.lane-coordinator")]
  (check "a lane-local coordinator that exits 2 is a FAIL naming the exit and the cause"
         (and (= :fail (:status row))
              (str/includes? (:why row) "exited 2")
              (str/includes? (:why row) "serve-flat was removed"))))

;; ---- (7) guard consistency: the impossible constraint --------------------------

(defn- guard-rows-with [{:keys [registry adopted advertised fram-home]}]
  (with-redefs [doctor/registry-paths (fn [] registry)
                doctor/scan-repos (fn [_ _] ["/tmp/doctor-fixture-repo"])
                doctor/sentinel-candidates (fn [_] {"/tmp/doctor-fixture-repo" []})
                doctor/guard-script "/fixture/code-upstream-guard.sh"
                doctor/guard-denies? (fn [path] (contains? (set adopted) path))
                doctor/advertised-fram-tools (fn [_] advertised)]
    (doctor/guard-rows {:graphAuthoring {:framHome fram-home}})))

(let [rows (guard-rows-with
            {:registry ["/etc/hostname"]
             :adopted ["/etc/hostname"]
             :fram-home "/fixture/fram"
             :advertised {:tools ["tell" "retract" "show" "ask" "validate"]}})
      row (find-row rows "guard.fram-verbs-mountable")]
  (check "a guard redirecting to verbs the server refuses is flagged IMPOSSIBLE"
         (= :impossible (:status row)))
  (check "the impossible-constraint row names the unmountable verbs"
         (and (str/includes? (:why row) "mcp__fram__add-def")
              (str/includes? (:why row) "mcp__fram__set-body")
              (str/includes? (:why row) "mcp__fram__rename-def")))
  (check "the impossible-constraint row names what the server does serve"
         (str/includes? (:why row) "tell,retract,show,ask,validate"))
  (check "the impossible-constraint row offers a compliant escape"
         (some? (:fix row))))

(let [rows (guard-rows-with
            {:registry ["/etc/hostname"]
             :adopted ["/etc/hostname"]
             :fram-home nil
             :advertised {:tools []}})]
  (check "adopted files with no resolvable fram checkout are flagged IMPOSSIBLE"
         (= :impossible (:status (find-row rows "guard.fram-verbs-mountable")))))

(let [rows (guard-rows-with
            {:registry ["/etc/hostname"]
             :adopted ["/etc/hostname"]
             :fram-home "/fixture/fram"
             :advertised {:tools ["tell" "add-def" "set-body" "rename-def"]}})]
  (check "a server advertising every redirect verb passes"
         (= :pass (:status (find-row rows "guard.fram-verbs-mountable")))))

(let [rows (guard-rows-with
            {:registry ["/etc/hostname"] :adopted [] :fram-home "/fixture/fram"
             :advertised {:tools []}})]
  (check "with nothing actually adopted the mountability check skips, never fails"
         (= :skip (:status (find-row rows "guard.fram-verbs-mountable")))))

(let [rows (guard-rows-with
            {:registry ["/etc/hostname" "/no/such/adopted/file.bclj"]
             :adopted ["/etc/hostname"] :fram-home "/fixture/fram"
             :advertised {:tools ["add-def" "set-body" "rename-def"]}})
      row (find-row rows "guard.graph-upstream-registry")]
  (check "a stale registry row is reported as lapsed adoption"
         (and (= :warn (:status row)) (str/includes? (:why row) "silently lapsed"))))

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
            {:presets [{:role "scout" :openai "read-only" :coordinationUnderReadOnly false}
                       {:role "integrator" :openai "workspace-write" :coordinationUnderReadOnly false}
                       {:role "director" :openai "read-only" :coordinationUnderReadOnly true}]})
      constraint (find-row rows "sandbox.openai")]
  (check "the sandbox expectation always prints as INFO, never a wall"
         (= :info (:status constraint)))
  (check "the sandbox expectation states the read-only worker constraint"
         (str/includes? (:why constraint) "READ-ONLY"))
  (check "the sandbox expectation is derived, listing each preset's real sandbox"
         (and (str/includes? (:why constraint) "read-only=director,scout")
              (str/includes? (:why constraint) "workspace-write=integrator")))
  (check "coordination inside the read-only sandbox is a named FAIL"
         (= :fail (:status (find-row rows "sandbox.openai-coordination")))))

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

(check "the canonical bespoke probe requests the sealed graph-authoring capability"
       (some #{"graph-authoring.fram"} (:capabilities doctor/canonical-graph-authoring-contract)))

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
            (doctor/row "guard" "guard.y" :impossible "no session can comply" "north config guards off")]
      payload (json/parse-string (str/trim (with-out-str (doctor/render! rows {:json? true}))) true)]
  (check "--json emits the versioned row contract"
         (= "north:spawn-doctor:v1" (:schema payload)))
  (check "--json counts an IMPOSSIBLE row as failing"
         (= 1 (:failing payload)))
  (check "--json carries the fix command for machine consumers"
         (= "north config guards off" (-> payload :rows second :fix))))

(let [rendered (with-out-str
                 (doctor/render! [(doctor/row "guard" "guard.y" :impossible "no session can comply"
                                              "north config guards off")]
                                 {:json? false}))]
  (check "the table prints the IMPOSSIBLE status and its fix line"
         (and (str/includes? rendered "IMPOSSIBLE")
              (str/includes? rendered "fix: north config guards off"))))

(let [pass (count (filter second @checks))]
  (println (format "spawn doctor: %d / %d PASS" pass (count @checks)))
  (System/exit (if (= pass (count @checks)) 0 1)))
