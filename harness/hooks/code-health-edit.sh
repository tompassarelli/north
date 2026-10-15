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
  *'"Edit"'*|*'"Write"'*|*'"MultiEdit"'*|*'"apply_patch"'*) ;;
  *) exit 0 ;;
esac

authoring_killswitch="$(dirname "$0")/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] \
  || authoring_killswitch="$(dirname "$0")/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || true
type hook_decide >/dev/null 2>&1 || exit 0
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0

read -r -d '' PY <<'PYEOF' || true
import json, os, re, signal, subprocess, sys

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(65)
if not isinstance(data, dict):
    sys.exit(0)

SOURCE = (".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".rs", ".c", ".h")
HEADER = ("code-health: this edit adds structural debt; fix it now (split the file or function, "
          "or move the import behind an allowed module) using the program-craftsmanship skill:")
LIMIT = float(os.environ.get("CODE_HEALTH_EDIT_TIMEOUT", "1"))
MAX_LINES = 20

tool = str(data.get("tool_name") or "").split(".")[-1]
tool_input = data.get("tool_input")
if not isinstance(tool_input, dict):
    tool_input = {"command": tool_input} if isinstance(tool_input, str) else {}
cwd = data.get("cwd") if isinstance(data.get("cwd"), str) else os.getcwd()

raw = []
if tool in ("Edit", "Write", "MultiEdit"):
    raw = [tool_input.get("file_path") or tool_input.get("filePath")]
elif tool == "apply_patch":
    text = tool_input.get("command") or tool_input.get("input") or tool_input.get("patch") or ""
    if isinstance(text, list):
        text = "\n".join(str(part) for part in text)
    raw = re.findall(r"^\*\*\* (?:Add File|Update File|Move to):\s*(.+?)\s*$", str(text), re.M)

paths = []
for item in raw:
    if not isinstance(item, str) or not item:
        continue
    path = os.path.normpath(os.path.join(cwd, os.path.expanduser(item)))
    if path.endswith(SOURCE) and os.path.isfile(path) and path not in paths:
        paths.append(path)


def run(argv, where):
    try:
        proc = subprocess.Popen(argv, cwd=where, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, text=True, start_new_session=True)
    except OSError:
        return None
    try:
        out, _ = proc.communicate(timeout=LIMIT)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.communicate()
        return None
    return out if proc.returncode in (0, 1) else None


lines = []
for path in paths[:8]:
    top = run(["git", "-C", os.path.dirname(path), "rev-parse", "--show-toplevel"], cwd)
    top = (top or "").strip()
    if not top or not os.path.isfile(os.path.join(top, "structure.toml")):
        continue
    out = run(["code-health", "check", path], top)
    lines += [line for line in (out or "").splitlines() if line.strip()]

if lines:
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PostToolUse",
        "additionalContext": "\n".join([HEADER] + lines[:MAX_LINES]),
    }}))
PYEOF

NORTH_HOOK_ID=code-health-edit hook_decide "${NORTH_AGENT_PYTHON:-python3}" -c "$PY"
exit 0
