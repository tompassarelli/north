#!/usr/bin/env bash
# PreToolUse(Bash) guard: refuses a heavy build, full test suite or headless
# render started outside `machine-capacity run`. Unleased work lands in the
# caller's terminal scope, where admission neither counts nor caps it.
#
# Kill-switch: `north config agents off heavy-command-guard` or env
# AGENT_NO_AUTHORING_HOOKS (shared impl: lib/authoring-killswitch.sh).
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

hook_dir="${0%/*}"
authoring_killswitch="$hook_dir/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] \
  || authoring_killswitch="$hook_dir/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || true
type hook_decide >/dev/null 2>&1 || exit 0
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0

PY="$(cat <<'PYEOF'
import json, os, re, sys
sys.path.insert(0, sys.argv[1])
import shellcmd

def allow():
    sys.exit(0)

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(65)
tool = data.get("tool_name", "").rsplit(".", 1)[-1]
if tool not in {"Bash", "exec_command"}:
    allow()
tool_input = data.get("tool_input", {}) or {}
if isinstance(tool_input, str):
    try:
        tool_input = json.loads(tool_input)
    except Exception:
        allow()
if not isinstance(tool_input, dict):
    allow()
cmd = tool_input.get("command" if tool == "Bash" else "cmd", "")
cwd = tool_input.get("workdir") or data.get("cwd") or os.getcwd()
if not isinstance(cmd, str) or not cmd or not isinstance(cwd, str):
    allow()

tokens = shellcmd.words(cmd)
if tokens is None:
    allow()

PREFIXES = {"env", "exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time"}

declarations = {}

def farm_only(words, cwd):
    test_args = None
    if os.path.basename(words[0]) == "bun":
        if words[1:2] == ["test"]:
            test_args = words[2:]
        elif words[1:3] == ["run", "test"]:
            test_args = words[3:]
    if test_args is not None and any(
        not word.startswith("-") and ("/" in word or word.endswith((".ts", ".js", ".tsx")))
        for word in test_args
    ):
        return None
    if cwd not in declarations:
        entries = []
        try:
            import tomllib
            reader, writer = os.pipe()
            null = os.open(os.devnull, os.O_WRONLY)
            try:
                pid = os.posix_spawnp("git", ["git", "-C", cwd, "rev-parse", "--show-toplevel"], os.environ,
                    file_actions=[(os.POSIX_SPAWN_DUP2, writer, 1), (os.POSIX_SPAWN_DUP2, null, 2),
                                  (os.POSIX_SPAWN_CLOSE, reader)])
            finally:
                os.close(writer)
                os.close(null)
            with os.fdopen(reader) as output:
                root = output.read(4096).strip()
            if os.waitpid(pid, 0)[1] != 0:
                raise OSError("unresolved Git root")
            with open(os.path.join(root, ".agents", "farm-only.toml"), "rb") as source:
                entries = tomllib.load(source).get("command", [])
            if not isinstance(entries, list) or any(
                not isinstance(entry, dict)
                or not isinstance(entry.get("pattern"), list) or not entry["pattern"]
                or not all(isinstance(word, str) and word for word in entry["pattern"])
                or not isinstance(entry.get("narrowing", []), list)
                or not all(isinstance(flag, str) for flag in entry.get("narrowing", []))
                or not isinstance(entry.get("farm"), str)
                or not isinstance(entry.get("why"), str)
                for entry in entries
            ):
                entries = []
        except Exception:
            entries = []
        declarations[cwd] = entries
    for entry in declarations[cwd]:
        pattern = entry["pattern"]
        leading = [word for word in pattern if not word.startswith("-")]
        required = {word for word in pattern if word.startswith("-")}
        flags = {word.split("=", 1)[0] for word in words[len(leading):] if word.startswith("-")}
        if (words[:len(leading)] == leading and required <= flags
                and not flags.intersection(entry.get("narrowing", []))):
            return f"No. {entry['why']}. Run on the farm: {entry['farm']}"
    return None

