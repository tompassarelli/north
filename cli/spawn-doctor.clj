(ns north.spawn-doctor
  "`north spawn --doctor` — every dispatch invariant tested in one pass.

  Barrier discovery used to be serial: one fail-closed wall per dead lane, one
  lane per attempt. Each check here is independent and non-fatal, so a run
  reports EVERY wall standing right now, with the fix command for each."
  (:require [babashka.process :as p]
            [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.string :as str]))

;; Loaded either standalone or into agents-cli, which already holds these.
(doseq [[lib file] [['north.coord "/coord.clj"]
                    ['north.message-routing "/message-routing.clj"]
                    ['north.topology-authority "/topology-authority.clj"]]]
  (when-not (find-ns lib)
    (load-file (str (.getParent (io/file *file*)) file))))

(def HOME (System/getenv "HOME"))
(def NORTH (or (System/getenv "NORTH_HOME")
               (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str)))
(def NORTH-CLI (or (System/getenv "NORTH_BIN") (str NORTH "/bin/north")))
(def PORT (or (System/getenv "NORTH_PORT") "7977"))
(def PROBE-CLI (str NORTH "/sdk/src/spawn-doctor-probe.ts"))
(def PROVIDERS-CLI (str NORTH "/sdk/src/providers-cli.ts"))
(def BUN (or (System/getenv "NORTH_POLICY_BUN") "bun"))
(def GRAPH-UPSTREAM-REGISTRY
  (or (System/getenv "GRAPH_UPSTREAM_REGISTRY") (str HOME "/.config/fram/graph-upstream-files")))

;; The three verbs code-upstream-guard.sh's deny message redirects a refused
;; text edit to. A guard is only satisfiable if a session can mount all three.
(def guard-redirect-verbs ["add-def" "set-body" "rename-def"])

(def probe-timeout-ms 180000)
(def providers-timeout-ms 180000)
(def dry-run-timeout-ms 240000)
(def mcp-probe-timeout-ms 180000)
(def deep-coordinator-timeout-ms 45000)

(def color? (and (nil? (System/getenv "NO_COLOR")) (some? (System/console))))
(defn- c [code s] (if color? (str "\033[" code "m" s "\033[0m") (str s)))
(defn- dim [s] (c "2" s))
(defn- bold [s] (c "1" s))

(def status-render
  {:pass ["PASS" "32"] :fail ["FAIL" "31"] :skip ["SKIP" "2"]
   :warn ["WARN" "33"] :info ["INFO" "36"] :impossible ["IMPOSSIBLE" "35"]})

(defn- shell
  "Never let a probe die anonymously: a timeout and a spawn failure carry no
   subprocess output, and an empty :why reads as a passing check."
  [argv & {:keys [timeout in env] :or {timeout 30000}}]
  (try
    (let [proc (p/process argv (cond-> {:out :string :err :string}
                                 in (assoc :in in)
                                 env (assoc :env env)))
          res (deref proc timeout ::timeout)]
      (if (= res ::timeout)
        (do (p/destroy-tree proc)
            {:ok false :out "" :err (str "exceeded its " timeout "ms budget") :timeout true})
        {:ok (zero? (:exit res)) :exit (:exit res)
         :out (or (:out res) "") :err (or (:err res) "")}))
    (catch Exception e
      {:ok false :out "" :err (or (not-empty (str (.getMessage e))) (.getName (class e)))})))

(defn- env-without
  "A dry run creates no lane, so dropping the caller's worker topology is not
   forged authority — it is the absence of a lane to authorize."
  [& ks]
  (apply dissoc (into {} (System/getenv)) ks))

(defn- first-line [s]
  (-> (str s) str/split-lines first (or "") str/trim))

(defn- truncate [n s]
  (let [s (str/replace (str s) #"\s+" " ")]
    (if (> (count s) n) (str (subs s 0 (- n 1)) "…") s)))

;; ---- row engine ----------------------------------------------------------------

(defn row [section id status why & [fix]]
  (cond-> {:section section :check id :status status :why (str why)}
    fix (assoc :fix fix)))

(defn- failing? [{:keys [status]}] (contains? #{:fail :impossible} status))

;; ---- (1) closure integrity ------------------------------------------------------

(defn probe! []
  (let [result (shell [BUN "run" PROBE-CLI] :timeout probe-timeout-ms)]
    (if-not (:ok result)
      {:error (or (not-empty (first-line (:err result))) (str "exit " (:exit result)))}
      (try (json/parse-string (:out result) true)
           (catch Exception e {:error (str "probe emitted unreadable JSON: " (.getMessage e))})))))

(defn closure-rows [probe]
  (if (:error probe)
    [(row "closure" "closure.managed-codex-hooks" :fail
          (str "the SDK preflight's hook inventory could not be read: " (:error probe))
          (str "cd " NORTH "/sdk && bun run check"))]
    (let [{:keys [requirements runtime hooks]} (:managedCodexHooks probe)
          paths (concat runtime hooks)
          broken (remove #(#{"nix" "sealed"} (:supply %)) paths)
          fix "north rebuild request --why \"managed Codex hook fell out of the verified closure\""]
      (into
       [(if (:ok requirements)
          (row "closure" "closure.requirements" :pass
               (str (:path requirements) " is Nix-supplied and pins North's exact managed hook surface"))
          (row "closure" "closure.requirements" :fail
               (str (:path requirements) ": " (truncate 160 (:detail requirements))) fix))
        (if (seq broken)
          (row "closure" "closure.managed-codex-hooks" :fail
               (str (count broken) " of " (count paths)
                    " preflight-verified session hook paths are outside the verified closure"
                    " — every managed OpenAI lane dies at preflight") fix)
          (row "closure" "closure.managed-codex-hooks" :pass
               (str (count paths) " preflight-verified hook paths all resolve into the closure ("
                    (count (filter #(= "nix" (:supply %)) paths)) " nix, "
                    (count (filter #(= "sealed" (:supply %)) paths)) " sealed promotion)")))]
       (map (fn [{:keys [hook detail]}]
              (row "closure" (str "closure.hook:" hook) :fail (truncate 200 detail) fix))
            broken)))))

;; ---- (2) provider accounts ------------------------------------------------------

(defn providers! []
  (let [result (shell [BUN "run" PROVIDERS-CLI "--json"] :timeout providers-timeout-ms)]
    (if-not (:ok result)
      {:error (or (not-empty (first-line (:err result))) (str "exit " (:exit result)))}
      (try (json/parse-string (:out result) true)
           (catch Exception e {:error (str "providers --json emitted unreadable JSON: " (.getMessage e))})))))

(defn- usage-summary [usage]
  (if-not usage
    "no usage observation"
    (let [windows (->> (:windows usage)
                       (map #(str (:limitId %) "=" (:usedPercent %) "%"))
                       (str/join " "))]
      (str (:status usage) (when (:cached usage) " cached")
           (when (seq windows) (str " " windows))))))

(defn provider-rows [providers]
  (if (:error providers)
    [(row "providers" "providers.accounts" :fail
          (str "provider account eligibility could not be read: " (:error providers))
          "north providers --json")]
    (let [targets (mapcat :targets (:providers providers))
          eligible (filter #(= "eligible" (:routing %)) targets)]
      (into
       [(if (seq eligible)
          (row "providers" "providers.accounts" :pass
               (str (count eligible) " of " (count targets) " configured accounts are routing-eligible"
                    " (allocation=" (:allocationMode providers) ")"))
          (row "providers" "providers.accounts" :fail
               (str "no configured account is routing-eligible; every dispatch will fail to select a target")
               "north providers"))]
       (map (fn [{:keys [id provider routing headroom available availabilityReason usage]}]
              (let [status (cond (not available) :fail
                                 (= "eligible" routing) :pass
                                 :else :warn)]
                (row "providers" (str "providers." id) status
                     (str provider " routing=" routing " headroom=" headroom
                          (when-not available (str " unavailable=" availabilityReason))
                          " · " (usage-summary usage))
                     (when-not available (str "north accounts login " id)))))
            targets)))))

;; ---- (3) listener ---------------------------------------------------------------

(defn- invoking-agent-id []
  (some-> (or (System/getenv "AGENT_ID") (System/getenv "NORTH_AGENT_ID")) str/trim not-empty))

(defn listener-rows []
  (let [id (invoking-agent-id)]
    (if-not id
      [(row "listener" "listener.armed" :skip
            "no AGENT_ID/NORTH_AGENT_ID in this environment; nothing addressable to wake")]
      (let [route (north.message-routing/require-live-address (Integer/parseInt PORT) id)]
        [(cond
           (true? (:live route))
           (row "listener" "listener.armed" :pass
                (str id " has a live address; lane completions can wake it"))
           (= :unavailable (:live route))
           (row "listener" "listener.armed" :warn
                (str "liveness for " id " is unreadable (coordinator degraded): "
                     (truncate 120 (some-> route :error .getMessage)))
                (str "north listen " id))
           :else
           (row "listener" "listener.armed" :fail
                (str id " HAS NO ARMED LISTENER — spawned lanes will complete without waking it")
                (str "north listen " id)))]))))

;; ---- (4) composition dry-run ----------------------------------------------------

(def canonical-graph-authoring-contract
  {:responsibility "Author one adopted graph-upstream module through the sealed graph-edit verbs."
   :deliverable "One committed graph edit plus the regenerated downstream text view."
   :capabilities ["filesystem.read" "filesystem.search" "graph-authoring.fram"]
   :mayDecide ["form of the edit inside the adopted module"]
   :mustEscalate ["adopting or de-adopting a file" "any edit outside the adopted module"]
   :doneWhen ["the graph edit is committed and the regenerated view recompiles"]
   :report "the edited defs, the recompile result, and residual uncertainty"})

(defn- dry-run! [args]
  (shell (into [NORTH-CLI "spawn"] args)
         :timeout dry-run-timeout-ms
         ;; AGENT_TOPOLOGY: a worker may not spawn, but a dry run spawns nothing.
         :env (env-without "AGENT_TOPOLOGY")))

(defn- dry-run-row [id label args]
  (let [result (dry-run! args)
        reason (truncate 200 (or (not-empty (first-line (:err result)))
                                 (not-empty (last (remove str/blank? (str/split-lines (:out result)))))
                                 (str "exit " (:exit result))))]
    (if (:ok result)
      (row "dry-run" id :pass
           (str label " composes: "
                (truncate 140 (or (some #(when (str/includes? % "orchestration dials") %)
                                        (str/split-lines (:out result)))
                                  "accepted"))))
      (row "dry-run" id :fail (str label " is REJECTED before spawn: " reason)
           (str NORTH-CLI " spawn " (str/join " " (map pr-str args)))))))

(defn dry-run-rows []
  [(dry-run-row "dryrun.preset" "canonical preset (integrator)"
                ["integrator" "north spawn --doctor composition probe" "--dry-run" "--ad-hoc"])
   (dry-run-row "dryrun.bespoke-graph-authoring" "canonical bespoke graph-authoring contract"
                ["graph-author" "north spawn --doctor composition probe"
                 "--dry-run" "--ad-hoc"
                 "--rationale" "spawn --doctor canonical bespoke composition probe"
                 "--nearest" "implementer"
                 "--contract" (json/generate-string canonical-graph-authoring-contract)])])

;; ---- (5) graph-authoring roots --------------------------------------------------

(defn- fram-home [probe]
  (some-> probe :graphAuthoring :framHome not-empty))

(defn roots-rows [probe]
  (if (:error probe)
    [(row "roots" "roots.graph-authoring" :skip
          "graph-authoring roots were not probed (the SDK probe failed)")]
    (let [{:keys [framHome beagleHome rootsError checkouts framMcpCommand framMcpExecutable]}
          (:graphAuthoring probe)]
      (if rootsError
        [(row "roots" "roots.graph-authoring" :fail
              (str rootsError
                   " — no graph-authoring.fram lane can be composed from this environment")
              "export NORTH_FRAM_HOME=~/code/fram/main NORTH_BEAGLE_HOME=~/code/beagle/main")]
        (conj
         (mapv (fn [{:keys [name path exists isGitCheckout]}]
                 (cond
                   (not exists)
                   (row "roots" (str "roots." name) :fail
                        (str path " is not a directory") (str "export " name "=<checkout>"))
                   (not isGitCheckout)
                   (row "roots" (str "roots." name) :fail
                        (str path " is not a git checkout") (str "export " name "=<checkout>"))
                   :else (row "roots" (str "roots." name) :pass (str path " is a real checkout"))))
               checkouts)
         (if framMcpExecutable
           (row "roots" "roots.fram-mcp" :pass (str framMcpCommand " is executable"))
           (row "roots" "roots.fram-mcp" :fail
                (str (or framMcpCommand (str framHome "/bin/fram-mcp")) " is not executable")
                (str "ls -l " framHome "/bin/fram-mcp"))))))))

(defn deep-rows [probe]
  (let [{:keys [framDaemonCommand framDaemonExecutable]} (:graphAuthoring probe)]
    (cond
      (:error probe) [(row "deep" "deep.lane-coordinator" :skip "the SDK probe failed")]
      (not framDaemonExecutable)
      [(row "deep" "deep.lane-coordinator" :fail
            (str "the lane-local coordinator launcher " (or framDaemonCommand "fram-daemon")
                 " is not executable")
            "export NORTH_FRAM_HOME=<fram checkout>")]
      :else
      ;; The exact argv sdk/src/fram-graph-authoring.ts launches for a lane's
      ;; own coordinator; a scratch log keeps the smoke off every real corpus,
      ;; and an absent one proves a fresh store boots with no migration step.
      (let [dir (java.nio.file.Files/createTempDirectory
                 "north-spawn-doctor" (into-array java.nio.file.attribute.FileAttribute []))
            log (str dir "/code.log")
            port (str (+ 41000 (rand-int 4000)))
            space "north-spawn-doctor-probe"
            result (shell [framDaemonCommand "serve" port log space]
                          :timeout deep-coordinator-timeout-ms
                          :env (assoc (into {} (System/getenv)) "FRAM_REQUIRE_LOG_FENCE" "1"))]
        (doseq [leftover [log (str log ".writer-authority.lock") (str dir)]]
          (io/delete-file (io/file leftover) true))
        [(if (:timeout result)
           (row "deep" "deep.lane-coordinator" :pass
                (str framDaemonCommand " serve stayed up for the smoke budget on a fresh store"))
           (row "deep" "deep.lane-coordinator" :fail
                (str framDaemonCommand " serve exited " (:exit result)
                     ": " (truncate 160 (or (not-empty (first-line (:err result)))
                                            (first-line (:out result))))
                     " — every graph-authoring lane dies at coordinator boot")
                (str framDaemonCommand " serve " port " <log> <space-id>")))]))))

;; ---- (6) sandbox expectations ---------------------------------------------------

(defn sandbox-rows [probe]
  (if (:error probe)
    [(row "sandbox" "sandbox.openai" :skip "preset capabilities were not probed")]
    (let [presets (:presets probe)
          by (group-by :openai presets)
          read-only-no-network (filter #(and (= "read-only" (:openai %))
                                             (= "closed" (:sandboxNetwork %))) presets)
          closed-loopback (filter #(= "closed" (:directShellLoopback %)) presets)
          host-coordination (filter #(= "north-mcp-host" (:coordinationTransport %)) presets)
          names #(str/join "," (sort (map :role %)))]
      (cond-> [(row "sandbox" "sandbox.openai" :info
                    (str "Codex shell authority is derived per preset: read-only/no-network="
                         (names read-only-no-network) " · workspace-write="
                         (names (get by "workspace-write"))
                         " · direct-shell-loopback-closed=" (names closed-loopback)
                         (when-let [rejected (seq (get by "rejected"))]
                           (str " · openai-rejected=" (names rejected)))))]
        (seq host-coordination)
        (conj (row "sandbox" "sandbox.openai-coordination" :pass
                   (str "coordination uses the required host-side North MCP for "
                        (names host-coordination)
                        "; the read-only/no-network sandbox still keeps direct shell loopback closed")))))))

;; ---- (7) guard consistency ------------------------------------------------------

(defn- registry-paths []
  (let [file (io/file GRAPH-UPSTREAM-REGISTRY)]
    (when (.isFile file)
      (->> (str/split-lines (slurp file))
           (map str/trim)
           (remove #(or (str/blank? %) (str/starts-with? % "#")))
           (map #(str/replace-first % #"^~" (str HOME)))
           vec))))

;; ---- graph-upstream registry repair ---------------------------------------------
;; Rows are bare absolute paths, so a checkout that moved cannot be matched by
;; provenance either: the guard's git probe needs the row's PARENT DIRECTORY to
;; still exist. Repair therefore rewrites the rows.

(def ^:private sentinel-markers
  ["@upstream:graph" "@upstream-is-graph" "@claim-canonical"])

(defn- sentinel-directive? [line]
  (boolean (re-find (re-pattern (str ";+\\s*(?:"
                                     (str/join "|" (map #(java.util.regex.Pattern/quote %)
                                                        sentinel-markers))
                                     ")(?:\\s|$)"))
                    line)))

(defn- carries-sentinel?
  "Mirror of code-upstream-guard.sh's in-band check: only a leading `;;` comment
   before the first real form counts. Kept in sync with that guard by hand."
  [path]
  (let [file (io/file path)]
    (and (.isFile file)
         (with-open [reader (io/reader file)]
           (loop [scanned 0]
             (if (>= scanned 65536)
               false
               (if-let [line (.readLine ^java.io.BufferedReader reader)]
                 (let [s (str/trim line)]
                   (cond
                     (or (str/blank? s)
                         (re-matches #"(?:#lang\s+\S+|\(define-target\s+\S+\))" s))
                     (recur (+ scanned (count line) 1))

                     (str/starts-with? s ";;")
                     (if (sentinel-directive? s) true (recur (+ scanned (count line) 1)))

                     :else false))
                 false)))))))

(defn- relocations
  "~/code/<project>/… and ~/code/client/<owner>/<project>/… both gained a `main/`
   checkout segment when the container layout landed."
  [path]
  (let [code (str HOME "/code/")
        client (str code "client/")]
    (cond
      (str/starts-with? path client)
      (let [[owner project & rest] (str/split (subs path (count client)) #"/")]
        (when (seq rest)
          [(str client owner "/" project "/main/" (str/join "/" rest))]))

      (str/starts-with? path code)
      (let [[project & rest] (str/split (subs path (count code)) #"/")]
        (when (seq rest)
          [(str code project "/main/" (str/join "/" rest))]))

      :else nil)))

(defn registry-repair-plan
  "Classify every row: a resolving row is left alone, a stale row is revived only
   when its relocated file still carries the graph-upstream sentinel, and every
   other stale row is retired rather than re-adopted."
  [paths]
  (mapv (fn [path]
          (cond
            (.exists (io/file path)) {:row path :action :keep}
            :else
            (if-let [moved (first (filter #(.isFile (io/file %)) (relocations path)))]
              (if (carries-sentinel? moved)
                {:row path :action :rewrite :to moved}
                {:row path :action :retire
                 :reason (str "moved to " moved ", which no longer carries the sentinel")})
              {:row path :action :retire :reason "no file at this path or the current layout"})))
        paths))

(defn repair-registry!
  "One-shot: rewrite GRAPH-UPSTREAM-REGISTRY from the repair plan, keeping a .bak."
  []
  (let [paths (registry-paths)]
    (cond
      (nil? paths)
      (do (println (c "33" "no registry") (str "nothing to repair at " GRAPH-UPSTREAM-REGISTRY)) 0)

      :else
      (let [plan (registry-repair-plan paths)
            kept (->> plan (remove #(= :retire (:action %))) (mapv #(or (:to %) (:row %))))
            changed (filter #(not= :keep (:action %)) plan)]
        (println (bold "north spawn --doctor --repair-registry") (dim GRAPH-UPSTREAM-REGISTRY))
        (doseq [{:keys [row action to reason]} plan]
          (println (format "%-8s %s%s"
                           (name action) row
                           (case action
                             :rewrite (str "\n         -> " to)
                             :retire (str "\n         retired: " reason)
                             ""))))
        (if (empty? changed)
          (do (println (c "32" (str "all " (count paths) " rows already resolve; nothing rewritten"))) 0)
          (do
            (spit (str GRAPH-UPSTREAM-REGISTRY ".bak") (slurp GRAPH-UPSTREAM-REGISTRY))
            (spit GRAPH-UPSTREAM-REGISTRY (str (str/join "\n" kept) "\n"))
            (println (c "32" (str (count (filter #(= :rewrite (:action %)) plan)) " rewritten, "
                                  (count (filter #(= :retire (:action %)) plan)) " retired, "
                                  (count kept) " rows remain"))
                     (dim (str "backup: " GRAPH-UPSTREAM-REGISTRY ".bak")))
            0))))))

(defn- nearest-existing-dir [path]
  (loop [file (some-> path io/file .getParentFile)]
    (cond (nil? file) nil
          (.isDirectory file) (str file)
          :else (recur (.getParentFile file)))))

(defn- git-toplevel [path]
  (when-let [dir (nearest-existing-dir path)]
    (let [result (shell ["git" "-C" dir "rev-parse" "--show-toplevel"] :timeout 5000)]
      (when (:ok result) (not-empty (str/trim (:out result)))))))

(defn- sentinel-candidates [repos]
  (reduce (fn [acc repo]
            (let [result (shell ["git" "-C" repo "grep" "-l" "-F" "@upstream:graph"] :timeout 30000)]
              (assoc acc repo (if (:ok result)
                                (mapv #(str repo "/" %)
                                      (remove str/blank? (str/split-lines (:out result))))
                                []))))
          {} repos))

(def guard-script
  (first (filter #(.canExecute (io/file %))
                 [(str HOME "/.agents/hooks/code-upstream-guard.sh")
                  (str NORTH "/profiles/tom/hooks/code-upstream-guard.sh")])))

(def max-guard-probes 256)

(defn- guard-denies?
  "Ask the guard itself instead of restating its adoption rule. AGENT_NO_AUTHORING_HOOKS=0
   forces guards live so an operator kill-switch cannot read as 'nothing is adopted'."
  [path]
  (when guard-script
    (let [result (shell [guard-script] :timeout 20000
                        :in (json/generate-string
                             {:tool_name "Edit" :tool_input {:file_path path}})
                        :env (assoc (into {} (System/getenv)) "AGENT_NO_AUTHORING_HOOKS" "0"))
          decision (try (-> (:out result) (json/parse-string true)
                            :hookSpecificOutput :permissionDecision)
                        (catch Exception _ nil))]
      (= "deny" decision))))

(defn- advertised-fram-tools [fram-root]
  (let [command (str fram-root "/bin/fram-mcp")
        request (str (json/generate-string
                      {:jsonrpc "2.0" :id 1 :method "initialize"
                       :params {:protocolVersion "2024-11-05" :capabilities {}
                                :clientInfo {:name "north-spawn-doctor" :version "1"}}})
                     "\n"
                     (json/generate-string {:jsonrpc "2.0" :id 2 :method "tools/list" :params {}})
                     "\n")
        result (shell [command] :timeout mcp-probe-timeout-ms :in request
                      :env (assoc (into {} (System/getenv))
                                  "FRAM_SPACE_ID" "north-spawn-doctor-probe"))]
    (if-not (or (:ok result) (seq (:out result)))
      {:error (truncate 160 (or (not-empty (first-line (:err result))) (str "exit " (:exit result))))}
      (or (some (fn [line]
                  (let [parsed (try (json/parse-string line true) (catch Exception _ nil))]
                    (when-let [tools (get-in parsed [:result :tools])]
                      {:tools (mapv :name tools)})))
                (str/split-lines (:out result)))
          {:error "fram-mcp answered tools/list with no tool catalog"}))))

(defn- scan-repos [paths probe]
  (->> (concat (keep git-toplevel paths)
               (keep git-toplevel [(str (System/getProperty "user.dir") "/.")])
               (keep #(some-> % (str "/.") git-toplevel)
                     [(fram-home probe) (some-> probe :graphAuthoring :beagleHome not-empty)]))
       (remove nil?) distinct sort vec))

(defn guard-rows [probe]
  (let [paths (registry-paths)
        resolving (filter #(.exists (io/file %)) paths)
        stale (remove #(.exists (io/file %)) paths)
        repos (scan-repos paths probe)
        sentinels (sentinel-candidates repos)
        candidates (->> (concat resolving (mapcat val sentinels))
                        distinct (take max-guard-probes) vec)
        refused (filterv guard-denies? candidates)
        adopted (count refused)
        fram-root (or (fram-home probe)
                      (first (filter #(.exists (io/file % "bin/fram-mcp")) repos)))
        advertised (when (and fram-root (pos? adopted)) (advertised-fram-tools fram-root))
        unmountable (when (:tools advertised) (remove (set (:tools advertised)) guard-redirect-verbs))]
    [(cond
       (empty? paths)
       (row "guard" "guard.graph-upstream-registry" :skip
            (str "no registry rows at " GRAPH-UPSTREAM-REGISTRY))
       (seq stale)
       (row "guard" "guard.graph-upstream-registry" :warn
            (str (count stale) " of " (count paths) " registry rows name files that no longer exist"
                 " — realpath and git-provenance matching both miss, so their adoption has"
                 " silently lapsed (e.g. " (first stale) ")")
            (str NORTH-CLI " spawn --doctor --repair-registry"))
       :else
       (row "guard" "guard.graph-upstream-registry" :pass
            (str "all " (count paths) " registry rows resolve to real files")))

     (cond
       (nil? guard-script)
       (row "guard" "guard.graph-upstream-adoption" :skip
            "code-upstream-guard.sh is not installed; nothing enforces graph upstream here")
       (empty? repos)
       (row "guard" "guard.graph-upstream-adoption" :skip
            "no scannable git checkout was resolvable from the registry, cwd, or the roots")
       :else
       (row "guard" "guard.graph-upstream-adoption" :info
            (str adopted " of " (count candidates) " candidates are actually refused by "
                 guard-script " (registry rows + in-band @upstream:graph across "
                 (count repos) " repos: "
                 (str/join " " (map (fn [[repo hits]] (str (.getName (io/file repo)) "=" (count hits)))
                                    (sort-by key sentinels)))
                 ")")))

     (cond
       (zero? adopted)
       (row "guard" "guard.fram-verbs-mountable" :skip
            "no file is currently graph-upstream, so the guard can refuse nothing")

       (nil? fram-root)
       (row "guard" "guard.fram-verbs-mountable" :impossible
            (str "IMPOSSIBLE CONSTRAINT — the guard refuses text edits to " adopted
                 " adopted files and redirects to mcp__fram__"
                 (str/join "/mcp__fram__" guard-redirect-verbs)
                 ", but no fram checkout is resolvable to serve them, so no session can comply")
            "export NORTH_FRAM_HOME=<fram checkout>")

       (:error advertised)
       (row "guard" "guard.fram-verbs-mountable" :fail
            (str fram-root "/bin/fram-mcp could not be queried: " (:error advertised))
            (str "FRAM_SPACE_ID=probe " fram-root "/bin/fram-mcp"))

       (seq unmountable)
       (row "guard" "guard.fram-verbs-mountable" :impossible
            (str "IMPOSSIBLE CONSTRAINT — the guard refuses text edits to " adopted
                 " adopted files and redirects to verbs the server refuses to advertise: "
                 (str/join "," (map #(str "mcp__fram__" %) unmountable))
                 ". " fram-root "/bin/fram-mcp serves only " (str/join "," (:tools advertised))
                 ", so NO session can hold the capability the guard demands")
            (str "north config guards off  # or de-adopt from " GRAPH-UPSTREAM-REGISTRY))

       :else
       (row "guard" "guard.fram-verbs-mountable" :pass
            (str "every verb the guard redirects to is advertised by " fram-root "/bin/fram-mcp")))]))

;; ---- rendering ------------------------------------------------------------------

(defn render! [rows {:keys [json?]}]
  (if json?
    (println (json/generate-string
              {:schema "north:spawn-doctor:v1"
               :at (str (java.time.Instant/now))
               :rows rows
               :failing (count (filter failing? rows))}))
    (let [width (apply max 24 (map (comp count :check) rows))]
      (println (bold "north spawn --doctor") (dim (str (java.time.Instant/now))))
      (println)
      (doseq [group (partition-by :section rows)]
        (doseq [{:keys [check status why fix]} group]
          (let [[label code] (status-render status)]
            (println (format (str "%-10s %-" width "s %s")
                             (c code label) check (truncate 150 why)))
            (when (and fix (contains? #{:fail :warn :impossible} status))
              (println (dim (format (str "%-10s %-" width "s fix: %s") "" "" fix))))))
        (println)))))

;; ---- canary ---------------------------------------------------------------------

(def canary-poll-ms 15000)
(def canary-budget-ms (* 12 60 1000))

(defn- agent-fact [id predicate]
  (try (north.coord/resolved (Integer/parseInt PORT) (str "@agent:" id) predicate)
       (catch Exception _ nil)))

(defn canary! []
  (north.topology-authority/require-coordination! "spawn --doctor --canary")
  (println (bold "north spawn --doctor --canary") (dim "one read-only managed lane, end to end"))
  (let [prompt (str "Read-only dispatch canary. Reply with exactly: canary-ok. "
                    "Do not write files, do not spawn, do not coordinate.")
        started (System/currentTimeMillis)
        result (shell [NORTH-CLI "spawn" "scout" prompt "--ad-hoc"]
                      :timeout dry-run-timeout-ms :env (env-without "AGENT_TOPOLOGY"))
        out (str (:out result) (:err result))
        control (second (re-find #"control:\s+(\S+)" out))]
    (println out)
    (cond
      (not control)
      (do (println (c "31" "FAIL") "canary never reached an identity; no control id was published")
          1)
      :else
      (loop []
        (let [outcome (agent-fact control "outcome")
              elapsed (- (System/currentTimeMillis) started)]
          (cond
            outcome
            (do (println (c "32" "PASS") "canary lifecycle complete"
                         (str "control=" control " outcome=" outcome
                              " wall=" (quot elapsed 1000) "s"))
                (println (dim (str "trace: north trace " control)))
                (let [trace (shell [NORTH-CLI "trace" control] :timeout 120000)]
                  (println (:out trace)))
                0)
            (> elapsed canary-budget-ms)
            (do (println (c "31" "FAIL") "canary never reached a terminal outcome inside"
                         (str (quot canary-budget-ms 60000) "m; control=" control))
                (println (dim (str "trace: north trace " control)))
                1)
            :else (do (Thread/sleep canary-poll-ms) (recur))))))))

;; ---- entry ----------------------------------------------------------------------

(def usage
  (str "usage: north spawn --doctor [--deep] [--json]\n"
       "       north spawn --doctor --canary\n"
       "       north spawn --doctor --repair-registry\n\n"
       "  --deep    also run the lane-local coordinator launch smoke\n"
       "  --canary  actually spawn one tiny read-only managed lane end to end\n"
       "  --json    emit the versioned north:spawn-doctor:v1 row contract\n"
       "  --repair-registry  relocate graph-upstream rows to the current checkout\n"
       "                     layout and retire the ones no sentinel still backs"))

(defn run! [args]
  (let [flags (set args)
        unknown (remove #{"--doctor" "--deep" "--json" "--canary" "--repair-registry"} args)]
    (cond
      (seq unknown)
      (do (binding [*out* *err*] (println (c "31" (str "unknown doctor option: " (first unknown)))))
          (println usage)
          2)
      (flags "--repair-registry") (repair-registry!)
      (flags "--canary") (canary!)
      :else
      ;; The three subprocess-heavy probes are independent; running them serially
      ;; is the same "one wall at a time" shape the doctor exists to replace.
      (let [probe-f (future (probe!))
            providers-f (future (providers!))
            dry-f (future (dry-run-rows))
            probe @probe-f
            rows (vec (concat (closure-rows probe)
                              (provider-rows @providers-f)
                              (listener-rows)
                              @dry-f
                              (roots-rows probe)
                              (when (flags "--deep") (deep-rows probe))
                              (sandbox-rows probe)
                              (guard-rows probe)))]
        (render! rows {:json? (contains? flags "--json")})
        (let [broken (filter failing? rows)]
          (when-not (contains? flags "--json")
            (println (if (seq broken)
                       (c "31" (str (count broken) " of " (count rows) " checks are walls right now"))
                       (c "32" (str "all " (count rows) " checks clear"))))
            (when-not (flags "--deep")
              (println (dim "  --deep adds the lane-local coordinator launch smoke"))))
          (if (seq broken) 1 0))))))
