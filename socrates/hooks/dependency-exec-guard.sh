#!/usr/bin/env bash
set -uo pipefail

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

[ "$payload_oversized" -eq 0 ] || exit 0

case "$payload" in
  *cargo*|*bun*|*npm*|*pnpm*|*yarn*|*npx*) ;;
  *) exit 0 ;;
esac

hook_dir="${0%/*}"
authoring_killswitch="$hook_dir/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] \
  || authoring_killswitch="$hook_dir/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || true
type hook_decide >/dev/null 2>&1 || exit 0
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0

active_list="${NORTH_AGENT_STATE_ROOT:-$HOME/.local/state/north/agents}/current/activation.active"
[ -z "${NORTH_AGENT_ACTIVATION:-}" ] || active_list="${NORTH_AGENT_ACTIVATION%.json}.active"
active_list="${NORTH_AGENT_ACTIVE:-$active_list}"
if [ -r "$active_list" ]; then
  active_count=0
  while IFS= read -r line || [ -n "$line" ]; do
    [ "$line" != 'hook dependency-exec-guard' ] || active_count=$((active_count + 1))
  done <"$active_list"
  [ "$active_count" -eq 1 ] || exit 0
fi

PY="$(cat <<'PYEOF'
import json, os, shlex, sys
sys.path.insert(0, sys.argv[1])
import shellcmd

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(65)
if not isinstance(data, dict):
    sys.exit(0)
tool = data.get("tool_name", "")
if not isinstance(tool, str):
    sys.exit(0)
tool = tool.rsplit(".", 1)[-1]
if tool not in {"Bash", "exec_command"}:
    sys.exit(0)
tool_input = data.get("tool_input", {}) or {}
if isinstance(tool_input, str):
    try:
        tool_input = json.loads(tool_input)
    except Exception:
        sys.exit(0)
if not isinstance(tool_input, dict):
    sys.exit(0)
cmd = tool_input.get("command" if tool == "Bash" else "cmd", "")
if not isinstance(cmd, str) or not cmd:
    sys.exit(0)
def decoded_words(command):
    if shellcmd.words(command) is None:
        return None
    segments = []
    segment = []
    comment = False
    for kind, word, quoted in shellcmd.tokenize(command):
        if comment:
            if kind == "sep" and word == "\n":
                comment = False
                segments.append(segment)
                segment = []
            continue
        if kind == "word" and not quoted and word.startswith("#"):
            comment = True
            continue
        if kind == "sep":
            segments.append(segment)
            segment = []
        else:
            segment.append(word)
    segments.append(segment)
    return segments

tokens = decoded_words(cmd)
if tokens is None:
    sys.exit(0)

PREFIXES = {"env", "exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time"}
BUN_FETCH = {"add", "update", "remove", "rm", "link", "x"}
CARGO_RUN = {"build", "b", "test", "t", "check", "c", "clippy", "run", "r", "bench", "doc"}
CARGO_FETCH = {"fetch", "update", "add", "install"}

def decide(tokens, depth=0):
    if depth > 8:
        raise ValueError("wrapper depth")
    for segment in tokens:
        rest = segment[shellcmd.command(segment, PREFIXES):]
        if not rest:
            continue
        name = os.path.basename(rest[0])
        if name in {"depbox", "cd"}:
            continue
        if name == "nix" and rest[1:2] in (["develop"], ["shell"]):
            marker = next((i for i, word in enumerate(rest[2:], 2)
                           if word in {"-c", "--command"}), None)
            if marker is not None:
                found = decide([rest[marker + 1:]], depth + 1)
                if found:
                    mode, command, postinstall = found
                    return mode, rest[:marker + 1] + command, postinstall
            continue
        if name in {"bash", "sh"}:
            marker = next((i for i, word in enumerate(rest[1:], 1)
                           if word.startswith("-") and "c" in word[1:]), None)
            if marker is not None and marker + 1 < len(rest):
                nested = decoded_words(rest[marker + 1])
                if nested is None:
                    raise ValueError("malformed nested command")
                found = decide(nested, depth + 1)
                if found:
                    mode, command, postinstall = found
                    return mode, rest[:marker + 1] + [shlex.join(command)] + rest[marker + 2:], postinstall
            continue
        args = rest[1:]
        if name == "bun":
            if args[:1] in (["install"], ["i"]):
                return "fetch", [rest[0], "install", "--frozen-lockfile", "--ignore-scripts"], True
            if args[:1] and (args[0] in BUN_FETCH or args[:2] == ["pm", "trust"]):
                return "fetch", rest, False
        if name in {"bunx", "npx", "npm", "pnpm", "yarn"}:
            return "fetch", rest, False
        if name == "cargo":
            subcommand = next((word for word in args if not word.startswith(("-", "+"))), None)
            if subcommand in CARGO_FETCH:
                return "fetch", rest, False
            if subcommand in CARGO_RUN:
                command = list(rest)
                for flag in ("--locked", "--offline"):
                    if flag not in command:
                        position = command.index("--") if "--" in command else len(command)
                        command.insert(position, flag)
                return "run", command, False
    return None

found = decide(tokens)
if found:
    mode, command, postinstall = found
    reason = "Dependency code must run inside depbox. Run: depbox " + mode + " -- " + shlex.join(command)
    if postinstall:
        reason += "; then depbox run -- bun run postinstall"
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }}))
PYEOF
)"

python_bin="${NORTH_AGENT_PYTHON:-python3}"
hook_decide "$python_bin" -I -S -c "$PY" "$hook_dir/lib"
exit 0
