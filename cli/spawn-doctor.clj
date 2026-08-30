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

(def NORTH (or (System/getenv "NORTH_HOME")
               (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str)))
(def NORTH-CLI (or (System/getenv "NORTH_BIN") (str NORTH "/bin/north")))
(def PORT (or (System/getenv "NORTH_PORT") "7977"))
(def PROBE-CLI (str NORTH "/sdk/src/spawn-doctor-probe.ts"))
(def PROVIDERS-CLI (str NORTH "/sdk/src/providers-cli.ts"))
(def BUN (or (System/getenv "NORTH_POLICY_BUN") "bun"))
(def probe-timeout-ms 180000)
(def providers-timeout-ms 180000)
(def dry-run-timeout-ms 240000)

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
          fix "firn rebuild"]
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
                (str "liveness for " id " is unreadable (Beagle Store server degraded): "
                     (truncate 120 (some-> route :error .getMessage)))
                (str "north listen " id))
           :else
           (row "listener" "listener.armed" :fail
                (str id " HAS NO ARMED LISTENER — spawned lanes will complete without waking it")
                (str "north listen " id)))]))))

;; ---- (4) composition dry-run ----------------------------------------------------

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
                ["integrator" "north spawn --doctor composition probe" "--dry-run" "--ad-hoc"])])

;; ---- (5) sandbox expectations ---------------------------------------------------

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
  (north.coord/resolved! (Integer/parseInt PORT) (str "@agent:" id) predicate))

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
  (str "usage: north spawn --doctor [--json]\n"
       "       north spawn --doctor --canary\n"
       "\n"
       "  --canary  actually spawn one tiny read-only managed lane end to end\n"
       "  --json    emit the versioned north:spawn-doctor:v1 row contract"))

(defn run! [args]
  (let [flags (set args)
        unknown (remove #{"--doctor" "--json" "--canary"} args)]
    (cond
      (seq unknown)
      (do (binding [*out* *err*] (println (c "31" (str "unknown doctor option: " (first unknown)))))
          (println usage)
          2)
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
                              (sandbox-rows probe)))]
        (render! rows {:json? (contains? flags "--json")})
        (let [broken (filter failing? rows)]
          (when-not (contains? flags "--json")
            (println (if (seq broken)
                       (c "31" (str (count broken) " of " (count rows) " checks are walls right now"))
                       (c "32" (str "all " (count rows) " checks clear")))))
          (if (seq broken) 1 0))))))
