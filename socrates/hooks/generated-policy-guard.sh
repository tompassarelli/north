#!/usr/bin/env bash
# PreToolUse guard — refuses hand edits of generated files: ~/.claude/CLAUDE.md,
# the ~/.codex files `agents sync` projects (AGENTS.md, skills/, prompts/), and
# any X.nix beside an X.bnix. Covers Edit/Write/MultiEdit/NotebookEdit,
# apply_patch, and shell redirection, sed -i, sd, tee, cp, mv, install, ln and
# rsync onto those paths. Reads, quoted mentions and `agents sync` pass.
#
# Kill-switch: persistent `north config agents off generated-policy-guard` OR env
# AGENT_NO_AUTHORING_HOOKS (any value but 0/false). Shared impl: lib/authoring-killswitch.sh.
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
  *CLAUDE.md*|*.codex*|*.nix*) ;;
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
import sys, json, os, re
sys.path.insert(0, sys.argv[1])
from shellcmd import words, segments, command_index

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(65)

POLICY = ("Never hand-edit generated agent policy (~/.claude/CLAUDE.md, ~/.codex). "
          "Edit south:agents/ or north:agent-machinery/ and run `agents sync`.")
NIX = "Never hand-edit a .nix generated from .bnix. Edit the .bnix and regenerate the .nix from it."

home = os.path.expanduser("~")
codex = os.path.join(home, ".codex")
cwd = data.get("cwd") or os.getcwd()


def located(raw):
    raw = re.sub(r"^(?:\$HOME|\$\{HOME\})(?=/|$)", home, raw)
    path = os.path.expanduser(raw)
    if not os.path.isabs(path):
        path = os.path.join(cwd, path)
    return os.path.normpath(path)


def reason(raw):
    if not raw or not isinstance(raw, str):
        return None
    path = located(raw)
    if path == os.path.join(home, ".claude", "CLAUDE.md") or path == os.path.join(codex, "AGENTS.md"):
        return POLICY
    for generated in ("skills", "prompts"):
        root = os.path.join(codex, generated)
        if path == root or path.startswith(root + "/"):
            return POLICY
    for candidate in {path, os.path.realpath(path)}:
        if candidate.endswith(".nix") and os.path.exists(candidate[:-4] + ".bnix"):
            return NIX
    return None


REDIRECT = re.compile(r"^\d*(?:>>|>\||>)(.*)$")
EVERY_OPERAND = {"sed", "sd", "tee", "truncate"}
LAST_OPERAND = {"cp", "mv", "install", "ln", "rsync"}


def shell_targets(command):
    tokens = words(command)
    if tokens is None:
        return
    for segment in segments(tokens):
        for index, token in enumerate(segment):
            match = REDIRECT.match(token)
            if match:
                yield match.group(1) or (segment[index + 1] if index + 1 < len(segment) else "")
        start = command_index(segment)
        if start >= len(segment):
            continue
        name = os.path.basename(segment[start])
        operands = [w for w in segment[start + 1:] if not w.startswith("-") and not REDIRECT.match(w)]
        if name == "sed" and not any(w.startswith("-i") or w.startswith("--in-place") for w in segment[start + 1:]):
            continue
        if name in EVERY_OPERAND:
            yield from operands
        elif name in LAST_OPERAND and operands:
            yield operands[-1]
        elif name == "dd":
            yield from (w[3:] for w in operands if w.startswith("of="))


tool = data.get("tool_name", "") or ""
tool_input = data.get("tool_input", {}) or {}
if tool == "Bash":
    command = tool_input.get("command", "") or ""
    targets = shell_targets(command) if isinstance(command, str) else ()
elif tool.endswith("apply_patch"):
    from launch_critical_decide import _find_envelope, _patch_targets
    envelope = _find_envelope(tool_input)
    targets = _patch_targets(envelope) if envelope else ()
elif tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
    targets = [tool_input.get(key) for key in ("file_path", "filePath", "notebook_path")]
else:
    targets = ()

found = next((r for r in map(reason, targets) if r), None)
if not found:
    sys.exit(0)

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": found,
    }
}))
sys.exit(0)
PYEOF

hook_decide python3 -c "$PY" "$(dirname "$0")/lib"
