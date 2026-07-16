#!/usr/bin/env bb
;; north config — every personal-stack posture setting, one entry point.
;;
;;   caveman  : output compression     session level + worker default
;;   dispatch : who runs agents        north SDK  vs  native Agent/Workflow
;;   coord    : coordination protocol  north / linear / both
;;   beagle   : code representation    text      vs  fact-native (per-file)
;;   guards   : authoring-guard hooks  + the kill-switch
;;
;; Ported from dotfiles/bin/my-agent-config (bash) 2026-07-10: north is the
;; top-level settings surface. Output contract is byte-faithful to the bash tool
;; (self-references now read `north config`); the slash command renders it verbatim.
;;
;; State file STAYS at ~/.claude/my-config.state — the guard hooks
;; (agent-spawn-guard.sh, north-clock-guard.sh) and the kill-switch lib read it
;; live; changing the path breaks enforcement. The kill-switch precedence below
;; is a faithful inline copy of hooks/lib/authoring-killswitch.sh so this report
;; and that enforcement can never disagree.

(require '[clojure.string :as str]
         '[clojure.java.io :as io])

(def home (System/getenv "HOME"))
(def STATE           (str home "/.claude/my-config.state"))
(def CAVEMAN-STATE   (str home "/.claude/.caveman-active"))
(def CAVEMAN-DEFAULT (str home "/code/nixos-config/dotfiles/caveman/config.json"))
(def REGISTRY        (or (System/getenv "GRAPH_UPSTREAM_REGISTRY")
                         (str home "/.config/fram/graph-upstream-files")))
(def SETTINGS        (str home "/code/nixos-config/dotfiles/claude/settings.json"))

(defn- slurp' [f] (try (slurp f) (catch Exception _ nil)))
(defn- eprintln [& xs] (binding [*out* *err*] (apply println xs)))
(defn- die [& xs] (apply eprintln xs) (System/exit 1))

;; --- state accessors (STATE = key=value lines; last wins) -----------------
(defn get' [k default]
  (let [prefix (str k "=")]
    (if-let [c (slurp' STATE)]
      (if-let [line (->> (str/split-lines c)
                         (filter #(str/starts-with? % prefix))
                         last)]
        (subs line (count prefix))
        default)
      default)))

(defn put' [k v]
  (io/make-parents STATE)
  (let [c (or (slurp' STATE) "")
        prefix (str k "=")
        lines (if (str/blank? c) [] (str/split-lines c))
        kept  (remove #(str/starts-with? % prefix) lines)]
    (spit STATE (str (str/join "\n" (concat kept [(str k "=" v)])) "\n"))))

(defn mark [a b] (if (= a b) "●" "○")) ; ● / ○

;; --- environment probes ---------------------------------------------------
(defn north-daemon []
  (try (with-open [s (java.net.Socket.)]
         (.connect s (java.net.InetSocketAddress. "127.0.0.1" 7977) 300))
       "up"
       (catch Exception _ "DOWN")))

(defn linear-mcp []
  (let [c (slurp' (str home "/.claude.json"))]
    (if (and c (str/includes? c "linear")) "configured" "absent")))

(defn wired [x]
  (let [c (slurp' SETTINGS)]
    (if (and c (str/includes? c x)) "✓" "✗"))) ; ✓ / ✗

(defn registry-raw []
  (if-let [c (slurp' REGISTRY)]
    (if (str/blank? c) [] (str/split-lines c))
    []))

(defn registry-lines []
  (->> (registry-raw)
       (remove #(re-matches #"\s*(#.*)?" %)))) ; drop blank + comment lines

(defn adopted-n [] (count (registry-lines)))

(defn caveman-lvl []
  (let [c (slurp' CAVEMAN-STATE)]
    (if (and c (not (str/blank? c))) (str/trim c) "off")))

(defn caveman-default []
  (or (some-> (slurp' CAVEMAN-DEFAULT)
              (->> (re-find #"\"defaultMode\"\s*:\s*\"([^\"]*)\""))
              second)
      "full"))

;; Kill-switch effective state — precedence identical to authoring-killswitch.sh:
;;   env 0|false  → force-live (state ignored this session)
;;   env non-empty (other) → engaged this session
;;   unset/empty  → state file `guards=off` decides
(defn effective-ks []
  (let [env (System/getenv "CLAUDE_NO_AUTHORING_HOOKS")]
    (cond
      (#{"0" "false"} env) "env force-live — guards LIVE (state ignored this session)"
      (and env (not (str/blank? env))) "ENGAGED via env (this session) — ALL GUARDS OFF"
      :else (if (= "off" (get' "guards" ""))
              "ENGAGED via state — ALL GUARDS OFF (north config guards on restores)"
              "off — guards LIVE"))))

(defn today []
  (.format (java.time.LocalDate/now)
           (java.time.format.DateTimeFormatter/ofPattern "yyyy-MM-dd")))

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

(defn status []
  (let [d  (get' "dispatch" "north")
        c  (get' "coord" "north")
        cv (caveman-lvl)
        cd (caveman-default)
        ac (or (System/getenv "AGENT_CAVEMAN") "full (SDK default)")]
    (println (banner))
    (println (str "
 1  CAVEMAN    output compression
    session: " cv " (lite|full|ultra + wenyan-*)      workers: " ac " (inherited at spawn)
    default: " cd " (persists — new sessions start here)
    [live]   session → north config caveman lite|full|ultra   (or /caveman)
    [live]   default → north config caveman default off|lite|full|ultra|wenyan-*
    [spawn]  one worker → spawn {caveman: off|lite|full}   (mcp__north__spawn param)
    [launch] all workers from a session → AGENT_CAVEMAN=off|lite|full claude

 2  DISPATCH   who runs agents                 [guard: " (wired "agent-spawn-guard") "]
    " (mark d "north") " north    SDK workers — persistent, steerable, fact trail;
               model, effort, caveman all have per-spawn opts on mcp__north__spawn;
               without them, workers inherit AGENT_MODEL + AGENT_CAVEMAN from the
               spawning session's env ([spawn] — inherited at spawn, frozen)
    " (mark d "warn") " warn     native Agent/Workflow allowed, nudged toward north
    " (mark d "native") " native   raw Claude Code spawns, no interference
    flip → north config dispatch north|warn|native

 3  COORD      coordination protocol           [north: " (north-daemon) " · linear MCP: " (linear-mcp) "]
    " (mark c "north") " north    facts on :7977 + concerns + msg-cli chat
    " (mark c "linear") " linear   Linear as the work queue (MCP)
    " (mark c "both") " both     Linear as consolidation layer over north
    note: declarative — agents read this posture; no hard enforcement yet
    flip → north config coord north|linear|both

 4  BEAGLE     code as text vs facts          [guard: " (wired "code-upstream-guard") "]
    fact-native adopted (text edits denied → fram graph tools): " (adopted-n) " file(s)
" (files-block) "
    default-flip: PARKED — pending M1.5-vs-M2 bake-off verdict
    flip → north config beagle adopt|unadopt <absolute-path> · north config beagle list

 5  GUARDS     authoring-guard hooks           kill-switch: " (effective-ks) "
    " (wired "agent-spawn-guard") " agent-spawn-guard   " (wired "code-upstream-guard") " upstream:graph   " (wired "firn-guard") " firn
    " (wired "tripwire-guard") " tripwire            " (wired "racket-build-guard") " racket-build      " (wired "beagle-session-start") " beagle-session
    [live]   flip all → north config guards on|off   (persists, all sessions)
    [launch] one session → CLAUDE_NO_AUTHORING_HOOKS=1 claude   (launch ONLY — mid-session flip impossible; per-command prefix does nothing; 0/false forces guards live)

 elsewhere: system/nix settings → firn tag status · session effort → /effort
 dials: [live] north config flip, effective now · [launch] env at claude launch, frozen for session · [spawn] per-worker opt or inherited env, frozen at spawn
 state: ~/.claude/my-config.state · descriptions + advice: north config help"))))

(defn help []
  (println "north config — every personal-stack posture setting, one entry point.

 1 CAVEMAN — output compression (token economy).
   Three binding classes:
   [live]   session — north config caveman lite|full|ultra (or /caveman);
            reads ~/.claude/.caveman-active; effective immediately.
   [live]   default — north config caveman default off|lite|full|ultra|wenyan-*;
            new sessions start here; persists across sessions.
   [spawn]  one worker — pass {caveman: off|lite|full} on mcp__north__spawn;
            frozen for that worker's lifetime.
   [launch] all workers from a session — AGENT_CAVEMAN=off|lite|full claude;
            inherited at spawn by workers without a per-spawn override;
            frozen for the session; mid-session flip impossible.
   lite/full/ultra + wenyan variants. Code/commits/quoted errors/security
   are never compressed at any level.
   Global default (new sessions start here) resolution order:
     CAVEMAN_DEFAULT_MODE env > repo-local .caveman.json
       > ~/.config/caveman/config.json (\"defaultMode\" field) > \"full\"
   ~/.config/caveman/config.json is a home-manager out-of-store symlink
   into ~/code/nixos-config/dotfiles/caveman/config.json — edit via
   `north config caveman default <mode>`, then commit in nixos-config.
   One-time: `firn rebuild` wires the symlink if not already present.
   flip default → north config caveman default off|lite|full|ultra|wenyan-*
   Advice: full for coordination, lite for high-stakes design review,
   never ultra/wenyan for substantive work (lossy — PLAYBOOK 2026-06-22).

 2 DISPATCH — who executes agent work.
   north   (default) native Agent/Task/Workflow calls are DENIED by a
           PreToolUse hook and redirected to the north SDK: mcp__north__spawn
           (ad-hoc) / mcp__north__dispatch (thread-driven). SDK workers are
           persistent, dormant-until-pinged, observable (web :8088),
           steerable (msg-cli :7977). Model, effort, and caveman all have
           per-spawn opts on mcp__north__spawn; without them, workers inherit
           AGENT_MODEL + AGENT_CAVEMAN from the spawning session's env
           ([spawn] — inherited at spawn, frozen for each worker's lifetime).
   warn    native spawns allowed; the hook injects a reminder instead.
   native  no interference. For A/B baselines against stock Claude Code.
   Advice: stay on north. Drop to warn only when the daemon is down.

 3 COORD — source of truth for work coordination.
   north / linear / both (Linear as consolidation layer over north).
   Declarative for now: agents read this posture; nothing mechanically
   blocks the other system yet. Flipping the option does not build the sync.
   Advice: north.

 4 BEAGLE — how Beagle source is authored, per file.
   text          ordinary Edit/Write; the beagle-authoring repair loop.
   fact-native  file is a regenerable view of the fram fact graph; text
                 edits DENIED (code-upstream-guard); author via
                 mcp__fram__* graph tools. Adoption is PER-FILE: the
                 registry (~/.config/fram/graph-upstream-files) or a
                 first-line `;; @upstream:graph` sentinel. The cascade
                 (skill, guard, repair loop vs recompile gate) keys off
                 adoption automatically.
   Advice: don't flip the default until the M1.5-vs-M2 bake-off verdict.

 5 GUARDS — the PreToolUse/SessionStart authoring guards.
   Individually wired in ~/code/nixos-config/dotfiles/claude/settings.json.
   Kill-switch is VALUE-AWARE and has two surfaces:

   [live] state flip (primary — effective immediately across ALL sessions,
   no relaunch; hooks re-read state on every call):
     north config guards off   → writes guards=off to ~/.claude/my-config.state
     north config guards on    → removes that line (or writes guards=on)

   [launch] env override — single session, launch ONLY; mid-session flip
   impossible; per-command env prefix does nothing (claude reads it at
   start, then frozen for the session):
     CLAUDE_NO_AUTHORING_HOOKS=1 claude     all guards OFF this session
     CLAUDE_NO_AUTHORING_HOOKS=0 claude     force-live (state ignored)
   Any non-empty value other than 0/false kills guards; 0 or false forces
   them live. Env beats state. Semantics live in the shared lib sourced by
   every guard hook AND by this verb:
     ~/.claude/hooks/lib/authoring-killswitch.sh

 Elsewhere (owned by other CLIs, not duplicated here):
   system/nix composition → firn tag status · firn enable <tag>
   session effort/ultracode → /effort (harness-level, not script-readable)"))

;; --- verb dispatch --------------------------------------------------------
(def caveman-modes #{"lite" "full" "ultra" "wenyan-lite" "wenyan-full" "wenyan-ultra"})
(def caveman-default-modes (conj caveman-modes "off"))

(defn cmd-caveman [[sub arg]]
  (cond
    (= sub "default")
    (cond
      (caveman-default-modes arg)
      (do (io/make-parents CAVEMAN-DEFAULT)
          (spit CAVEMAN-DEFAULT (str "{\"defaultMode\":\"" arg "\"}\n"))
          (println (str "caveman default → " arg " (written to ~/code/nixos-config/dotfiles/caveman/config.json)"))
          (let [link (str home "/.config/caveman/config.json")
                canon (try (.getCanonicalPath (io/file link)) (catch Exception _ nil))]
            (when (not= canon CAVEMAN-DEFAULT)
              (eprintln "  ⚠  ~/.config/caveman/config.json not yet linked — run: firn rebuild")))
          (println "  note: change lives in nixos-config — commit it there"))
      (nil? arg)
      (println (str "caveman default = " (caveman-default) "   (north config caveman default <mode>)"))
      :else
      (die "usage: north config caveman default [off|lite|full|ultra|wenyan-lite|wenyan-full|wenyan-ultra]"))

    (caveman-modes sub)
    (do (spit CAVEMAN-STATE sub)
        (println (str "caveman session level → " sub " (plugin reads ~/.claude/.caveman-active)")))

    (nil? sub)
    (println (str "caveman = " (caveman-lvl) "   default = " (caveman-default)
                  "   (north config caveman lite|full|ultra|default <mode>; off → say 'stop caveman' / use /caveman)"))

    (= sub "off")
    (die "turn off via the plugin: say 'stop caveman' or /caveman — plugin owns the off-path")

    :else
    (die "usage: north config caveman [default <mode>|lite|full|ultra|wenyan-lite|wenyan-full|wenyan-ultra]")))

(defn cmd-dispatch [[sub]]
  (cond
    (#{"north" "warn" "native"} sub)
    (do
      (put' "dispatch" sub)
      (println (str "dispatch → " sub " "
                    (cond (= sub "north") "(native Agent/Workflow now DENIED → north SDK)"
                          (= sub "warn")  "(native allowed, nudged)"
                          :else           "(native allowed, silent)"))))
    (nil? sub)
    (let [d (get' "dispatch" "north")]
      (println (str "dispatch = " d "   (north config dispatch north|warn|native)")))
    :else
    (die "usage: north config dispatch [north|warn|native]")))

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
  (let [[verb & rest] args]
    (case (or verb "status")
      ("status") (status)
      "caveman"  (cmd-caveman rest)
      "dispatch" (cmd-dispatch rest)
      "coord"    (cmd-coord rest)
      "beagle"   (cmd-beagle rest)
      "guards"   (cmd-guards rest)
      ("help" "-h" "--help") (help)
      (die "usage: north config [status|caveman|dispatch|coord|beagle|guards [on|off]|help]"))))

(apply -main *command-line-args*)
