#!/usr/bin/env bash
# PreToolUse guard — refuses session-killing signal shapes: broadcast kill
# (`kill -1`), user-wide process sweeps (`pkill -u` with no pattern,
# `killall -u`), compositor kills (pkill/killall niri), and login-session
# teardown (`loginctl terminate-*`/`kill-*`, `systemctl --user exit`,
# stop/kill/restart of `user@*` or the compositor unit).
# ============================================================================
# kill(-1, SIG) signals EVERY process the user owns — the compositor, the
# user manager, the login shell, and every other agent — in one syscall. The
# scoped alternatives (a specific PID, a unique -f pattern, a named unit the
# agent itself started) stay allowed, as do quoted mentions in commit
# messages and heredoc bodies: only a command-position invocation is denied.
#
# Kill-switch: persistent `north config guards off` (state) OR env
# CLAUDE_NO_AUTHORING_HOOKS / AGENT_NO_AUTHORING_HOOKS (any value but
# 0/false; 0/false forces guards live). Shared impl: lib/authoring-killswitch.sh.
# ============================================================================
set -uo pipefail

# Drain before every decision, including the kill-switch. Keep active-path input
# memory-bounded; an oversized envelope follows the existing malformed fail-open.
capture_hook_stdin() {
  local chunk status keep
  local LC_ALL=C
  payload=""
  payload_oversized=0
  while :; do
    chunk=""
    IFS= read -r -N 65536 chunk
    status=$?
    if [ -n "$chunk" ]; then
      keep=$((1048576 - ${#payload}))
      [ "$keep" -le 0 ] || payload+="${chunk:0:$keep}"
      [ "${#chunk}" -le "$keep" ] || payload_oversized=1
    fi
    [ "$status" -eq 0 ] || break
  done
}
capture_hook_stdin

# shellcheck disable=SC1090,SC1091
. "$(dirname "$0")/lib/authoring-killswitch.sh" 2>/dev/null || true
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0
[ "$payload_oversized" -eq 0 ] || exit 0

# Fast-path: only Bash commands naming a signal or session verb are candidates.
case "$payload" in
  *kill*|*loginctl*|*systemctl*) ;;
  *) exit 0 ;;
esac

read -r -d '' PY <<'PYEOF' || true
import sys, json, re

def allow():
    sys.exit(0)

try:
    data = json.load(sys.stdin)
except Exception:
    allow()

if data.get("tool_name", "") != "Bash":
    allow()

cmd = (data.get("tool_input", {}) or {}).get("command", "") or ""
if not cmd:
    allow()

# --- Strip heredoc bodies, then quoted segments, so prose that merely
# mentions a trigger phrase is never treated as an invocation. Blanking (not
# deleting) preserves newline structure for the command-position anchors. ---

_HEREDOC_START = re.compile(r"<<-?\s*(['\"]?)(\w+)\1")

def _blank(text):
    return "".join(c if c == "\n" else " " for c in text)

def strip_heredocs(s):
    out = []
    i, n = 0, len(s)
    while i < n:
        m = _HEREDOC_START.search(s, i)
        if not m:
            out.append(s[i:])
            break
        out.append(s[i:m.end()])
        line_end = s.find("\n", m.end())
        if line_end == -1:
            out.append(s[m.end():])
            break
        out.append(s[m.end():line_end + 1])
        body_start = line_end + 1
        delim = m.group(2)
        term = re.compile(r"^[ \t]*" + re.escape(delim) + r"[ \t]*$", re.M)
        tm = term.search(s, body_start)
        if tm:
            out.append(_blank(s[body_start:tm.start()]))
            i = tm.start()
        else:
            out.append(_blank(s[body_start:]))
            i = n
    return "".join(out)

def strip_quotes(s):
    out = []
    i, n = 0, len(s)
    while i < n:
        c = s[i]
        if c == "'":
            j = s.find("'", i + 1)
            end = n if j == -1 else j + 1
            out.append(_blank(s[i:end]))
            i = end
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if s[j] == "\\" and j + 1 < n:
                    j += 2
                    continue
                if s[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(_blank(s[i:j]))
            i = j
            continue
        out.append(c)
        i += 1
    return "".join(out)

cleaned = strip_quotes(strip_heredocs(cmd))

SEP = r"(?:^|[\n;&|({`])"
WRAP = r"(?:(?:sudo|doas|command|exec|nohup|setsid)\s+)*"
ARG_SPAN = r"[^\n;&|)}`]*"

CMD_RE = re.compile(
    SEP + r"\s*" + WRAP + r"(kill|pkill|killall|loginctl|systemctl)\s+("
    + ARG_SPAN + r")")

# The compositor and its session units: killing these IS killing the session.
SESSION_PROCS = {"niri", "niri-session"}
SESSION_UNITS = {"niri", "niri.service", "graphical-session.target"}

SIG_FLAG = re.compile(r"^-(?:[0-9]+|[A-Za-z]+)$")

def kill_hit(tokens):
    # Separate signal-spec tokens from PID operands; the broadcast shape is
    # `-1` in OPERAND position (`kill -9 -1`, `kill -- -1`). `kill -1 <pid>`
    # is SIGHUP to a pid and stays allowed.
    operands = []
    i, n = 0, len(tokens)
    seen_ddash = False
    while i < n:
        t = tokens[i]
        if not seen_ddash:
            if t == "--":
                seen_ddash = True
                i += 1
                continue
            if t in ("-l", "-L"):
                return None
            if t in ("-s", "-n", "--signal") and i + 1 < n:
                i += 2
                continue
            if i == 0 and SIG_FLAG.match(t) and (t != "-1" or n > 1):
                i += 1
                continue
        operands.append(t)
        i += 1
    if "-1" in operands:
        return "broadcast kill -1"
    return None

VALUE_FLAGS = {"-u", "-U", "--uid", "--euid", "-g", "-G", "--group",
               "-s", "--signal", "-P", "--parent", "-t", "--terminal",
               "-F", "--pidfile", "-d", "--delay", "-o", "-n"}

def pkill_hit(tokens):
    has_user = False
    operands = []
    skip = False
    for t in tokens:
        if skip:
            skip = False
            continue
        if t in ("-u", "-U") or t.startswith(("--uid", "--euid")):
            has_user = True
            if t in VALUE_FLAGS:
                skip = True
            continue
        if t in VALUE_FLAGS:
            skip = True
            continue
        if t.startswith("-"):
            continue
        operands.append(t)
    if has_user and not operands:
        return "user-wide pkill sweep"
    if any(o in SESSION_PROCS for o in operands):
        return "compositor kill"
    return None

def killall_hit(tokens):
    if any(t in ("-u", "--user") for t in tokens):
        return "user-wide killall sweep"
    if any(t in SESSION_PROCS for t in tokens):
        return "compositor kill"
    return None

LOGINCTL_VERBS = {"terminate-user", "kill-user", "terminate-session",
                  "kill-session", "terminate-seat"}

def loginctl_hit(tokens):
    for t in tokens:
        if t.startswith("-"):
            continue
        if t in LOGINCTL_VERBS:
            return "login-session teardown"
        return None
    return None

def systemctl_hit(tokens):
    user_mode = "--user" in tokens
    verb = None
    args = []
    for t in tokens:
        if t.startswith("-"):
            continue
        if verb is None:
            verb = t
            continue
        args.append(t)
    if verb is None:
        return None
    if user_mode and verb == "exit":
        return "user-manager exit"
    if verb == "isolate" and any(a.startswith("exit") for a in args):
        return "user-manager exit"
    if verb in ("stop", "kill", "restart"):
        if any(a.startswith("user@") for a in args):
            return "user-manager teardown"
        if user_mode and any(a in SESSION_UNITS for a in args):
            return "compositor teardown"
    return None

hit = None
for m in CMD_RE.finditer(cleaned):
    name = m.group(1)
    tokens = m.group(2).split()
    if name == "kill":
        hit = kill_hit(tokens)
    elif name == "pkill":
        hit = pkill_hit(tokens)
    elif name == "killall":
        hit = killall_hit(tokens)
    elif name == "loginctl":
        hit = loginctl_hit(tokens)
    elif name == "systemctl":
        hit = systemctl_hit(tokens)
    if hit:
        break

if hit is None:
    allow()

reason = (
    "BLOCKED: session-killing shape refused (%s) — this signals the whole "
    "desktop session (compositor, user manager, every agent), not just your "
    "target. Signal only processes YOU started, by specific PID (`kill <pid>`)"
    " or a unique pattern (`pkill -f '<unique-pattern>'`); manage your own "
    "units by exact name. Session/compositor teardown is the operator's call. "
    "Prose that mentions these phrases is unaffected — only a command-position "
    "invocation is denied. Rare explicit override: `north config guards off` "
    "(persistent, live), or a session LAUNCHED with AGENT_NO_AUTHORING_HOOKS=1."
) % hit

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }
}))
sys.exit(0)
PYEOF

printf '%s' "$payload" | python3 -c "$PY"
