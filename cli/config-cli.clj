#!/usr/bin/env bb
;; north config — every personal-stack posture setting, one entry point.
;;
;;   dispatch : who runs agents        north SDK  vs  native Agent/Workflow
;;   coord    : coordination protocol  north / linear / both
;;   beagle   : code representation    text      vs  fact-native (per-file)
;;   guards   : authoring-guard hooks  + the kill-switch
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
(def REGISTRY        (or (System/getenv "GRAPH_UPSTREAM_REGISTRY")
                         (str home "/.config/fram/graph-upstream-files")))
(def HOOK-REGISTRY   (north.harness-dial/registry-path home))
(def ROUTING-POLICY  (or (System/getenv "NORTH_ROUTING_POLICY")
                         (str home "/.config/north/routing-policy.json")))

(defn- slurp' [f] (try (slurp f) (catch Exception _ nil)))
(defn- eprintln [& xs] (binding [*out* *err*] (apply println xs)))
(defn- die [& xs] (apply eprintln xs) (System/exit 1))

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

(defn registry-raw []
  (if-let [c (slurp' REGISTRY)]
    (if (str/blank? c) [] (str/split-lines c))
    []))

(defn registry-lines []
  (->> (registry-raw)
       (remove #(re-matches #"\s*(#.*)?" %)))) ; drop blank + comment lines

(defn adopted-n [] (count (registry-lines)))

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

;; --- the report -----------------------------------------------------------
(defn banner []
  (let [rule  (apply str (repeat 66 "─"))
        label "  NORTH CONFIG — every setting, one report"
        d     (today)
        gap   (max 1 (- 66 (count label) (count d) 7))]
    (str "╭" rule "╮\n"
         "│" label (apply str (repeat gap " ")) d "       │\n"
         "╰" rule "╯")))

(defn files-block []
  (let [ls (registry-lines)]
    (if (seq ls)
      (str/join "\n"
                (map #(str "       "
                           (if (str/starts-with? % home)
                             (str "~" (subs % (count home)))
                             %))
                     ls))
      "       (none)")))

(defn- hook-verdict [id]
  (north.harness-dial/hook-verdict #(get' % nil) (hook-registry) id))

(defn- hooks-summary []
  (let [hooks (hook-registry)
        executable (count (filter #(= "EXEC" (hook-path-status %)) hooks))]
    (str executable "/" (count hooks) " executable")))

(defn status []
  (let [d  (north.harness-state/get-dispatch-mode home)
        c  (get' "coord" "north")
        ]
    (println (banner))
    (println (str "
 1  DISPATCH   type × enforcement — who runs agents, how strictly  [guard: " (wired "agent-spawn-guard") "]
" (dispatch-status-lines d) "
    flip → north config dispatch " (north.dispatch-mode/usage) "

 2  COORD      coordination protocol           [north: " (north-daemon) " · linear MCP: " (linear-mcp) "]
    " (mark c "north") " north    facts on :7977 + concerns + msg-cli chat
    " (mark c "linear") " linear   Linear as the work queue (MCP)
    " (mark c "both") " both     Linear as consolidation layer over north
    note: declarative — agents read this posture; no hard enforcement yet
    flip → north config coord north|linear|both

 3  BEAGLE     code as text vs facts          [guard: " (wired "code-upstream-guard") "]
    fact-native adopted (text edits denied → fram graph tools): " (adopted-n) " file(s)
" (files-block) "
    default-flip: PARKED — pending M1.5-vs-M2 bake-off verdict
    flip → north config beagle adopt|unadopt <absolute-path> · north config beagle list

 4  GUARDS     authoring-guard hooks           kill-switch: " (effective-ks) "
    " (wired "agent-spawn-guard") " agent-spawn-guard   " (wired "code-upstream-guard") " upstream:graph   " (wired "firn-guard") " firn
    " (wired "tripwire-guard") " tripwire            " (wired "racket-build-guard") " racket-build      " (wired "beagle-session-start") " beagle-session
    [live]   flip authoring guards → north config guards on|off   (persists, all sessions; dispatch remains independent)
    [launch] one session → CLAUDE_NO_AUTHORING_HOOKS=1 claude   (launch ONLY — mid-session flip impossible; per-command prefix does nothing; 0/false forces guards live)

 5  ROUTING    provider targets + entitlement envelopes
    " (routing-summary (routing-read)) "
    pressure: automatic usage sensing; manual command is a temporary override/fallback
    configure → north config routing
    policy: " ROUTING-POLICY "

 6  HOOKS      per-hook and per-category runtime dials   [" (hooks-summary) "]
    precedence: item > category > all > default(on); coordination is excluded from all
    configure → north config hooks · north config hooks explain <hook-id>

 elsewhere: system/nix settings → firn tag status · session effort → /effort
 dials: [live] north config flip, effective now · [launch] env at claude launch, frozen for session · [spawn] request-owned routing; managed compression defaults off when no request/env exists
 state: ~/.local/state/north/harness.conf · legacy read fallback: ~/.claude/my-config.state · descriptions + advice: north config help"))))

(defn help []
  (println (str "north config — every personal-stack posture setting, one entry point.

 1 DISPATCH — TYPE (native vs managed, who executes) × ENFORCEMENT
   (forced vs biased, how strictly) = four modes.
" (dispatch-help-lines) "
   Legacy values native/warn/north are still accepted and map to the
   canonical name above (printed as a one-line note on use).
   Advice: stay on managed-forced. Drop to managed-biased only when the
   daemon is down.

 2 COORD — source of truth for work coordination.
   north / linear / both (Linear as consolidation layer over north).
   Declarative for now: agents read this posture; nothing mechanically
   blocks the other system yet. Flipping the option does not build the sync.
   Advice: north.

 3 BEAGLE — how Beagle source is authored, per file.
   text          ordinary Edit/Write; the beagle-authoring repair loop.
   fact-native  file is a regenerable view of the fram fact graph; text
                 edits DENIED (code-upstream-guard); author via
                 mcp__fram__* graph tools. Adoption is PER-FILE: the
                 registry (~/.config/fram/graph-upstream-files) or a
                 first-line `;; @upstream:graph` sentinel. The cascade
                 (skill, guard, repair loop vs recompile gate) keys off
                 adoption automatically.
   Advice: don't flip the default until the M1.5-vs-M2 bake-off verdict.

 4 GUARDS — the PreToolUse/SessionStart authoring guards.
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

 5 ROUTING — durable provider selection and subscription-entitlement policy.
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

 6 HOOKS — runtime control for every registered hook.
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

 Elsewhere (owned by other CLIs, not duplicated here):
   system/nix composition → firn tag status · firn enable <tag>
   session effort/ultracode → /effort (harness-level, not script-readable)")))

;; --- verb dispatch --------------------------------------------------------

(defn cmd-dispatch [[sub & extra]]
  (cond
    (= sub "--canonical")
    (if (seq extra)
      (die "usage: north config dispatch --canonical")
      (println (north.harness-state/get-dispatch-mode home)))

    (= sub "--guard-action")
    (if (seq extra)
      (die "usage: north config dispatch --guard-action")
      (println
       (north.dispatch-mode/guard-action
        (north.harness-state/get-dispatch-mode home))))

    (= sub "--managed-admission")
    (if (seq extra)
      (die "usage: north config dispatch --managed-admission")
      (println
       (north.dispatch-mode/managed-admission
        (north.harness-state/get-dispatch-mode home))))

    (north.dispatch-mode/recognized? sub)
    (let [canon (north.dispatch-mode/normalize sub)
          legacy? (north.dispatch-mode/legacy-alias? sub)
          summary (:summary (north.dispatch-mode/spec canon))]
      (put' "dispatch" canon)
      (println
       (if legacy?
         (str "dispatch → " canon " (legacy alias '" sub
              "' accepted; canonical name is '" canon "')")
         (str "dispatch → " canon " (" summary ")"))))

    (nil? sub)
    (let [d (north.harness-state/get-dispatch-mode home)]
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

(defn cmd-rebuild-coordination [[sub]]
  (cond
    (#{"on" "off"} sub)
    (do
      (put' "rebuild-coordination" sub)
      (println (str "rebuild-coordination → " sub)))
    (nil? sub)
    (let [r (get' "rebuild-coordination" "off")]
      (println (str "rebuild-coordination = " r
                    "   (default off; north config rebuild-coordination on|off)")))
    :else
    (die "usage: north config rebuild-coordination [on|off]")))

(defn cmd-beagle [[sub path]]
  (case (or sub "list")
    "list"
    (do (println (str "fact-native adopted files (" (adopted-n) "):"))
        (let [ls (registry-lines)]
          (if (seq ls) (doseq [l ls] (println l)) (println "  (none)"))))
    "adopt"
    (cond
      (nil? path) (die "usage: north config beagle adopt </absolute/path>")
      (not (.isFile (io/file path))) (die (str "no such file: " path))
      :else
      (do (io/make-parents REGISTRY)
          (when-not (some #{path} (registry-raw))
            (spit REGISTRY (str path "\n") :append true))
          (println (str "adopted fact-native: " path " (text edits now denied; use mcp__fram__* graph tools)"))))
    "unadopt"
    (if (nil? path)
      (die "usage: north config beagle unadopt </absolute/path>")
      (let [kept (remove #{path} (registry-raw))]
        (spit REGISTRY (if (seq kept) (str (str/join "\n" kept) "\n") ""))
        (println (str "un-adopted (text mode again): " path))))
    (die "usage: north config beagle [list|adopt <path>|unadopt <path>]")))

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
        (doseq [g ["agent-spawn-guard" "code-upstream-guard" "firn-guard"
                   "tripwire-guard" "racket-build-guard" "beagle-session-start"]]
          (println (str "  " (wired g) " " g))))
    :else (die "usage: north config guards [on|off]")))

(defn -main [& args]
  (try
    (let [[verb & rest] args]
      (case (or verb "status")
        ("status") (status)
        "dispatch" (cmd-dispatch rest)
        "coord"    (cmd-coord rest)
        "rebuild-coordination" (cmd-rebuild-coordination rest)
        "beagle"   (cmd-beagle rest)
        "guards"   (cmd-guards rest)
        "hooks"    (cmd-hooks rest)
        "routing"  (cmd-routing rest)
        ("help" "-h" "--help") (help)
        (die "usage: north config [status|dispatch|coord|rebuild-coordination|beagle|guards|hooks|routing|help]")))
    (catch clojure.lang.ExceptionInfo error
      (die (.getMessage error)))))

(apply -main *command-line-args*)
