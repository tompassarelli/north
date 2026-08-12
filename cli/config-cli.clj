#!/usr/bin/env bb
;; north config — every personal-stack posture setting, one entry point.
;;
;;   dispatch : who runs agents        managed North SDK  vs  native Agent/Workflow
;;   coord    : coordination protocol  north / linear / both
;;   guards   : authoring-guard hooks  + the kill-switch
;;   context  : native prompt sections full      vs  gated
;;   skills   : shared skill discovery complete set vs resolved projection
;;   comms    : peer mail protocol      off / db / file / both
;;
;; Ported from dotfiles/bin/my-agent-config (bash) 2026-07-10: north is the
;; top-level settings surface. Output contract is byte-faithful to the bash tool
;; (self-references now read `north config`); the slash command renders it verbatim.
;;
;; Provider-neutral posture state lives at ~/.local/state/north/harness.conf.
;; ~/.claude/my-config.state is a read-only migration fallback until the first
;; canonical write. The kill-switch precedence below is a faithful inline copy
;; of hooks/lib/authoring-killswitch.sh so report and enforcement agree.

(require '[clojure.string :as str]
         '[clojure.java.io :as io]
         '[clojure.java.shell :as shell]
         '[cheshire.core :as json])

(def home (System/getenv "HOME"))
(load-file (str (or (System/getenv "NORTH_HOME")
                    (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
                "/cli/harness-state.clj"))
(load-file (str (or (System/getenv "NORTH_HOME")
                    (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
                "/cli/harness-dial.clj"))
(def STATE           (north.harness-state/canonical-path home))
(def LEGACY-STATE    (north.harness-state/legacy-path home))
(def HOOK-REGISTRY   (north.harness-dial/registry-path home))
(def ROUTING-POLICY  (or (System/getenv "NORTH_ROUTING_POLICY")
                         (str home "/.config/north/routing-policy.json")))
(def LEARNING-POLICY (or (System/getenv "NORTH_LEARNING_POLICY")
                         (str home "/.config/north/learning-policy.json")))
(def CONTEXT-SOURCE  (or (System/getenv "NORTH_CONTEXT_SOURCE")
                         (str home "/.agents/AGENTS.md")))
(def CONTEXT-OUTPUT  (or (System/getenv "NORTH_CONTEXT_OUTPUT")
                         (str home "/.claude/CLAUDE.md")))
(def SKILLS-PROFILE  (or (System/getenv "NORTH_SKILLS_PROFILE")
                         (str (or (System/getenv "WORLD_REPO_NORTH")
                                  (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
                              "/agent-profile/skills")))
(def SKILLS-FARM     (or (System/getenv "NORTH_SKILLS_FARM")
                         (str home "/.local/state/north/skills")))
(def SKILLS-GENERATIONS (str SKILLS-FARM ".d"))
(def SKILLS-LOCK     (str SKILLS-FARM ".lock"))
(def COMMS-BIN       (or (System/getenv "NORTH_COMMS_BIN")
                         (str (or (System/getenv "NORTH_HOME")
                                  (some-> *file* io/file .getCanonicalFile
                                          .getParentFile .getParentFile str))
                              "/bin/north-comms")))
(def CONTEXT-BUCKETS #{"core" "write" "shell" "orch" "client" "nixos" "beagle"})
(def CONTEXT-TAG
  #"^<!-- north-section: ([a-z0-9][a-z0-9-]*) · bucket: (core|write|shell|orch|client|nixos|beagle) -->$")

(def learning-axes ["model-tier" "effort" "prompt" "authoring" "history"])
(def learning-axis-set (set learning-axes))
(def default-learning-policy
  {:version 1 :mode "frozen" :intensity 0.1 :axes learning-axes
   :maxTierDelta 1 :riskCeiling "p1" :seed "north-default" :epoch "1"
   :evidenceMode "discovery"})

(defn- slurp' [f] (try (slurp f) (catch Exception _ nil)))
(defn- eprintln [& xs] (binding [*out* *err*] (apply println xs)))
(defn- die [& xs] (apply eprintln xs) (System/exit 1))

(defn- dispatch-mode [] (north.harness-state/get-dispatch-mode home))

;; --- state accessors (key=value lines; last wins) --------------------------
(defn get' [k default]
  (north.harness-state/get-value home k default))

(defn put' [k v]
  (north.harness-state/put-value! home k v))

(defn mark [a b] (if (= a b) "●" "○")) ; ● / ○

(defn dispatch-status-lines [selected]
  (str/join
   "\n"
   (map (fn [{:keys [name summary]}]
          (format "    %s %-16s %s" (mark selected name) name summary))
        (reverse north.dispatch-mode/mode-specs))))

(defn dispatch-help-lines []
  (str/join
   "\n"
   (map (fn [{:keys [name help]}]
          (format "   %-16s %s" name help))
        (reverse north.dispatch-mode/mode-specs))))

;; --- environment probes ---------------------------------------------------
(defn north-daemon []
  (try (with-open [s (java.net.Socket.)]
         (.connect s (java.net.InetSocketAddress. "127.0.0.1" 7977) 300))
       "reachable (corpus health is checked by `north doctor`)"
       (catch Exception _ "DOWN")))

(defn linear-mcp []
  (let [c (slurp' (str home "/.claude.json"))]
    (if (and c (str/includes? c "linear")) "configured" "absent")))

(def CLAUDE-MCP-CONFIG (or (System/getenv "NORTH_CLAUDE_MCP_CONFIG")
                           (str home "/.claude.json")))
(def CLAUDE-SETTINGS (or (System/getenv "NORTH_CLAUDE_SETTINGS")
                         (str home "/.claude/settings.json")))
(def CODEX-CONFIG (or (System/getenv "NORTH_CODEX_CONFIG")
                      (str (or (System/getenv "CODEX_HOME") (str home "/.codex"))
                           "/config.toml")))
(def CONFIG-DRIFT-AUDIT
  (str (or (System/getenv "NORTH_HOME")
           (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
       "/cli/config-drift-audit.py"))

(def mcp-usage
  "usage: north config mcp [list [--json]|add <name> <url>|add <name> -- <command> [args...]|remove <name>]\nMCP declarations are applied to both Claude (user scope) and Codex.")

(declare print-provider-readouts)

(defn- run-provider! [& argv]
  (let [{:keys [exit out err]} (apply shell/sh argv)]
    (when-not (zero? exit)
      (throw (ex-info (str (str/join " " argv) " failed: "
                           (str/trim (if (str/blank? err) out err))) {})))
    out))

(defn- run-config-drift-audit! [& argv]
  (let [{:keys [exit out err]}
        (apply shell/sh "python3" CONFIG-DRIFT-AUDIT argv)]
    (when-not (str/blank? out) (print out))
    (when-not (str/blank? err) (binding [*out* *err*] (print err)))
    (when-not (zero? exit) (System/exit exit))))

(defn cmd-mcp [args]
  (let [[verb name target & extra] args]
    (case (or verb "list")
      "list" (do
               (when (or target (seq extra) (and name (not= name "--json")))
                 (die mcp-usage))
               (apply run-config-drift-audit!
                      (cond-> ["--section" "mcp"]
                        (= name "--json") (conj "--json"))))
      "add" (do
              (when (or (str/blank? name) (str/blank? target))
                (die mcp-usage))
              (if (= target "--")
                (do
                  (when-not (seq extra) (die mcp-usage))
                  (apply run-provider! "claude" "mcp" "add" "--scope" "user"
                         name "--" extra)
                  (apply run-provider! "codex" "mcp" "add" name "--" extra)
                  (println (str name " → shared Claude/Codex stdio MCP")))
                (do
                  (when (or (seq extra) (not (re-matches #"https?://.+" target)))
                    (die mcp-usage))
                  (run-provider! "claude" "mcp" "add" "--scope" "user"
                                 "--transport" "http" name target)
                  (run-provider! "codex" "mcp" "add" name "--url" target)
                  (println (str name " → shared Claude/Codex MCP (" target ")")))))
      "remove" (do
                 (when (or (str/blank? name) target (seq extra)) (die mcp-usage))
                 (run-provider! "claude" "mcp" "remove" "--scope" "user" name)
                 (run-provider! "codex" "mcp" "remove" name)
                 (println (str name " removed from Claude and Codex MCP")))
      (die mcp-usage))))

(defn- json-at [path]
  (when-let [text (slurp' path)]
    (try (json/parse-string text false) (catch Exception _ nil))))

(defn- map-names [value]
  (sort (if (map? value) (map str (keys value)) [])))

(defn- toml-section-names [path section]
  (if-let [text (slurp' path)]
    (->> (str/split-lines text)
         (keep #(second (re-matches (re-pattern (str "^\\[" section "\\.([^]]+)\\]$")) %)))
         sort)
    []))

(defn- print-provider-readouts []
  (let [claude-mcp (map-names (get (json-at CLAUDE-MCP-CONFIG) "mcpServers"))
        codex-mcp (toml-section-names CODEX-CONFIG "mcp_servers")
        claude-plugins (map-names (get (json-at CLAUDE-SETTINGS) "enabledPlugins"))
        codex-plugins (toml-section-names CODEX-CONFIG "plugins")
        render (fn [provider kind names command path]
                 (println (str "    " provider " " kind ": " path))
                 (if (seq names)
                   (doseq [name names]
                     (println (str "      " name " → " (format command name))))
                   (println "      (none declared)")))]
    (println "\n10 PROVIDER MCP  provider-owned declarations")
    (render "Claude" "MCP" claude-mcp "claude mcp remove %s" CLAUDE-MCP-CONFIG)
    (render "Codex" "MCP" codex-mcp "codex mcp remove %s" CODEX-CONFIG)
    (println "\n11 PROVIDER PLUGINS  provider-owned installations")
    (render "Claude" "plugin" claude-plugins "claude plugin uninstall %s" CLAUDE-SETTINGS)
    (render "Codex" "plugin" codex-plugins "codex plugin uninstall %s" CODEX-CONFIG)))

(defn hook-registry []
  (north.harness-dial/read-registry HOOK-REGISTRY))

(defn- hook-entry [id]
  (some #(when (= id (:id %)) %) (hook-registry)))

(defn- hook-file [{:keys [path]}]
  (let [registry-file (.getCanonicalFile (io/file HOOK-REGISTRY))
        candidate (io/file path)]
    (.getCanonicalFile
     (if (.isAbsolute candidate)
       candidate
       (io/file (.getParentFile registry-file) path)))))

(defn- hook-path-status [hook]
  (let [file (hook-file hook)]
    (cond
      (not (.exists file)) "MISSING"
      (.canExecute file) "EXEC"
      :else "NONEXEC")))

(defn wired [id]
  (if-let [hook (hook-entry id)]
    (if (= "EXEC" (hook-path-status hook)) "✓" "✗")
    "✗")) ; ✓ / ✗

;; Kill-switch effective state — precedence identical to authoring-killswitch.sh:
;;   env 0|false  → force-live (state ignored this session)
;;   env non-empty (other) → engaged this session
;;   unset/empty  → state file `guards=off` decides
;; Delegates to the shared resolver rather than keeping a second copy. The
;; copy this replaced read only CLAUDE_NO_AUTHORING_HOOKS, so a session
;; launched with the canonical AGENT_NO_AUTHORING_HOOKS ran with guards
;; disabled while this report cheerfully printed "guards LIVE".
(defn effective-ks []
  (case (north.harness-dial/authoring-env)
    "on"  "env force-live — guards LIVE (state ignored this session)"
    "off" "ENGAGED via env (this session) — authoring guards OFF; dispatch topology unchanged"
    (if (= "off" (get' "guards" ""))
      "ENGAGED via state — authoring guards OFF; dispatch topology unchanged (north config guards on restores)"
      "off — guards LIVE")))

(defn today []
  (.format (java.time.LocalDate/now)
           (java.time.format.DateTimeFormatter/ofPattern "yyyy-MM-dd")))

;; --- provider routing policy ---------------------------------------------
;; This file is deliberately separate from the legacy key=value posture state:
;; it is structured, provider-neutral input consumed by the SDK router. Named
;; targets make account profiles representable without pretending the current
;; adapters can select between profiles yet.
(def default-routing-policy
  {:schemaVersion 1
   :mode "balanced"
   :targets {"anthropic" {:provider "anthropic" :authMode "ambient"}
             "openai" {:provider "openai" :authMode "ambient"}}
   :order ["anthropic" "openai"]
   :weights {"anthropic" 1 "openai" 1}
   :reserve nil
   :pressure {}
   :envelopes {}})

(defn- portable-profile-slug? [value]
  (boolean (re-matches #"[a-z0-9][a-z0-9_-]{0,63}" (or value ""))))

(defn- validate-routing [p]
  (let [ids (set (keys (:targets p)))
        refs (concat (:order p) (keys (:weights p)) (keys (:pressure p))
                     (when-let [r (:reserve p)] [r]))
        isolated-roots (for [[_ {:keys [provider authMode profile]}] (:targets p)
                             :when (= "isolated" authMode)]
                         [provider profile])
        ambient-providers (for [[_ {:keys [provider authMode]}] (:targets p)
                                :when (not= "isolated" authMode)]
                            provider)
        dangling (seq (remove ids refs))]
    (when-not (= 1 (:schemaVersion p))
      (throw (ex-info (str "unsupported schemaVersion " (:schemaVersion p)) {})))
    (when-not (contains? #{"preferential" "balanced" "reserved"} (:mode p))
      (throw (ex-info (str "invalid mode " (:mode p)) {})))
    (when-let [[id target] (first (remove #(contains? #{"anthropic" "openai"} (:provider (val %))) (:targets p)))]
      (throw (ex-info (str "target " id " has invalid provider " (:provider target)) {})))
    (when-let [[id target] (first (remove #(contains? #{nil "ambient" "isolated"} (:authMode (val %))) (:targets p)))]
      (throw (ex-info (str "target " id " has invalid authMode " (:authMode target)) {})))
    (when-let [[id target] (first (filter #(and (= "isolated" (:authMode (val %)))
                                                (not (portable-profile-slug? (:profile (val %)))))
                                          (:targets p)))]
      (throw (ex-info (str "target " id " requires a portable profile slug when authMode is isolated") {})))
    (when (some #(> (val %) 1) (frequencies isolated-roots))
      (throw (ex-info "isolated targets must not reuse the same provider/profile root" {})))
    (when (some #(> (val %) 1) (frequencies ambient-providers))
      (throw (ex-info "ambient targets must not reuse the same provider account" {})))
    (when dangling
      (throw (ex-info (str "dangling target reference(s): " (str/join ", " dangling)) {})))
    p))

(defn- flatten-envelopes [value]
  (let [limits (fn [m] (into {} (map (fn [[k v]] [(keyword k) v])) (or m {})))
        direct (into {} (keep (fn [scope]
                                (when-let [v (get value scope)] [scope (limits v)])))
                     ["default" "month" "week"])
        named (fn [kind]
                (into {} (map (fn [[id v]] [(str kind ":" id) (limits v)]))
                      (get value (str kind "s") {})))]
    (merge direct (named "project") (named "session"))))

(defn- nest-envelopes [value]
  (let [direct (into {} (keep (fn [scope] (when-let [v (get value scope)] [(keyword scope) v])))
                     ["default" "month" "week"])
        named (fn [kind]
                (into {} (keep (fn [[scope limits]]
                                 (when (str/starts-with? scope (str kind ":"))
                                   [(subs scope (inc (count kind))) limits]))) value))]
    (cond-> direct
      (seq (named "project")) (assoc :projects (named "project"))
      (seq (named "session")) (assoc :sessions (named "session")))))

(defn- routing-read []
  (if-let [raw (slurp' ROUTING-POLICY)]
    (try
      (let [j (json/parse-string raw false)]
        (validate-routing
          (merge default-routing-policy
               {:schemaVersion (get j "version" 1)
                :mode (get j "mode" "balanced")
                :targets (into {} (map (fn [v]
                                         [(get v "id") (cond-> {:provider (get v "provider")}
                                                         (get v "authMode") (assoc :authMode (get v "authMode"))
                                                         (get v "profile") (assoc :profile (get v "profile")))]))
                               (get j "targets" []))
                :order (vec (get j "targetOrder" (map #(get % "id") (get j "targets" []))))
                :weights (get j "weights" {})
                :reserve (get j "reservedFrontierTarget")
                :pressure (into {} (map (fn [[id v]] [id (cond-> {:level (get v "level")
                                                                   :observedAt (get v "observedAt")}
                                                            (get v "until") (assoc :until (get v "until")))]))
                                (into {} (map (fn [[id v]] [id (assoc v "level" (get v "state"))]))
                                      (get j "pressures" {})))
                :envelopes (flatten-envelopes (get j "envelopes" {}))})))
      (catch Exception e
        (die (str "invalid routing policy " ROUTING-POLICY ": " (.getMessage e)))))
    default-routing-policy))

(defn- routing-write! [policy]
  (io/make-parents ROUTING-POLICY)
  (let [dest (.toPath (io/file ROUTING-POLICY))
        dir  (.getParent dest)
        tmp  (java.nio.file.Files/createTempFile dir ".routing-policy." ".tmp"
                                                  (make-array java.nio.file.attribute.FileAttribute 0))]
    (try
      (let [document (cond-> {:version 1
                              :mode (:mode policy)
                              :targets (mapv (fn [[id target]] (assoc target :id id))
                                             (sort-by key (:targets policy)))
                              :targetOrder (:order policy)
                              :weights (:weights policy)
                              :pressures (into {} (map (fn [[id observation]]
                                                        [id (-> observation
                                                                (assoc :state (:level observation))
                                                                (dissoc :level))]))
                                               (:pressure policy))
                              :envelopes (nest-envelopes (:envelopes policy))}
                       (:reserve policy) (assoc :reservedFrontierTarget (:reserve policy)))]
        (spit (.toFile tmp) (str (json/generate-string document {:pretty true}) "\n")))
      (java.nio.file.Files/move tmp dest
        (into-array java.nio.file.CopyOption
                    [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                     java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
      (finally (java.nio.file.Files/deleteIfExists tmp)))))

(defn- target? [p id] (contains? (:targets p) id))
(defn- require-target! [p id]
  (when-not (target? p id) (die (str "unknown routing target: " id " (add it with `north config routing target add …`)"))))
(defn- positive-int [s label]
  (try
    (let [n (Long/parseLong (or s ""))]
      (if (pos? n) n (throw (Exception.))))
    (catch Exception _ (die (str label " must be a positive integer")))))
(defn- now-iso [] (.toString (java.time.Instant/now)))
(defn- require-iso! [s]
  (try (java.time.OffsetDateTime/parse s)
       (catch Exception _ (die "--until must be an ISO-8601 timestamp, for example 2026-08-01T00:00:00Z"))))

(defn- canonical-iso! [s]
  (-> (require-iso! s)
      .toInstant
      (.truncatedTo java.time.temporal.ChronoUnit/SECONDS)
      .toString))

(defn- default-hook-until []
  (-> (java.time.Instant/now)
      (.plusSeconds (* 24 60 60))
      (.truncatedTo java.time.temporal.ChronoUnit/SECONDS)
      .toString))

(defn- routing-summary [p]
  (let [reserve (or (:reserve p) "off")]
    (str "mode " (:mode p)
         " · reserve " reserve
         " · targets " (count (:targets p)))))

(defn- print-target-selection [p]
  (let [targets (:order p)]
    (case (:mode p)
      "balanced"
      (do
        (println (str "  configured candidate target set (unordered): " (str/join " · " targets)))
        (println "  eligibility: live authentication/headroom is evaluated by `north providers`")
        (println "  allocation: usage/headroom-weighted stable distribution; `north providers` shows current approximate shares"))

      "preferential"
      (do
        (println (str "  target priority: " (str/join " → " targets)))
        (println "  allocation: first eligible target, then retries in priority order"))

      "reserved"
      (do
        (println (str "  non-reserve target order: " (str/join " → " targets)))
        (println "  allocation: preserve the configured reserve outside eligible frontier work"))

      (println (str "  configured targets: " (str/join " · " targets))))))

(defn- pressure-label [observation]
  (if-not observation
    "automatic"
    (str "manual " (:level observation) " (observed " (:observedAt observation)
         (if-let [until (:until observation)] (str "; until " until) "; 24h TTL") ")")))

(defn- print-routing [p]
  (println (str "routing: " (routing-summary p)))
  (println (str "  policy: " ROUTING-POLICY))
  (print-target-selection p)
  (doseq [[id {:keys [provider authMode profile]}] (sort-by key (:targets p))]
    (println (str "  target " id " → " provider " · auth " (or authMode "ambient")
                  (when profile (str " (profile " profile ")"))
                  " · weight " (get (:weights p) id 1)
                  " · pressure " (pressure-label (get-in p [:pressure id])))))
  (when (seq (:envelopes p))
    (println "  envelopes:")
    (doseq [[scope limits] (sort-by key (:envelopes p))]
      (println (str "    " scope " " (str/join " · " (map (fn [[k v]] (str (name k) "=" v)) (sort-by key limits)))))))
  (println "  live pressure: `north providers` for categorized routing status · `north account usage` for per-account windows and resets.")
  (println "  policy pressure: automatic unless a temporary manual override is shown (24h unless --until is set).")
  (println "  adapter status: provider selection and exact named-account execution are live; an explicit target is pinned with no fallback."))

(def routing-usage
  "usage: north config routing [show|mode preferential|balanced|reserved|order <target...>|weight <target> <positive>|reserve <target|off>|pressure <target> <plenty|normal|low|exhausted|unknown> [--until ISO]|target add <id> <anthropic|openai> [profile] [--auth-mode ambient|isolated]|target remove <id>|envelope set <month|week|default|project:<id>|session:<id>> <runs|frontierRuns|retries|parallelism> <positive>|envelope clear <scope> [limit]]")

(defn cmd-routing [args]
  (let [p (routing-read)
        [verb & xs] args
        save! (fn [next]
                (let [validated (validate-routing next)]
                  (routing-write! validated)
                  (print-routing validated)))]
    (case (or verb "show")
      "show" (print-routing p)
      "mode" (let [[mode & extra] xs]
               (if (and (contains? #{"preferential" "balanced" "reserved"} mode) (empty? extra))
                 (save! (assoc p :mode mode))
                 (die routing-usage)))
      "order" (do (when (empty? xs) (die routing-usage))
                    (doseq [id xs] (require-target! p id))
                    (when-not (= (count xs) (count (distinct xs))) (die "routing order contains duplicate targets"))
                    (save! (assoc p :order (vec xs))))
      "weight" (let [[id n & extra] xs]
                 (when (or (nil? id) (nil? n) (seq extra)) (die routing-usage))
                 (require-target! p id)
                 (save! (assoc-in p [:weights id] (positive-int n "weight"))))
      "reserve" (let [[id & extra] xs]
                  (when (or (nil? id) (seq extra)) (die routing-usage))
                  (when-not (= id "off") (require-target! p id))
                  (save! (assoc p :reserve (when-not (= id "off") id))))
      "pressure" (let [[id level flag until & extra] xs]
                   (when (or (nil? id) (nil? level) (seq extra)
                             (and flag (not= flag "--until"))
                             (and (= flag "--until") (nil? until))) (die routing-usage))
                   (require-target! p id)
                   (when-not (contains? #{"plenty" "normal" "low" "exhausted" "unknown"} level)
                     (die routing-usage))
                   (when until (require-iso! until))
                   (save! (assoc-in p [:pressure id]
                                    (cond-> {:level level :observedAt (now-iso)}
                                      until (assoc :until until)))))
      "target" (let [[op id provider & target-args] xs]
                 (case op
                   "add" (let [[profile auth-mode]
                               (cond
                                 (empty? target-args) [nil nil]
                                 (= 1 (count target-args)) [(first target-args) nil]
                                 (and (= 2 (count target-args)) (= "--auth-mode" (first target-args)))
                                 [nil (second target-args)]
                                 (and (= 3 (count target-args)) (= "--auth-mode" (second target-args)))
                                 [(first target-args) (nth target-args 2)]
                                 :else (die routing-usage))]
                           (when (or (nil? id) (nil? provider)
                                     (not (contains? #{"anthropic" "openai"} provider))) (die routing-usage))
                           (when (and auth-mode (not (contains? #{"ambient" "isolated"} auth-mode))) (die routing-usage))
                           (when (and (= auth-mode "isolated") (not (portable-profile-slug? profile)))
                             (die "isolated routing targets require a portable profile slug (lowercase letters, digits, _ or -; max 64 characters)"))
                           (when (target? p id) (die (str "routing target already exists: " id)))
                           (save! (-> p
                                      (assoc-in [:targets id] (cond-> {:provider provider}
                                                               auth-mode (assoc :authMode auth-mode)
                                                               profile (assoc :profile profile)))
                                      (assoc-in [:weights id] 1)
                                      (update :order conj id))))
                   "remove" (do (when (or (nil? id) provider (seq target-args)) (die routing-usage))
                                 (require-target! p id)
                                 (when (= 1 (count (:targets p))) (die "cannot remove the final routing target"))
                                 (save! (-> p
                                            (update :targets dissoc id)
                                            (update :weights dissoc id)
                                            (update :pressure dissoc id)
                                            (update :order #(vec (remove #{id} %)))
                                            (update :reserve #(when-not (= % id) %)))))
                   (die routing-usage)))
      "envelope" (let [[op scope limit value & extra] xs
                       valid-scope? #(or (contains? #{"month" "week" "default"} %)
                                         (boolean (re-matches #"(project|session):.+" (or % ""))))
                       valid-limit? #(contains? #{"runs" "frontierRuns" "retries" "parallelism"} %)]
                   (case op
                     "set" (do (when (or (seq extra) (not (valid-scope? scope)) (not (valid-limit? limit)) (nil? value)) (die routing-usage))
                               (save! (assoc-in p [:envelopes scope (keyword limit)] (positive-int value "envelope limit"))))
                     "clear" (do (when (or value (seq extra) (not (valid-scope? scope)) (and limit (not (valid-limit? limit)))) (die routing-usage))
                                 (save! (if limit
                                          (let [next (update-in p [:envelopes scope] dissoc (keyword limit))]
                                            (if (empty? (get-in next [:envelopes scope])) (update next :envelopes dissoc scope) next))
                                          (update p :envelopes dissoc scope))))
                     (die routing-usage)))
      (die routing-usage))))

;; --- learning regime -----------------------------------------------------
;; Orthogonal to dispatch/account/posture: frozen reuses the admitted control
;; policy; learning admits bounded, deterministic one-axis exploration during
;; ordinary managed work. The SDK fingerprints this whole document.
(def learning-usage
  "usage: north config learning [show|mode frozen|learning|intensity <0..1>|axes all|none|<model-tier effort prompt authoring history...>|max-tier-delta <0..3>|risk-ceiling <p0|p1|p2|p3>|seed <id>|epoch <id>|evidence-mode discovery|evaluation]")

(defn- learning-id? [value]
  (boolean (re-matches #"[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}" (or value ""))))

(defn- validate-learning [policy]
  (let [expected #{:version :mode :intensity :axes :maxTierDelta
                   :riskCeiling :seed :epoch :evidenceMode}
        unknown (seq (remove expected (keys policy)))
        intensity (:intensity policy)
        axes (:axes policy)]
    (when unknown (throw (ex-info (str "unknown learning policy field(s): "
                                      (str/join ", " (map name unknown))) {})))
    (when-not (= 1 (:version policy))
      (throw (ex-info "learning policy version must be 1" {})))
    (when-not (#{"frozen" "learning"} (:mode policy))
      (throw (ex-info "learning mode must be frozen or learning" {})))
    (when-not (and (number? intensity) (Double/isFinite (double intensity))
                   (<= 0.0 (double intensity) 1.0))
      (throw (ex-info "learning intensity must be between 0 and 1" {})))
    (when-not (and (vector? axes) (= (count axes) (count (distinct axes)))
                   (every? learning-axis-set axes))
      (throw (ex-info (str "learning axes must contain only: "
                           (str/join ", " learning-axes)) {})))
    (when-not (and (integer? (:maxTierDelta policy))
                   (<= 0 (:maxTierDelta policy) 3))
      (throw (ex-info "learning maxTierDelta must be 0..3" {})))
    (when-not (#{"p0" "p1" "p2" "p3"} (:riskCeiling policy))
      (throw (ex-info "learning riskCeiling must be p0, p1, p2, or p3" {})))
    (when-not (learning-id? (:seed policy))
      (throw (ex-info "learning seed must be a portable identifier" {})))
    (when-not (learning-id? (:epoch policy))
      (throw (ex-info "learning epoch must be a portable identifier" {})))
    (when-not (#{"discovery" "evaluation"} (:evidenceMode policy))
      (throw (ex-info "learning evidenceMode must be discovery or evaluation" {})))
    policy))

(defn- learning-read []
  (if (.exists (io/file LEARNING-POLICY))
    (try
      (validate-learning (json/parse-string (slurp LEARNING-POLICY) true))
      (catch Exception error
        (die (str "invalid learning policy " LEARNING-POLICY ": "
                  (.getMessage error)))))
    default-learning-policy))

(defn- learning-write! [policy]
  (let [validated (validate-learning policy)]
    (io/make-parents LEARNING-POLICY)
    (let [dest (.toPath (io/file LEARNING-POLICY))
          dir (.getParent dest)
          tmp (java.nio.file.Files/createTempFile
               dir ".learning-policy." ".tmp"
               (make-array java.nio.file.attribute.FileAttribute 0))]
      (try
        (spit (.toFile tmp)
              (str (json/generate-string validated {:pretty true}) "\n"))
        (java.nio.file.Files/move
         tmp dest
         (into-array java.nio.file.CopyOption
                     [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                      java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
        (finally (java.nio.file.Files/deleteIfExists tmp))))
    validated))

(defn- print-learning [policy]
  (println (str "learning: " (:mode policy)
                " · " (:evidenceMode policy)
                " · intensity " (:intensity policy)))
  (println (str "  axes: " (if (seq (:axes policy))
                              (str/join " · " (:axes policy)) "none")))
  (println (str "  max tier delta: " (:maxTierDelta policy)
                " · risk ceiling: " (:riskCeiling policy)))
  (println (str "  seed: " (:seed policy) " · epoch: " (:epoch policy)))
  (println (str "  policy: " LEARNING-POLICY))
  (println "  frozen remains fully measured; learning changes at most one eligible axis per episode."))

(defn cmd-learning [args]
  (let [policy (learning-read)
        [verb & xs] args
        save! #(print-learning (learning-write! %))]
    (case (or verb "show")
      "show" (do (when (seq xs) (die learning-usage)) (print-learning policy))
      "mode" (let [[value & extra] xs]
               (if (and (#{"frozen" "learning"} value) (empty? extra))
                 (save! (assoc policy :mode value))
                 (die learning-usage)))
      "intensity" (let [[value & extra] xs
                        parsed (try (Double/parseDouble (or value ""))
                                    (catch Exception _ ##NaN))]
                    (if (and (empty? extra) (Double/isFinite parsed)
                             (<= 0.0 parsed 1.0))
                      (save! (assoc policy :intensity parsed))
                      (die learning-usage)))
      "axes" (let [values (cond
                            (= ["all"] xs) learning-axes
                            (= ["none"] xs) []
                            :else xs)]
               (if (and (= (count values) (count (distinct values)))
                        (every? learning-axis-set values))
                 (save! (assoc policy :axes (vec values)))
                 (die learning-usage)))
      "max-tier-delta" (let [[value & extra] xs
                             parsed (try (Long/parseLong (or value ""))
                                         (catch Exception _ -1))]
                         (if (and (empty? extra) (<= 0 parsed 3))
                           (save! (assoc policy :maxTierDelta parsed))
                           (die learning-usage)))
      "risk-ceiling" (let [[value & extra] xs]
                       (if (and (#{"p0" "p1" "p2" "p3"} value) (empty? extra))
                         (save! (assoc policy :riskCeiling value))
                         (die learning-usage)))
      "seed" (let [[value & extra] xs]
               (if (and (learning-id? value) (empty? extra))
                 (save! (assoc policy :seed value))
                 (die learning-usage)))
      "epoch" (let [[value & extra] xs]
                (if (and (learning-id? value) (empty? extra))
                  (save! (assoc policy :epoch value))
                  (die learning-usage)))
      "evidence-mode" (let [[value & extra] xs]
                        (if (and (#{"discovery" "evaluation"} value) (empty? extra))
                          (save! (assoc policy :evidenceMode value))
                          (die learning-usage)))
      (die learning-usage))))

;; --- native context assembly ---------------------------------------------
;; The tagged source remains the authority. Older or malformed sources retain
;; the SDK's historical heading-table behavior, with unknown headings kept in
;; core so a vocabulary mistake cannot silently drop policy.
(defn- context-fallback-metadata [heading]
  (let [h (str/lower-case heading)
        slug (-> h
                 (str/replace #"^##\s+" "")
                 (str/replace #"[^a-z0-9]+" "-")
                 (str/replace #"(^-+|-+$)" ""))]
    (cond
      (str/includes? h "done-claims") ["done-claims" "core"]
      (str/includes? h "standing guards") ["standing-guards" "core"]
      (str/includes? h "pre-edit gate") ["pre-edit-gate" "orch"]
      (str/includes? h "model +") ["model-routing" "orch"]
      (str/includes? h "push freely") ["push" "write"]
      (str/includes? h "external code") ["external-code" "write"]
      (str/includes? h "internal notes") ["internal-notes" "write"]
      (or (str/includes? h "nixos-config")
          (str/includes? h "global agent config")) ["global-agent-config" "nixos"]
      (or (str/includes? h "racket")
          (str/includes? h "beagle")) ["beagle" "beagle"]
      (str/includes? h "new code") ["new-code" "write"]
      (str/includes? h "blocked") ["blocked" "core"]
      (str/includes? h "paths") ["paths" "core"]
      (str/includes? h "north") ["north" "core"]
      :else [(if (str/blank? slug) "legacy-section" slug) "core"])))

(defn- context-section [text]
  (let [[_ heading second-line] (re-find #"(?s)\A(## [^\r\n]+)\r?\n([^\r\n]*)" text)
        heading (or heading (first (str/split-lines text)) "")
        tag (re-matches CONTEXT-TAG (or second-line ""))
        [fallback-id fallback-bucket] (context-fallback-metadata heading)]
    {:heading heading
     :text text
     :id (or (second tag) fallback-id)
     :bucket (or (nth tag 2 nil) fallback-bucket)
     :tagged? (boolean tag)}))

(defn- context-document []
  (let [raw (slurp' CONTEXT-SOURCE)]
    (when (nil? raw)
      (die (str "cannot read context source: " CONTEXT-SOURCE)))
    (let [matcher (re-matcher #"(?m)^## [^\r\n]*" raw)
          starts (loop [found []]
                   (if (.find matcher)
                     (recur (conj found (.start matcher)))
                     found))
          boundaries (map vector starts (concat (rest starts) [(count raw)]))]
      {:raw raw
       :preamble (if-let [start (first starts)] (subs raw 0 start) raw)
       :sections (mapv (fn [[start end]]
                         (context-section (subs raw start end)))
                       boundaries)})))

(defn- context-mode []
  (if (= "gated" (get' "context" "full")) "gated" "full"))

(defn- context-verdict [mode {:keys [id bucket]} now]
  (if (= mode "full")
    ["on" "full"]
    (let [[verdict decided-by]
          (north.harness-dial/resolve-dial
           nil
           (get' (str "context.bucket." bucket) nil)
           (get' (str "context.section." id) nil)
           nil
           now)]
      [verdict (if (= decided-by "category") "bucket" decided-by)])))

(defn- context-resolutions [mode sections]
  (let [now (north.harness-dial/now-iso)]
    (mapv (fn [section]
            [section (context-verdict mode section now)])
          sections)))

(defn- context-render [document mode resolutions]
  (if (= mode "full")
    (:raw document)
    (str (:preamble document)
         (apply str
                (keep (fn [[section [verdict _]]]
                        (when (= "on" verdict)
                          (:text section)))
                      resolutions)))))

(defn- context-write! [text]
  (io/make-parents CONTEXT-OUTPUT)
  (let [dest (.toAbsolutePath (.normalize (.toPath (io/file CONTEXT-OUTPUT))))
        dir (.getParent dest)
        tmp (java.nio.file.Files/createTempFile
             dir ".north-context." ".tmp"
             (make-array java.nio.file.attribute.FileAttribute 0))]
    (try
      (java.nio.file.Files/write
       tmp
       (.getBytes text java.nio.charset.StandardCharsets/UTF_8)
       (into-array java.nio.file.OpenOption
                   [java.nio.file.StandardOpenOption/WRITE
                    java.nio.file.StandardOpenOption/TRUNCATE_EXISTING]))
      (java.nio.file.Files/move
       tmp dest
       (into-array java.nio.file.CopyOption
                   [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                    java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
      (finally (java.nio.file.Files/deleteIfExists tmp)))))

(def context-usage
  "usage: north config context [show|on|off <section-id>|bucket on|off <bucket>|apply]")

(defn- require-context-section! [sections id]
  (when-not (some #(= id (:id %)) sections)
    (die (str "unknown context section: " id)))
  id)

(defn- print-context []
  (let [mode (context-mode)
        {:keys [sections]} (context-document)
        resolutions (context-resolutions mode sections)]
    (println (str "context = " mode))
    (println (str "  source " CONTEXT-SOURCE))
    (println (str "  output " CONTEXT-OUTPUT))
    (doseq [[{:keys [id bucket tagged?]} [verdict decided-by]] resolutions]
      (println
       (format "  %-24s %-7s %-3s %-8s %s"
               id bucket verdict decided-by
               (if tagged? "tagged" "fallback"))))))

(defn cmd-context [args]
  (let [[verb & xs] args]
    (case (or verb "show")
      "show"
      (do
        (when (seq xs) (die context-usage))
        (print-context))

      ("on" "off")
      (let [[id & extra] xs
            sections (:sections (context-document))]
        (when (or (nil? id) (seq extra)) (die context-usage))
        (require-context-section! sections id)
        ;; Keep the byte-identical full mode in force until the specific dial is
        ;; durable; the second atomic state write activates the gated view.
        (put' (str "context.section." id) verb)
        (put' "context" "gated")
        (println (str "context section " id " → " verb " (context → gated)")))

      "bucket"
      (let [[state bucket & extra] xs]
        (when (or (not (#{"on" "off"} state))
                  (not (CONTEXT-BUCKETS bucket))
                  (seq extra))
          (die context-usage))
        (put' (str "context.bucket." bucket) state)
        (put' "context" "gated")
        (println (str "context bucket " bucket " → " state " (context → gated)")))

      "apply"
      (do
        (when (seq xs) (die context-usage))
        (let [mode (context-mode)
              document (context-document)
              resolutions (context-resolutions mode (:sections document))
              text (context-render document mode resolutions)
              included (count (filter #(= "on" (first (second %)))
                                      resolutions))]
          (context-write! text)
          (println (str "context applied → " CONTEXT-OUTPUT
                        " (" mode ", " included "/" (count (:sections document))
                        " sections)"))))

      (die context-usage))))

;; --- shared skill projection ----------------------------------------------
;; The source inventory stays declarative and complete. Runtime dials select a
;; fresh immutable generation, then one atomic symlink replacement moves every
;; provider that follows ~/.agents/skills onto the same resolved set.
(def ^:private skill-slug #"[a-z0-9][a-z0-9-]*")
(defonce ^:private skills-in-process-lock (Object.))

(defn- absolute-path [path]
  (.toAbsolutePath (.normalize (.toPath (io/file path)))))

(defn- nofollow-exists? [path]
  (java.nio.file.Files/exists
   path
   (into-array java.nio.file.LinkOption
               [java.nio.file.LinkOption/NOFOLLOW_LINKS])))

(defn- nofollow-directory? [path]
  (java.nio.file.Files/isDirectory
   path
   (into-array java.nio.file.LinkOption
               [java.nio.file.LinkOption/NOFOLLOW_LINKS])))

(defn- skill-metadata [skill-file]
  (let [lines (vec (str/split-lines (slurp skill-file)))
        end (first
             (keep-indexed
              (fn [index line]
                (when (and (pos? index) (= "---" line)) index))
              lines))]
    (when-not (= "---" (first lines))
      (throw (ex-info (str "skill lacks YAML frontmatter: " skill-file)
                      {:path (str skill-file)})))
    (when-not end
      (throw (ex-info (str "skill has unterminated YAML frontmatter: " skill-file)
                      {:path (str skill-file)})))
    (let [frontmatter (subvec lines 1 end)
          ;; Only unindented scalar keys belong to the root contract. Folded
          ;; description text and document body prose cannot manufacture one.
          root (reduce
                (fn [metadata line]
                  (if-let [[_ key value]
                           (and (not (re-find #"^\s" line))
                                (re-matches
                                 #"([A-Za-z][A-Za-z0-9_-]*):\s*(.*)"
                                 line))]
                    (assoc metadata key (str/trim value))
                    metadata))
                {}
                frontmatter)
          metadata-index (first
                          (keep-indexed
                           (fn [index line]
                             (when (re-matches #"metadata:\s*" line) index))
                           frontmatter))
          nested-category
          (when metadata-index
            (some
             (fn [line]
               (some-> (re-matches #"\s+category:\s*(.*)" line) second str/trim))
             (take-while
              #(or (str/blank? %) (re-find #"^\s" %))
              (drop (inc metadata-index) frontmatter))))]
      (when (and nested-category (contains? root "category"))
        (throw (ex-info (str "skill declares category twice: " skill-file)
                        {:path (str skill-file)})))
      (cond-> root
        nested-category (assoc "category" nested-category)))))

(defn- skill-inventory []
  (let [root (io/file SKILLS-PROFILE)]
    (when-not (.isDirectory root)
      (throw (ex-info (str "skills source is not a directory: " SKILLS-PROFILE)
                      {:path SKILLS-PROFILE})))
    (let [entries (.listFiles root)]
      (when (nil? entries)
        (throw (ex-info (str "cannot read skills source: " SKILLS-PROFILE)
                        {:path SKILLS-PROFILE})))
      (when (empty? entries)
        (throw (ex-info (str "skills source is empty: " SKILLS-PROFILE)
                        {:path SKILLS-PROFILE})))
      (mapv
       (fn [entry]
         (let [id (.getName entry)
               skill-file (io/file entry "SKILL.md")]
           (when-not (re-matches skill-slug id)
             (throw (ex-info (str "invalid skill id: " id) {:id id})))
           (when-not (.isDirectory entry)
             (throw (ex-info (str "skill source entry is not a directory: " entry)
                             {:id id})))
           (when-not (.isFile skill-file)
             (throw (ex-info (str "skill is missing SKILL.md: " entry)
                             {:id id})))
           (let [metadata (skill-metadata skill-file)
                 declared-name (get metadata "name")
                 category (if (contains? metadata "category")
                            (get metadata "category")
                            "uncategorized")]
             (when-not (= id declared-name)
               (throw
                (ex-info
                 (str "skill frontmatter name " (pr-str declared-name)
                      " does not match directory " id)
                 {:id id :declared-name declared-name})))
             (when-not (re-matches skill-slug category)
               (throw (ex-info (str "invalid skill category for " id ": "
                                    (pr-str category))
                               {:id id :category category})))
             {:id id
              :category category
              ;; Keep the composed North profile as the visible authority.
              ;; Its owner link may move without rewriting a farm generation.
              :source (absolute-path entry)})))
       (sort-by #(.getName %) entries)))))

;; Readouts must remain useful while an individual profile item is malformed;
;; publication commands intentionally continue to use the strict inventory.
(defn- skill-readout-inventory []
  (let [root (io/file SKILLS-PROFILE)]
    (if-not (.isDirectory root)
      {:inventory [] :warnings [(str "skills source: not a directory: " SKILLS-PROFILE)]}
      (let [entries (.listFiles root)]
        (if (nil? entries)
          {:inventory [] :warnings [(str "skills source: cannot read: " SKILLS-PROFILE)]}
          (reduce
           (fn [{:keys [inventory warnings]} entry]
             (try
               (let [id (.getName entry)]
                 (if-not (.isDirectory entry)
                   {:inventory inventory
                    :warnings (conj warnings (str id ": not a skill directory"))}
                   (let [skill-file (io/file entry "SKILL.md")
                         metadata (do
                                    (when-not (.isFile skill-file)
                                      (throw (ex-info "missing SKILL.md" {})))
                                    (skill-metadata skill-file))
                         declared-name (get metadata "name")
                         category (get metadata "category" "uncategorized")]
                     (when-not (= id declared-name)
                       (throw (ex-info (str "frontmatter name " (pr-str declared-name)
                                            " does not match directory") {})))
                     (when-not (re-matches skill-slug category)
                       (throw (ex-info (str "invalid category " (pr-str category)) {})))
                     {:inventory (conj inventory {:id id :category category
                                                  :source (absolute-path entry)})
                      :warnings warnings})))
               (catch Exception error
                 {:inventory inventory
                  :warnings (conj warnings
                                  (str (.getName entry) ": "
                                       (or (.getMessage error) "invalid skill")))})))
           {:inventory [] :warnings []}
           (sort-by #(.getName %) entries)))))))

(defn- state-with-overlay [overlay key]
  (if (contains? overlay key)
    (get overlay key)
    (get' key nil)))

(defn- skill-resolutions
  ([inventory] (skill-resolutions inventory {}))
  ([inventory overlay]
   (let [now (north.harness-dial/now-iso)
         all (state-with-overlay overlay "skills")]
     (mapv
      (fn [{:keys [id category] :as skill}]
        (let [[verdict decided-by]
              (north.harness-dial/resolve-dial
               all
               (state-with-overlay overlay (str "skills.cat." category))
               (state-with-overlay overlay (str "skills.skill." id))
               nil
               now)]
          (assoc skill :verdict verdict :decided-by decided-by)))
      inventory))))

(defn- ensure-skills-lock-file! []
  (let [path (absolute-path SKILLS-LOCK)
        parent (.getParent path)]
    (java.nio.file.Files/createDirectories
     parent
     (make-array java.nio.file.attribute.FileAttribute 0))
    (try
      (java.nio.file.Files/createFile
       path
       (make-array java.nio.file.attribute.FileAttribute 0))
      (catch java.nio.file.FileAlreadyExistsException _))
    (when (or (java.nio.file.Files/isSymbolicLink path)
              (not (java.nio.file.Files/isRegularFile
                    path
                    (into-array java.nio.file.LinkOption
                                [java.nio.file.LinkOption/NOFOLLOW_LINKS]))))
      (throw (ex-info (str "skills lock must be a regular file: " path)
                      {:path (str path)})))
    path))

(defn- with-skills-lock [f]
  (locking skills-in-process-lock
    (let [path (ensure-skills-lock-file!)]
      (with-open
        [channel
         (java.nio.channels.FileChannel/open
          path
          (into-array
           java.nio.file.OpenOption
           [java.nio.file.StandardOpenOption/WRITE
            java.nio.file.LinkOption/NOFOLLOW_LINKS]))]
        (let [_held (.lock channel)]
          (f))))))

(defn- ensure-skills-topology! []
  (let [farm (absolute-path SKILLS-FARM)
        generations (absolute-path SKILLS-GENERATIONS)
        parent (.getParent farm)]
    (java.nio.file.Files/createDirectories
     parent
     (make-array java.nio.file.attribute.FileAttribute 0))
    (when (and (nofollow-exists? farm)
               (not (java.nio.file.Files/isSymbolicLink farm)))
      (throw
       (ex-info
        (str "refusing to replace unmanaged skills farm path: " farm)
        {:path (str farm)})))
    (if (nofollow-exists? generations)
      (when-not (nofollow-directory? generations)
        (throw
         (ex-info
          (str "skills generation root must be a real directory: " generations)
          {:path (str generations)})))
      (java.nio.file.Files/createDirectories
       generations
       (make-array java.nio.file.attribute.FileAttribute 0)))
    {:farm farm :generations generations}))

(defn- cleanup-prepared-skills! [{:keys [generation pointer]}]
  ;; A private generation contains only immediate symlinks created below.
  ;; Delete entries without walking them: following one would traverse back
  ;; into the authoritative profile.
  (when pointer
    (try (java.nio.file.Files/deleteIfExists pointer) (catch Throwable _)))
  (when generation
    (try
      (when (nofollow-directory? generation)
        (doseq [entry (or (.listFiles (.toFile generation))
                          (make-array java.io.File 0))]
          (java.nio.file.Files/deleteIfExists (.toPath entry))))
      (java.nio.file.Files/deleteIfExists generation)
      (catch Throwable _))))

(defn- prepare-skills-publication! [resolutions]
  (let [{:keys [farm generations]} (ensure-skills-topology!)
        generation (.resolve
                    generations
                    (str "gen-" (System/currentTimeMillis) "-"
                         (java.util.UUID/randomUUID)))
        pointer (.resolve
                 (.getParent farm)
                 (str ".skills-" (java.util.UUID/randomUUID) ".tmp"))
        prepared {:generation generation :pointer pointer}]
    (try
      (java.nio.file.Files/createDirectory
       generation
       (make-array java.nio.file.attribute.FileAttribute 0))
      (doseq [{:keys [id source verdict]} resolutions
              :when (= "on" verdict)]
        (java.nio.file.Files/createSymbolicLink
         (.resolve generation id)
         source
         (make-array java.nio.file.attribute.FileAttribute 0)))
      (java.nio.file.Files/createSymbolicLink
       pointer
       generation
       (make-array java.nio.file.attribute.FileAttribute 0))
      (assoc prepared :farm farm)
      (catch Throwable error
        (cleanup-prepared-skills! prepared)
        (throw
         (ex-info (str "cannot stage skills farm: " (.getMessage error))
                  {:farm SKILLS-FARM}
                  error))))))

(defn- publish-prepared-skills! [{:keys [farm pointer]}]
  ;; There is deliberately no non-atomic fallback. If the filesystem cannot
  ;; honor this replacement, the previous pointer remains authoritative.
  (when (and (nofollow-exists? farm)
             (not (java.nio.file.Files/isSymbolicLink farm)))
    (throw
     (ex-info
      (str "refusing to replace unmanaged skills farm path: " farm)
      {:path (str farm)})))
  (java.nio.file.Files/move
   pointer
   farm
   (into-array
    java.nio.file.CopyOption
    [java.nio.file.StandardCopyOption/ATOMIC_MOVE
     java.nio.file.StandardCopyOption/REPLACE_EXISTING])))

(defn- sync-skills! [inventory]
  (let [resolutions (skill-resolutions inventory)
        prepared (prepare-skills-publication! resolutions)]
    (try
      (publish-prepared-skills! prepared)
      (catch Throwable error
        (cleanup-prepared-skills! prepared)
        (throw
         (ex-info (str "cannot publish skills farm: " (.getMessage error))
                  {:farm SKILLS-FARM}
                  error))))
    (println
     (str "skills synchronized → " SKILLS-FARM " ("
          (count (filter #(= "on" (:verdict %)) resolutions))
          "/" (count resolutions) " enabled)"))))

(defn- change-skill-dial! [inventory key state label]
  (let [old-value (get' key nil)
        resolutions (skill-resolutions inventory {key state})
        prepared (prepare-skills-publication! resolutions)]
    ;; The expensive and fallible source/generation work is complete before
    ;; state changes. The only remaining farm operation is one atomic rename.
    (try
      (put' key state)
      (publish-prepared-skills! prepared)
      (catch Throwable error
        (let [rollback-error
              (try
                ;; Empty is resolver-equivalent to an absent prior key.
                (put' key (or old-value ""))
                nil
                (catch Throwable rollback rollback))]
          (cleanup-prepared-skills! prepared)
          (if rollback-error
            (throw
             (ex-info
              (str "skills update failed and state rollback also failed: "
                   (.getMessage error) "; rollback: "
                   (.getMessage rollback-error))
              {:key key :farm SKILLS-FARM}
              error))
            (throw
             (ex-info (str "skills update failed; prior state restored: "
                           (.getMessage error))
                      {:key key :farm SKILLS-FARM}
                      error))))))
    (println (str label " → " state " (skills synchronized)"))))

(defn- print-skills [inventory]
  (println (str "skills source: " SKILLS-PROFILE))
  (println (str "skills farm:   " SKILLS-FARM))
  (let [farm (absolute-path SKILLS-FARM)
        generations (absolute-path SKILLS-GENERATIONS)
        target (when (java.nio.file.Files/isSymbolicLink farm)
                 (.toAbsolutePath (.normalize (.resolve (.getParent farm)
                                                          (java.nio.file.Files/readSymbolicLink farm)))))
        published? (and target
                        (.startsWith target generations)
                        (nofollow-directory? target))]
    (println (str "published target: " (or (some-> target str) "MISSING")))
    (println (str "published generation: "
                  (if published? (.getFileName target) "MISSING")))
    (println (str "published farm: " (if published? "READY" "NOT PUBLISHED"))))
  (doseq [{:keys [id category verdict decided-by]}
          (skill-resolutions inventory)]
    (println
     (format "%-24s %-16s %-3s %s"
             id category verdict decided-by)))
  (println "provider/plugin-contributed skills live outside this farm and are not controlled here"))

(defn- skills-publication-summary []
  (let [farm (absolute-path SKILLS-FARM)
        generations (absolute-path SKILLS-GENERATIONS)
        target (when (java.nio.file.Files/isSymbolicLink farm)
                 (.toAbsolutePath (.normalize (.resolve (.getParent farm)
                                                          (java.nio.file.Files/readSymbolicLink farm)))))
        published? (and target (.startsWith target generations) (nofollow-directory? target))]
    (if published?
      (str "READY · target: " target " · generation: " (.getFileName target))
      "NOT PUBLISHED")))

(defn- skills-summary []
  (let [resolutions (skill-resolutions (skill-inventory))]
    (str (count (filter #(= "on" (:verdict %)) resolutions))
         "/" (count resolutions) " enabled")))

(defn- skills-readout-summary [{:keys [inventory warnings]}]
  {:summary (str (count (filter #(= "on" (:verdict %))
                                (skill-resolutions inventory)))
                 "/" (count inventory) " enabled")
   :warnings warnings})

(def skills-usage
  "usage: north config skills [list|on|off <skill-id>|category on|off <category>|all on|off|sync]")

(defn- require-skill! [inventory id]
  (when-not (some #(= id (:id %)) inventory)
    (die (str "unknown skill: " id)))
  id)

(defn- require-skill-category! [inventory category]
  (when-not (some #(= category (:category %)) inventory)
    (die (str "unknown skill category: " category)))
  category)

(defn cmd-skills [args]
  (let [[verb & xs] args]
    (case (or verb "list")
      "list"
      (do
        (when (seq xs) (die skills-usage))
        (with-skills-lock
          #(print-skills (skill-inventory)))
        (println)
        (run-config-drift-audit! "--section" "skills"))

      ("on" "off")
      (let [[id & extra] xs]
        (when (or (nil? id) (seq extra)) (die skills-usage))
        (with-skills-lock
          (fn []
            (let [inventory (skill-inventory)]
              (require-skill! inventory id)
              (change-skill-dial!
               inventory
               (str "skills.skill." id)
               verb
               (str "skill " id))))))

      "category"
      (let [[state category & extra] xs]
        (when (or (not (#{"on" "off"} state))
                  (nil? category)
                  (seq extra))
          (die skills-usage))
        (with-skills-lock
          (fn []
            (let [inventory (skill-inventory)]
              (require-skill-category! inventory category)
              (change-skill-dial!
               inventory
               (str "skills.cat." category)
               state
               (str "skill category " category))))))

      "all"
      (let [[state & extra] xs]
        (when (or (not (#{"on" "off"} state)) (seq extra))
          (die skills-usage))
        (with-skills-lock
          (fn []
            (let [inventory (skill-inventory)]
              (change-skill-dial! inventory "skills" state "skills all")))))

      "sync"
      (do
        (when (seq xs) (die skills-usage))
        (with-skills-lock
          (fn []
            (sync-skills! (skill-inventory)))))

      (die skills-usage))))

;; --- communications protocol ---------------------------------------------

(def comms-usage
  "usage: north config comms [show|off|db|file|both [--native|--managed] [--forced|--biased]|set <sub-key> <value>|doctor]")

(defn- comms-resolution [surface]
  (north.harness-dial/comms-selection #(get' % nil) surface))

(defn- comms-operational []
  {"db.poll" (get' "comms.db.poll" "hook")
   "db.budget-ms" (get' "comms.db.budget-ms" "1800")
   "file.root" (get' "comms.file.root"
                     (str home "/.local/state/north/comms"))
   "file.poll" (get' "comms.file.poll" "hook")
   "file.retain-hours" (get' "comms.file.retain-hours" "24")})

(defn- print-comms []
  (let [native (comms-resolution "native")
        managed (comms-resolution "managed")]
    (println "comms")
    (println (format "  %-12s %s" "base" (:base native)))
    (println
     (format "  %-12s %-5s (override %s)"
             "native" (:selected native) (:override native)))
    (println
     (format "  %-12s %-5s (override %s)"
             "managed" (:selected managed) (:override managed)))
    (println (format "  %-12s %s" "enforcement" (:enforcement native)))
    (doseq [[key value] (sort-by key (comms-operational))]
      (println (format "  %-17s %s" key value)))))

(defn- positive-int-string? [value]
  (boolean (re-matches #"[1-9][0-9]*" (or value ""))))

(defn- nonnegative-int-string? [value]
  (boolean (re-matches #"[0-9]+" (or value ""))))

(defn- validate-comms-sub-key! [sub-key value]
  (case sub-key
    "native"
    (when-not (#{"off" "db" "file" "both" "inherit"} value)
      (die "comms native must be off, db, file, both, or inherit"))

    "managed"
    (when-not (#{"off" "db" "file" "both" "inherit"} value)
      (die "comms managed must be off, db, file, both, or inherit"))

    "enforcement"
    (when-not (#{"forced" "biased"} value)
      (die "comms enforcement must be forced or biased"))

    "db.poll"
    (when-not (#{"hook" "listener" "off"} value)
      (die "comms db.poll must be hook, listener, or off"))

    "db.budget-ms"
    (when-not (positive-int-string? value)
      (die "comms db.budget-ms must be a positive integer"))

    "file.root"
    (when-not (and (not (str/blank? value))
                   (.isAbsolute (io/file value))
                   (not= "/" (.getCanonicalPath (io/file value))))
      (die "comms file.root must be an absolute non-root path"))

    "file.poll"
    (when-not (#{"hook" "inotify" "off"} value)
      (die "comms file.poll must be hook, inotify, or off"))

    "file.retain-hours"
    (when-not (nonnegative-int-string? value)
      (die "comms file.retain-hours must be a nonnegative integer"))

    (die (str "unknown comms sub-key: " sub-key)))
  value)

(defn- parse-comms-flags! [flags]
  (reduce
   (fn [{:keys [surface enforcement] :as parsed} flag]
     (cond
       (#{"--native" "--managed"} flag)
       (if surface
         (die comms-usage)
         (assoc parsed :surface (subs flag 2)))

       (#{"--forced" "--biased"} flag)
       (if enforcement
         (die comms-usage)
         (assoc parsed :enforcement (subs flag 2)))

       :else (die comms-usage)))
   {:surface nil :enforcement nil}
   flags))

(defn- run-comms-doctor! []
  (let [{:keys [exit out err]} (shell/sh COMMS-BIN "doctor")]
    (when-not (str/blank? out) (print out))
    (when-not (str/blank? err) (binding [*out* *err*] (print err)))
    (when-not (zero? exit) (System/exit exit))))

(defn cmd-comms [args]
  (let [[verb & xs] args]
    (cond
      (or (nil? verb) (= "show" verb))
      (do
        (when (seq xs) (die comms-usage))
        (print-comms))

      (#{"off" "db" "file" "both"} verb)
      (let [{:keys [surface enforcement]} (parse-comms-flags! xs)
            protocol-key (if surface (str "comms." surface) "comms")]
        ;; Base off must not leave a provider-specific override masking it.
        (when (and (= verb "off") (nil? surface))
          (put' "comms.native" "inherit")
          (put' "comms.managed" "inherit"))
        (put' protocol-key verb)
        (when enforcement
          (put' "comms.enforcement" enforcement))
        (println
         (str protocol-key " → " verb
              (when enforcement
                (str " · comms.enforcement → " enforcement))))
        (when (and (= verb "off") (nil? surface))
          (print-comms)))

      (= "set" verb)
      (let [[sub-key value & extra] xs]
        (when (or (nil? sub-key) (nil? value) (seq extra))
          (die comms-usage))
        (validate-comms-sub-key! sub-key value)
        (put' (str "comms." sub-key) value)
        (println (str "comms." sub-key " → " value)))

      (= "doctor" verb)
      (do
        (when (seq xs) (die comms-usage))
        (run-comms-doctor!))

      :else
      (die comms-usage))))

;; --- the report -----------------------------------------------------------
(defn banner []
  (let [rule  (apply str (repeat 66 "─"))
        label "  NORTH CONFIG — every setting, one report"
        d     (today)
        gap   (max 1 (- 66 (count label) (count d) 7))]
    (str "╭" rule "╮\n"
         "│" label (apply str (repeat gap " ")) d "       │\n"
         "╰" rule "╯")))

(defn- hook-verdict [id]
  (north.harness-dial/hook-verdict #(get' % nil) (hook-registry) id))

(defn- hooks-summary []
  (let [hooks (hook-registry)
        executable (count (filter #(= "EXEC" (hook-path-status %)) hooks))]
    (str executable "/" (count hooks) " executable")))

(defn status []
  (let [d  (dispatch-mode)
        c  (get' "coord" "north")
        comms-native (comms-resolution "native")
        comms-managed (comms-resolution "managed")
        learning (learning-read)
        skills-readout (skills-readout-summary (skill-readout-inventory))
        ]
    (println (banner))
    (println (str "
 1  DISPATCH   execution surface selection                 [guard: " (wired "agent-spawn-guard") "]
" (dispatch-status-lines d) "
    flip → north config dispatch " (north.dispatch-mode/usage) "

 2  COORD      coordination protocol           [north: " (north-daemon) " · linear MCP: " (linear-mcp) "]
    " (mark c "north") " north    facts on :7977 + concerns + msg-cli chat
    " (mark c "linear") " linear   Linear as the work queue (MCP)
    " (mark c "both") " both     Linear as consolidation layer over north
    note: declarative — agents read this posture; no hard enforcement yet
    flip → north config coord north|linear|both

 3  GUARDS     authoring-guard hooks           kill-switch: " (effective-ks) "
    " (wired "agent-spawn-guard") " agent-spawn-guard   " (wired "firn-guard") " firn
    " (wired "tripwire-guard") " tripwire            " (wired "beagle-session-start") " beagle-session
    [live]   flip authoring guards → north config guards on|off   (persists, all sessions; dispatch remains independent)
    [launch] one session → CLAUDE_NO_AUTHORING_HOOKS=1 claude   (launch ONLY — mid-session flip impossible; per-command prefix does nothing; 0/false forces guards live)

 4  ROUTING    provider targets + entitlement envelopes
    " (routing-summary (routing-read)) "
    pressure: automatic usage sensing; manual command is a temporary override/fallback
    configure → north config routing
    policy: " ROUTING-POLICY "

 5  HOOKS      per-hook and per-category runtime dials   [" (hooks-summary) "]
    precedence: item > category > all > default(on); coordination is excluded from all
    configure → north config hooks · north config hooks explain <hook-id>

 6  CONTEXT    native provider constitution assembly
    mode: " (context-mode) " · source: " CONTEXT-SOURCE "
    precedence in gated mode: section > bucket > default(on)
    configure → north config context · north config context apply

 7  SKILLS     shared provider-neutral discovery projection
    " (:summary skills-readout) " · source: " SKILLS-PROFILE "
    warnings: " (if (seq (:warnings skills-readout))
                    (str/join " · " (:warnings skills-readout)) "none") "
    farm: " SKILLS-FARM "
    published: " (skills-publication-summary) "
    precedence: item > category > all > default(on)
    configure → north config skills

 8  COMMS      peer mail protocol
    base: " (:base comms-native) " · native: " (:selected comms-native) " · managed: " (:selected comms-managed) " · enforcement: " (:enforcement comms-native) "
    default db preserves the fact-backed path; file is pure Bash/coreutils; both dedupes by @msg id
    configure → north config comms

 9  LEARNING   ordinary-operation exploration regime
    mode: " (:mode learning) " · evidence: " (:evidenceMode learning) " · intensity: " (:intensity learning) "
    axes: " (if (seq (:axes learning)) (str/join " · " (:axes learning)) "none") "
    frozen uses the current best-known route/prompt/interface and still records receipts
    configure → north config learning

 elsewhere: system/nix settings → firn tag status · session effort → /effort
 dials: [live] north config flip, effective now · [launch] env at claude launch, frozen for session · [spawn] request-owned routing; managed compression defaults off when no request/env exists
 state: ~/.local/state/north/harness.conf · legacy read fallback: ~/.claude/my-config.state · descriptions + advice: north config help"))
    (print-provider-readouts)))

(defn help []
  (println (str "north config — every personal-stack posture setting, one entry point.

 1 DISPATCH — execution surface selection.
" (dispatch-help-lines) "
   Auto is governed by the orthogonal `north config learning` axis: frozen
   uses deterministic known-best assignment; learning permits bounded
   experimental assignment. Account allocation is a routing detail, not a
   dispatch mode.
   Advice: managed pins North; native pins the provider surface; auto delegates
   the choice to the system.

 2 COORD — source of truth for work coordination.
   north / linear / both (Linear as consolidation layer over north).
   Declarative for now: agents read this posture; nothing mechanically
   blocks the other system yet. Flipping the option does not build the sync.
   Advice: north.

 3 GUARDS — the PreToolUse/SessionStart authoring guards.
   Individually wired in ~/code/nixos-config/main/dotfiles/claude/settings.json.
   Kill-switch is VALUE-AWARE and has two surfaces:

   [live] state flip (primary — effective immediately across ALL sessions,
   no relaunch; hooks re-read state on every call):
     north config guards off   → writes guards=off to ~/.local/state/north/harness.conf
     north config guards on    → removes that line (or writes guards=on)

   [launch] env override — single session, launch ONLY; mid-session flip
   impossible; per-command env prefix does nothing (claude reads it at
   start, then frozen for the session):
     CLAUDE_NO_AUTHORING_HOOKS=1 claude     authoring guards OFF this session; dispatch unchanged
     CLAUDE_NO_AUTHORING_HOOKS=0 claude     force-live (state ignored)
   Any non-empty value other than 0/false kills guards; 0 or false forces
   them live. This never changes native-vs-North agent topology; `north config
   dispatch` owns that independent axis. Env beats state. Semantics live in the shared lib sourced by
   every guard hook AND by this verb:
     ~/.claude/hooks/lib/authoring-killswitch.sh

 4 ROUTING — durable provider selection and subscription-entitlement policy.
   Show everything with `north config routing`. Balanced allocation is the
   default; preferential and reserved remain explicit choices. Configure
   provider/profile targets and
   month/week/project/session run envelopes. Provider adapters automatically
   sense available subscription usage during normal operation. `routing
   pressure` records a temporary manual override/fallback when sensing cannot
   represent what you know; it expires after 24 hours unless --until is given.
   The canonical atomic JSON file is ~/.config/north/routing-policy.json
   (NORTH_ROUTING_POLICY overrides it for isolated tests/tools).
   Named profiles are executable account targets with isolated subscription
   sessions. No API keys, credit balances, prices, or dollars live
   in this policy.

 5 HOOKS — runtime control for every registered hook.
   List resolved state, provenance, and executable path status:
     north config hooks
     north config hooks explain <hook-id>
   Set the most specific level needed:
     north config hooks on|off <hook-id> [--until ISO]
     north config hooks category on|off <category> [--until ISO]
     north config hooks all on|off [--until ISO]
   Resolution is item > category > all > default(on). Coordination/identity
   hooks are excluded from the global sweep and must be named. Disabling any
   deny-capable scope expires after 24 hours by default; --until sets an
   explicit deadline. `north config guards` remains the compatibility surface
   for the authoring category.

 6 CONTEXT — assemble the native provider constitution from North's source.
   The default `full` mode copies the source byte-for-byte. A section or bucket
   change activates `gated` mode; resolution is section > bucket > default(on):
     north config context
     north config context on|off <section-id>
     north config context bucket on|off <core|write|shell|orch|client|nixos|beagle>
     north config context apply
   `show` reports each section's effective value, provenance, and whether its
   metadata came from a tag or the compatibility heading table. `apply`
   atomically replaces ~/.claude/CLAUDE.md, including when it is currently a
   symlink; the provider-neutral ~/.agents/AGENTS.md source is never mutated.

 7 SKILLS — resolved shared skill discovery.
   North inventories the complete source at the stable North profile:
   $WORLD_REPO_NORTH/agent-profile/skills (the current checkout is the
   direct-invocation fallback). Optional `category:` frontmatter groups skills;
   a missing category resolves as `uncategorized`.
     north config skills
     north config skills on|off <skill-id>
     north config skills category on|off <category>
     north config skills all on|off
     north config skills sync
   Resolution is item > category > all > default(on). Every mutation stages a
   complete immutable generation and atomically replaces
   ~/.local/state/north/skills; ~/.agents/skills and provider adapters follow
   that stable farm. Provider/plugin-contributed skills remain outside it.

   The readout also proves the published farm symlink, its resolved immutable
   generation, and whether that generation is ready for provider discovery.

 8 COMMS — select the peer-mail transport independently for native and managed
   execution. The default is db + forced, exactly the pre-dial behavior:
     north config comms
     north config comms off|db|file|both [--native|--managed] [--forced|--biased]
     north config comms set <sub-key> <value>
     north config comms doctor
   off disables peer delivery. forced uses only the selected protocol and
   rejects an explicit conflicting request with the compliant move.
   biased writes the selected protocol and
   reads both. both dual-writes one globally unique @msg id and dedupes reads.
   file uses an atomic scratch-to-new publication, new-to-cur acknowledgement,
   finite broadcast snapshots, and renewable .live presence. It deliberately
   has no durable audit trail; thread facts are unchanged.

 10 PROVIDER MCP / 11 PROVIDER PLUGINS — status prints each declaration it
   can read and its exact provider inverse command. These are provider-owned;
   run `north config` again after changing them.

 12 CONFIG DRIFT — read-only effective capability audit.
   `north config audit [--json]` inventories enabled shared/provider/plugin
   skill roots with canonical provenance and collision uncertainty, then
   compares parsed Claude/Codex MCP declarations for alignment, same-name
   drift, and equivalent aliases. Environment and header values are never
   printed; only their key sets and deterministic digests are exposed.

 9 LEARNING — bounded experimentation during ordinary managed work.
   frozen    use the current best-known control policy consistently; continue
             telemetry and content-addressed prompt/environment receipts.
   learning  deterministically explore at most one eligible axis per episode,
             within the configured risk ceiling, hard quality floor, and tier
             delta. Discovery observations are never evaluation comparisons.
   Advice: frozen for critical/high-risk periods; learning for routine work.
   Configure: north config learning

 Elsewhere (owned by other CLIs, not duplicated here):
   system/nix composition → firn tag status · firn enable <tag>
   session effort/ultracode → /effort (harness-level, not script-readable)")))

;; --- verb dispatch --------------------------------------------------------

(defn cmd-dispatch [[sub & extra]]
  (cond
    (= sub "--canonical")
    (if (seq extra)
      (die "usage: north config dispatch --canonical")
      (println (dispatch-mode)))

    (= sub "--guard-action")
    (if (seq extra)
      (die "usage: north config dispatch --guard-action")
      (println
       (north.dispatch-mode/guard-action
        (dispatch-mode))))

    (= sub "--managed-admission")
    (if (seq extra)
      (die "usage: north config dispatch --managed-admission")
      (println
       (north.dispatch-mode/managed-admission
        (dispatch-mode))))

    (north.dispatch-mode/recognized? sub)
    (let [summary (:summary (north.dispatch-mode/spec sub))]
      (put' "dispatch" sub)
      (println (str "dispatch → " sub " (" summary ")")))

    (nil? sub)
    (let [d (dispatch-mode)]
      (println (str "dispatch = " d "\n" (north.dispatch-mode/grid)
                    "\n   (north config dispatch " (north.dispatch-mode/usage) ")")))

    :else
    (die (str "usage: north config dispatch [" (north.dispatch-mode/usage) "]"))))

(defn cmd-coord [[sub]]
  (cond
    (#{"north" "linear" "both"} sub)
    (do
      (put' "coord" sub)
      (println (str "coord → " sub " (declarative; agents read it from the north config report)")))
    (nil? sub)
    (let [c (get' "coord" "north")]
      (println (str "coord = " c "   (north config coord north|linear|both)")))
    :else
    (die "usage: north config coord [north|linear|both]")))

(def hooks-usage
  "usage: north config hooks [list|explain <hook-id>|on|off <hook-id> [--until ISO]|category on|off <category> [--until ISO]|all on|off [--until ISO]]")

(defn- require-hook! [id]
  (or (hook-entry id)
      (die (str "unknown hook: " id))))

(defn- require-hook-category! [category]
  (when-not (some #(= category (:category %)) (hook-registry))
    (die (str "unknown hook category: " category)))
  category)

(defn- hook-category-key [category]
  (if (= category "authoring")
    "guards"
    (str "hooks.cat." category)))

(defn- parse-hook-until! [args]
  (cond
    (empty? args) nil
    (and (= 2 (count args)) (= "--until" (first args)))
    (canonical-iso! (second args))
    :else (die hooks-usage)))

(defn- put-hook-dial!
  [key state until ttl-required? label]
  (when-not (#{"on" "off"} state)
    (die hooks-usage))
  (when (and (= state "on") until)
    (die "--until is valid only when disabling a hook scope"))
  (let [deadline (when (= state "off")
                   (or until (when ttl-required? (default-hook-until))))
        value (if deadline (str "off:until=" deadline) state)]
    (put' key value)
    (println (str label " → " value
                  (when (and deadline (nil? until))
                    " (default 24h TTL)")))))

(defn- print-hooks []
  (let [hooks (hook-registry)]
    (if (empty? hooks)
      (println "(no hooks registered)")
      (doseq [{:keys [id category kind events] :as hook} hooks
              :let [[verdict decided-by] (hook-verdict id)]]
        (println
         (format "%-34s %-13s %-9s %-3s %-9s %-7s %s · %s"
                 id category kind verdict decided-by
                 (hook-path-status hook) events (str (hook-file hook))))))))

(defn- explain-hook [id]
  (let [{:keys [category in-all?] :as hook} (require-hook! id)
        item-key (str "hooks.hook." id)
        category-key (hook-category-key category)
        item (get' item-key nil)
        category-value (get' category-key nil)
        all (when in-all? (get' "hooks" nil))
        env (when (= category "authoring")
              (north.harness-dial/authoring-env))
        [verdict decided-by] (hook-verdict id)
        shown #(or % "(unset)")]
    (println (str id " · " category " · " (:kind hook)))
    (println (str "  item      " item-key "=" (shown item)))
    (println (str "  category  " category-key "=" (shown category-value)))
    (println (str "  all       "
                  (if in-all? (str "hooks=" (shown all)) "(excluded)")))
    (println (str "  env       " (shown env)))
    (println "  default   on")
    (println (str "  effective " verdict " (decided by " decided-by ")"))
    (println (str "  path      " (hook-path-status hook) " " (hook-file hook)))))

(defn cmd-hooks [args]
  (let [[verb & xs] args]
    (case (or verb "list")
      "list"
      (do
        (when (seq xs) (die hooks-usage))
        (print-hooks))

      "explain"
      (let [[id & extra] xs]
        (when (or (nil? id) (seq extra)) (die hooks-usage))
        (explain-hook id))

      ("on" "off")
      (let [[id & extra] xs
            hook (require-hook! id)
            until (parse-hook-until! extra)]
        (put-hook-dial! (str "hooks.hook." id) verb until
                        (:ttl-required? hook) (str "hook " id)))

      "category"
      (let [[state category & extra] xs
            _ (when (or (nil? state) (nil? category)) (die hooks-usage))
            _ (require-hook-category! category)
            until (parse-hook-until! extra)
            ttl-required? (boolean
                           (some #(and (= category (:category %))
                                       (:ttl-required? %))
                                 (hook-registry)))]
        (put-hook-dial! (hook-category-key category) state until
                        ttl-required? (str "hook category " category)))

      "all"
      (let [[state & extra] xs
            _ (when (nil? state) (die hooks-usage))
            until (parse-hook-until! extra)
            ttl-required? (boolean
                           (some #(and (:in-all? %) (:ttl-required? %))
                                 (hook-registry)))]
        (put-hook-dial! "hooks" state until ttl-required? "hooks all"))

      (die hooks-usage))))

(defn cmd-guards [[sub]]
  (cond
    (= sub "off") (do (put' "guards" "off")
                      (println "guards → OFF in all sessions (hooks re-read state per call, no relaunch needed); north config guards on restores"))
    (= sub "on")  (do (put' "guards" "on")
                      (println "guards → LIVE in all sessions (takes effect immediately)"))
    (nil? sub)
    (do (println (str "kill-switch: " (effective-ks)))
        (doseq [g ["agent-spawn-guard" "firn-guard"
                   "tripwire-guard" "beagle-session-start"]]
          (println (str "  " (wired g) " " g))))
    :else (die "usage: north config guards [on|off]")))

(defn cmd-audit [args]
  (when-not (or (empty? args) (= ["--json"] (vec args)))
    (die "usage: north config audit [--json]"))
  (apply run-config-drift-audit! args))

(defn -main [& args]
  (try
    (let [[verb & rest] args]
      (case (or verb "status")
        ("status") (status)
        "dispatch" (cmd-dispatch rest)
        "coord"    (cmd-coord rest)
        "guards"   (cmd-guards rest)
        "hooks"    (cmd-hooks rest)
        "context"  (cmd-context rest)
        "skills"   (cmd-skills rest)
        "mcp"      (cmd-mcp rest)
        "audit"    (cmd-audit rest)
        "comms"    (cmd-comms rest)
        "routing"  (cmd-routing rest)
        "learning" (cmd-learning rest)
        ("help" "-h" "--help") (help)
        (die "usage: north config [status|dispatch|coord|guards|hooks|context|skills|mcp|audit|comms|routing|learning|help]")))
    (catch clojure.lang.ExceptionInfo error
      (die (.getMessage error)))))

(apply -main *command-line-args*)
