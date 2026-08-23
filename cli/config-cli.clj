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
;; Agent unit permissions live in the immutable activation generation. Other
;; provider-neutral posture remains in ~/.local/state/north/harness.conf.

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
(load-file (str (or (System/getenv "NORTH_HOME")
                    (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
                "/cli/agent-catalog.clj"))
(load-file (str (or (System/getenv "NORTH_HOME")
                    (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
                "/cli/agent-catalog-cli.clj"))
(def STATE           (north.harness-state/canonical-path home))
(def ROUTING-POLICY  (or (System/getenv "NORTH_ROUTING_POLICY")
                         (str home "/.config/north/routing-policy.json")))
(def LEARNING-POLICY (or (System/getenv "NORTH_LEARNING_POLICY")
                         (str home "/.config/north/learning-policy.json")))
(def COMMS-BIN       (or (System/getenv "NORTH_COMMS_BIN")
                         (str (or (System/getenv "NORTH_HOME")
                                  (some-> *file* io/file .getCanonicalFile
                                          .getParentFile .getParentFile str))
                              "/bin/north-comms")))

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

(def CODEX-CONFIG (or (System/getenv "NORTH_CODEX_CONFIG")
                      (str (or (System/getenv "CODEX_HOME") (str home "/.codex"))
                           "/config.toml")))
(def CONFIG-DRIFT-AUDIT
  (str (or (System/getenv "NORTH_HOME")
           (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
       "/cli/config-drift-audit.py"))

(def mcp-usage
  "usage: north config mcp [list [--json]|add <name> <url>|add <name> -- <command> [args...]|remove <name>]\nMCP declarations are applied to Codex.")

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
                  (apply run-provider! "codex" "mcp" "add" name "--" extra)
                  (println (str name " → Codex stdio MCP")))
                (do
                  (when (or (seq extra) (not (re-matches #"https?://.+" target)))
                    (die mcp-usage))
                  (run-provider! "codex" "mcp" "add" name "--url" target)
                  (println (str name " → Codex MCP (" target ")")))))
      "remove" (do
                 (when (or (str/blank? name) target (seq extra)) (die mcp-usage))
                 (run-provider! "codex" "mcp" "remove" name)
                 (println (str name " removed from Codex MCP")))
      (die mcp-usage))))

(defn- toml-section-names [path section]
  (if-let [text (slurp' path)]
    (->> (str/split-lines text)
         (keep #(second (re-matches (re-pattern (str "^\\[" section "\\.([^]]+)\\]$")) %)))
         sort)
    []))

(defn linear-mcp []
  (if (some #(str/includes? % "linear")
            (toml-section-names CODEX-CONFIG "mcp_servers"))
    "configured"
    "absent"))

(defn- print-provider-readouts []
  (let [codex-mcp (toml-section-names CODEX-CONFIG "mcp_servers")
        codex-plugins (toml-section-names CODEX-CONFIG "plugins")
        render (fn [provider kind names command path]
                 (println (str "    " provider " " kind ": " path))
                 (if (seq names)
                   (doseq [name names]
                     (println (str "      " name " → " (format command name))))
                   (println "      (none declared)")))]
    (println "\n9 CODEX MCP  provider-owned declarations")
    (render "Codex" "MCP" codex-mcp "codex mcp remove %s" CODEX-CONFIG)
    (println "\n10 CODEX PLUGINS  provider-owned installations")
    (render "Codex" "plugin" codex-plugins "codex plugin uninstall %s" CODEX-CONFIG)))

(declare agent-activation)

(defn wired [id]
  (let [activation (agent-activation)
        entries (for [type ["hook" "providerAdapter"]
                      [_target plans] (get-in activation ["projectionPlan" type])
                      plan plans
                      :when (= id (get plan "unitId"))]
                  plan)
        directory (io/file (north.agent-catalog/agents-root) "current/provider-hooks")]
    (if (and (seq entries)
             (every? #(.canExecute (io/file directory (get % "adapterId"))) entries))
      "✓"
      "✗"))) ; ✓ / ✗

(defn effective-ks []
  (case (north.harness-dial/authoring-env)
    "on"  "env force-live — guards LIVE (state ignored this session)"
    "off" "ENGAGED via env (this session) — authoring guards OFF; dispatch topology unchanged"
    (let [hooks (filter #(and (= "hook" (get % "kind"))
                              (= "authoring" (get % "category")))
                        (get (agent-activation) "units"))]
      (str (count (filter #(get % "active") hooks)) "/" (count hooks) " active"))))

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

;; Agent skill projection and permissions are owned by north.agent-catalog.

(defn- skills-publication-summary []
  (if-let [activation (north.agent-catalog/current-activation)]
    (str "READY · target: " (north.agent-catalog/agents-root)
         "/current/skills/shared · generation: " (get activation "generationId"))
    "NOT PUBLISHED"))

(defn- skills-summary []
  (let [activation (or (north.agent-catalog/current-activation)
                       (north.agent-catalog/compile-activation
                        (north.agent-catalog/load-catalog)))
        skills (filter #(= "skill" (get % "kind")) (get activation "units"))]
    (str (count (filter #(get % "active") skills))
         "/" (count skills) " active")))

(defn- skills-readout-summary [_]
  {:summary (skills-summary) :warnings []})

(def skills-usage
  "usage: north config skills [list|on|off <skill-id>|category on|off <category>|all on|off|sync]")

(defn- agent-activation []
  (or (north.agent-catalog/current-activation)
      (north.agent-catalog/compile-activation (north.agent-catalog/load-catalog))))

(defn- agent-skills []
  (filterv #(= "skill" (get % "kind")) (get (agent-activation) "units")))

(defn- require-skill! [inventory id]
  (when-not (some #(= id (get % "id")) inventory)
    (die (str "unknown skill: " id)))
  id)

(defn- require-skill-category! [inventory category]
  (when-not (some #(= category (get % "category")) inventory)
    (die (str "unknown skill category: " category)))
  category)

(defn- print-agent-skills []
  (let [activation (agent-activation)
        current? (some? (north.agent-catalog/current-activation))]
    (println (str "skills source: " (north.agent-catalog/catalog-path)))
    (println (str "skills farm:   " (north.agent-catalog/agents-root)
                  "/current/skills/shared"))
    (println (str "published generation: "
                  (if current? (get activation "generationId") "MISSING")))
    (println (str "published farm: " (if current? "READY" "NOT PUBLISHED")))
    (doseq [skill (filter #(= "skill" (get % "kind")) (get activation "units"))]
      (println
       (format "%-24s %-16s %-13s %s"
               (get skill "id") (get skill "category" "uncategorized")
               (get skill "permission")
               (if (get skill "active") "active" "inactive"))))
    (println "provider-owned system skills live outside this catalog and are not controlled here")))

(defn cmd-skills [args]
  (let [[verb & xs] args]
    (case (or verb "list")
      "list"
      (do
        (when (seq xs) (die skills-usage))
        (print-agent-skills)
        (println)
        (run-config-drift-audit! "--section" "skills"))

      ("on" "off")
      (let [[id & extra] xs]
        (when (or (nil? id) (seq extra)) (die skills-usage))
        (let [inventory (agent-skills)]
          (require-skill! inventory id)
          (north.agent-catalog/change-permissions! {id verb})
          (println (str "skill " id " → " verb " (skills synchronized)"))))

      "category"
      (let [[state category & extra] xs]
        (when (or (not (#{"on" "off"} state))
                  (nil? category)
                  (seq extra))
          (die skills-usage))
        (let [inventory (agent-skills)]
          (require-skill-category! inventory category)
          (north.agent-catalog/change-permissions!
           (into {} (map (fn [unit] [(get unit "id") state]))
                 (filter #(= category (get % "category")) inventory)))
          (println (str "skill category " category " → " state
                        " (skills synchronized)"))))

      "all"
      (let [[state & extra] xs]
        (when (or (not (#{"on" "off"} state)) (seq extra))
          (die skills-usage))
        (north.agent-catalog/change-permissions!
         (into {} (map (fn [unit] [(get unit "id") state])) (agent-skills)))
        (println (str "skills all → " state " (skills synchronized)")))

      "sync"
      (do
        (when (seq xs) (die skills-usage))
        (let [activation (north.agent-catalog/sync!)]
          (println (str "skills synchronized → " (north.agent-catalog/agents-root)
                        "/current/skills/shared ("
                        (count (filter #(and (= "skill" (get % "kind"))
                                             (get % "active"))
                                       (get activation "units")))
                        " active)"))))

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

(defn- hooks-summary []
  (let [hooks (filter #(= "hook" (get % "kind")) (get (agent-activation) "units"))]
    (str (count (filter #(get % "active") hooks)) "/" (count hooks) " active")))

(defn status []
  (let [d  (dispatch-mode)
        c  (get' "coord" "north")
        comms-native (comms-resolution "native")
        comms-managed (comms-resolution "managed")
        learning (learning-read)
        skills-readout (skills-readout-summary nil)
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
    " (wired "agent-spawn-guard") " agent-spawn-guard   " (wired "firn-system-policy") " firn
    " (wired "tripwire-guard") " tripwire            " (wired "beagle-session-start") " beagle-session
    [live]   flip authoring guards → north config guards on|off   (persists, all sessions; dispatch remains independent)
    [launch] one session → AGENT_NO_AUTHORING_HOOKS=1 provider   (launch ONLY — mid-session flip impossible; per-command prefix does nothing; 0/false forces guards live)

 4  ROUTING    provider targets + entitlement envelopes
    " (routing-summary (routing-read)) "
    pressure: automatic usage sensing; manual command is a temporary override/fallback
    configure → north config routing
    policy: " ROUTING-POLICY "

 5  HOOKS      catalog activation   [" (hooks-summary) "]
    configure → north config hooks · north config agents inspect <hook-id>

 6  SKILLS     shared provider-neutral discovery projection
    " (:summary skills-readout) " · source: " (north.agent-catalog/catalog-path) "
    warnings: " (if (seq (:warnings skills-readout))
                    (str/join " · " (:warnings skills-readout)) "none") "
    farm: " (north.agent-catalog/agents-root) "/current/skills/shared
    published: " (skills-publication-summary) "
    one UnitId permission authority
    configure → north config skills

 7  COMMS      peer mail protocol
    base: " (:base comms-native) " · native: " (:selected comms-native) " · managed: " (:selected comms-managed) " · enforcement: " (:enforcement comms-native) "
    default db preserves the fact-backed path; file is pure Bash/coreutils; both dedupes by @msg id
    configure → north config comms

 8  LEARNING   ordinary-operation exploration regime
    mode: " (:mode learning) " · evidence: " (:evidenceMode learning) " · intensity: " (:intensity learning) "
    axes: " (if (seq (:axes learning)) (str/join " · " (:axes learning)) "none") "
    frozen uses the current best-known route/prompt/interface and still records receipts
    configure → north config learning

 elsewhere: system/nix settings → firn tag status · session effort → /effort
 agents: immutable generation at ~/.local/state/north/agents/current · session-only authoring override at provider launch
 other state: ~/.local/state/north/harness.conf · descriptions + advice: north config help"))
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
   Registered in the global catalog and materialized as generated provider adapters.
   Kill-switch is VALUE-AWARE and has two surfaces:

   [live] catalog permission flip (effective across ALL sessions; hooks read
   the current activation generation on every call):
     north config guards off
     north config guards on

   [launch] env override — single session, launch ONLY; mid-session flip
   impossible; per-command env prefix does nothing after the provider harness
   captures it at session start:
     AGENT_NO_AUTHORING_HOOKS=1 provider    authoring guards OFF this session; dispatch unchanged
     AGENT_NO_AUTHORING_HOOKS=0 provider    force-live (state ignored)
   Any non-empty value other than 0/false kills guards; 0 or false forces
   them live. This never changes native-vs-North agent topology; `north config
   dispatch` owns that independent axis. Env beats activation for authoring
   hooks only. Semantics live in the generated support library:
     ~/.local/state/north/agents/current/provider-hooks/lib/authoring-killswitch.sh

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

 5 HOOKS — thin batch and status views over agent activation.
   List resolved state and provenance:
     north config hooks
     north config hooks explain <hook-id>
   Mutate one hook or a batch through the same UnitId authority:
     north config hooks on|off <hook-id> [--until ISO]
     north config hooks category on|off <category> [--until ISO]
     north config hooks all on|off [--until ISO]
   `north config guards` is the authoring-hook batch client. The optional
   --until value stores a timed-off permission in the same generation.

 6 SKILLS — resolved shared skill discovery.
   North reads `north:agent-catalog/catalog.json`; it never scans projects.
     north config skills
     north config skills on|off <skill-id>
     north config skills category on|off <category>
     north config skills all on|off
     north config skills sync
   Every mutation materializes a complete immutable generation and atomically
   replaces ~/.local/state/north/agents/current. Provider-owned system and
   plugin skills remain outside the shared user farm.

   The readout also proves the published farm symlink, its resolved immutable
   generation, and whether that generation is ready for provider discovery.

 7 COMMS — select the peer-mail transport independently for native and managed
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

 9 CODEX MCP / 10 CODEX PLUGINS — status prints each declaration it can read
   and its exact inverse command. These are provider-owned;
   run `north config` again after changing them.

 11 CONFIG AUDIT — read-only effective capability audit.
   `north config audit [--json]` inventories enabled shared/Codex plugin skill
   roots with canonical provenance and collision uncertainty, plus parsed
   Codex MCP declarations. Environment and header values are never printed;
   only their key sets and deterministic digests are exposed.

 8 LEARNING — bounded experimentation during ordinary managed work.
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

(defn- agent-hooks []
  (filterv #(= "hook" (get % "kind")) (get (agent-activation) "units")))

(defn- require-hook! [id]
  (or (some #(when (= id (get % "id")) %) (agent-hooks))
      (die (str "unknown hook: " id))))

(defn- parse-hook-permission! [state args]
  (when-not (#{"on" "off"} state) (die hooks-usage))
  (cond
    (empty? args) state
    (and (= state "off") (= 2 (count args)) (= "--until" (first args)))
    (str "off:until=" (canonical-iso! (second args)))
    :else (die hooks-usage)))

(defn- print-hooks []
  (doseq [hook (agent-hooks)]
    (println
     (format "%-34s %-13s %-24s %s"
             (get hook "id") (get hook "category" "uncategorized")
             (str (get hook "permission") " · "
                  (if (get hook "active") "active" "inactive"))
             (str (get-in hook ["owner" "repo"]) ":"
                  (get-in hook ["owner" "path"]))))))

(defn- mutate-hook-batch! [hooks permission label]
  (when-not (seq hooks) (die (str "no hooks matched " label)))
  (let [activation
        (north.agent-catalog/change-permissions!
         (into {} (map (fn [hook] [(get hook "id") permission])) hooks))]
    (println (str label " → " permission " · generation "
                  (get activation "generationId")))))

(defn cmd-hooks [args]
  (let [[verb & xs] args]
    (case (or verb "list")
      "list" (do (when (seq xs) (die hooks-usage)) (print-hooks))
      "explain" (let [[id & extra] xs]
                  (when (or (nil? id) (seq extra)) (die hooks-usage))
                  (let [hook (require-hook! id)]
                    (println (json/generate-string hook {:pretty true}))))
      ("on" "off")
      (let [[id & extra] xs
            hook (require-hook! id)
            permission (parse-hook-permission! verb extra)]
        (mutate-hook-batch! [hook] permission (str "hook " id)))
      "category"
      (let [[state category & extra] xs
            permission (parse-hook-permission! state extra)
            hooks (filterv #(= category (get % "category")) (agent-hooks))]
        (mutate-hook-batch! hooks permission (str "hook category " category)))
      "all"
      (let [[state & extra] xs
            permission (parse-hook-permission! state extra)]
        (mutate-hook-batch! (agent-hooks) permission "hooks all"))
      (die hooks-usage))))

(defn cmd-guards [[sub & extra]]
  (when (or (seq extra) (and sub (not (#{"on" "off"} sub))))
    (die "usage: north config guards [on|off]"))
  (let [hooks (filterv #(= "authoring" (get % "category")) (agent-hooks))]
    (if sub
      (mutate-hook-batch! hooks sub "authoring guards")
      (do
        (println (str "authoring guards: " (effective-ks)))
        (doseq [hook hooks]
          (println (str "  " (get hook "id") " · "
                        (get hook "permission") " · "
                        (if (get hook "active") "active" "inactive"))))))))
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
        "skills"   (cmd-skills rest)
        "sets"     (north.agent-catalog-cli/cmd-agents (cons "sets" rest))
        "agents"   (north.agent-catalog-cli/cmd-agents rest)
        "mcp"      (cmd-mcp rest)
        "audit"    (cmd-audit rest)
        "comms"    (cmd-comms rest)
        "routing"  (cmd-routing rest)
        "learning" (cmd-learning rest)
        ("help" "-h" "--help") (help)
        (die "usage: north config [status|dispatch|coord|guards|hooks|skills|sets|agents|mcp|audit|comms|routing|learning|help]")))
    (catch clojure.lang.ExceptionInfo error
      (die (.getMessage error)))))

(apply -main *command-line-args*)