def heavy(words):
    index = shellcmd.command(words, PREFIXES)
    rest = words[index:]
    if not rest:
        return None
    if any(re.search(r"machine-capacity(\.mjs)?$", w) for w in rest[:3]):
        return None
    name = os.path.basename(rest[0])
    args = rest[1:]
    plain = [a for a in args if not a.startswith("-")]
    if name == "cargo" and plain[:1] == ["build"]:
        return "cargo build"
    if name == "bun":
        if plain[:1] == ["test"] and not any("/" in a or a.endswith((".ts", ".js", ".tsx")) for a in plain[1:]):
            return "bun test (full suite)"
        if plain[:1] == ["wisp"] or (plain[:2] == ["run", "wisp"]):
            sub = plain[1:] if plain[0] == "wisp" else plain[2:]
            if sub[:2] == ["map", "build"]:
                return "wisp map build"
            if sub[:1] == ["view"] or (sub[:1] == ["headless"] and "--render" in args):
                return "wisp render"
    return None

def decide(tokens, cwd, leased=False, depth=0):
    if depth > 8:
        return None
    for segment in shellcmd.segments(tokens):
        rest = segment[shellcmd.command(segment, PREFIXES - {"env"}):]
        if rest and os.path.basename(rest[0]) == "env":
            index = 1
            env_cwd = cwd
            while index < len(rest):
                word = rest[index]
                if word in {"-u", "--unset", "-C", "--chdir"}:
                    if index + 1 >= len(rest):
                        break
                    if word in {"-C", "--chdir"}:
                        env_cwd = os.path.abspath(os.path.join(env_cwd, rest[index + 1]))
                    index += 2
                elif word.startswith("--chdir="):
                    env_cwd = os.path.abspath(os.path.join(env_cwd, word.split("=", 1)[1]))
                    index += 1
                elif word.startswith("-") or re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", word):
                    index += 1
                else:
                    break
            rest = rest[index:]
            if rest:
                reason = decide(rest, env_cwd, leased, depth + 1)
                if reason:
                    return reason
            continue
        if not rest:
            continue
        name = os.path.basename(rest[0])
        if name == "cd" and len(rest) == 2:
            cwd = os.path.abspath(os.path.join(cwd, os.path.expanduser(rest[1])))
            continue
        capacity = next((i for i, word in enumerate(rest[:3])
                         if re.search(r"machine-capacity(\.mjs)?$", word)), None)
        nested = None
        nested_lease = leased
        if capacity is not None and rest[capacity + 1:capacity + 2] == ["run"]:
            if "--" in rest[capacity + 2:]:
                nested = rest[rest.index("--", capacity + 2) + 1:]
                nested_lease = True
        elif name == "nix" and rest[1:2] == ["develop"]:
            marker = next((i for i, word in enumerate(rest[2:], 2)
                           if word in {"-c", "--command"}), None)
            if marker is not None:
                nested = rest[marker + 1:]
        elif name in {"bash", "sh"}:
            marker = next((i for i, word in enumerate(rest[1:], 1)
                           if word.startswith("-") and "c" in word[1:]), None)
            if marker is not None and marker + 1 < len(rest):
                nested = shellcmd.words(rest[marker + 1])
        if nested is not None:
            reason = decide(nested, cwd, nested_lease, depth + 1)
            if reason:
                return reason
            continue
        reason = farm_only(rest, cwd)
        if reason:
            return reason
        found = None if leased else heavy(segment)
        if found:
            return (
                f"No. `{found}` is heavy and would run in your terminal scope, outside capacity admission. "
                "Run it as: bun \"$(dirname \"$(agents path machine-capacity)\")/scripts/machine-capacity.mjs\" "
                "run --owner OWNER --timeout-seconds N -- COMMAND ARG... (omit --class to size from measured "
                "use; --class gpu for headless renders). Single test files and cargo check pass unwrapped."
            )
    return None

reason = decide(tokens, cwd)
if reason:
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }}))
allow()
PYEOF
)"

python_bin="${NORTH_AGENT_PYTHON:-python3}"
hook_decide "$python_bin" -I -S -c "$PY" "$hook_dir/lib"
exit 0
