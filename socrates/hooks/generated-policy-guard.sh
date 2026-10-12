#!/usr/bin/env bash
# PreToolUse guard — refuses hand edits of generated files: ~/.claude/CLAUDE.md,
# the ~/.codex files `agents sync` projects (AGENTS.md, skills/, prompts/), and
# any X.nix beside an X.bnix. Covers Edit/Write/MultiEdit/NotebookEdit,
# apply_patch, and shell redirection, sed/perl/ruby/awk in-place, sd, tee, ed, cp,
# mv, install, ln and rsync onto those paths, resolving relative paths after cd.
# Reads, quoted mentions and `agents sync` pass.
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


def located(raw, base=None):
    raw = re.sub(r"^(?:\$HOME|\$\{HOME\})(?=/|$)", home, raw)
    path = os.path.expanduser(raw)
    if not os.path.isabs(path):
        path = os.path.join(base or cwd, path)
    return os.path.normpath(path)


PROTECTED_DIRS = (os.path.join(home, ".claude"), codex)
PROTECTED_NAMES = {"CLAUDE.md", "AGENTS.md", "SKILL.md"}


def reason(target):
    if isinstance(target, tuple):
        raw, base, fuzzy = target
    else:
        raw, base, fuzzy = target, None, False
    if not raw or not isinstance(raw, str):
        return None
    expanded = os.path.expanduser(re.sub(r"^(?:\$HOME|\$\{HOME\})(?=/|$)", home, raw))
    if fuzzy and not os.path.isabs(expanded):
        inside = any(base == d or base.startswith(d + "/") for d in PROTECTED_DIRS)
        generated = any(base.startswith(os.path.join(codex, d)) for d in ("skills", "prompts"))
        return POLICY if inside and (os.path.basename(raw) in PROTECTED_NAMES or generated) else None
    path = located(raw, base)
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
EVERY_OPERAND = {"sed", "sd", "tee", "truncate", "perl", "ruby", "ed", "ex"}
LAST_OPERAND = {"cp", "mv", "install", "ln", "rsync"}
IN_PLACE = {
    "sed": re.compile(r"^(?:--in-place|-[a-zA-Z]*i)"),
    "perl": re.compile(r"^-[nplaswtTUWXcS]*i"),
    "ruby": re.compile(r"^-[nplasw]*i"),
}
UNRESOLVED = re.compile(r"[$`*?\[{]")


def changed_dir(operands, base, fuzzy):
    if not operands:
        return home, False
    raw = operands[0]
    if raw == "-":
        return base, True
    expanded = re.sub(r"^(?:\$HOME|\$\{HOME\})(?=/|$)", home, raw)
    if UNRESOLVED.search(expanded):
        literal = UNRESOLVED.split(expanded, 1)[0]
        return located(os.path.dirname(literal) if literal and not literal.endswith("/") else literal or ".", base), True
    return located(expanded, base), fuzzy


def shell_targets(command):
    tokens = words(command)
    if tokens is None:
        return
    base, fuzzy = cwd, False
    for segment in segments(tokens):
        for index, token in enumerate(segment):
            match = REDIRECT.match(token)
            if match:
                yield (match.group(1) or (segment[index + 1] if index + 1 < len(segment) else ""), base, fuzzy)
        start = command_index(segment)
        if start >= len(segment):
            continue
        name = os.path.basename(segment[start])
        args = segment[start + 1:]
        if name in ("cd", "pushd"):
            base, fuzzy = changed_dir([w for w in args if not w.startswith("-") or w == "-"], base, fuzzy)
            continue
        flags = [w for w in args if w.startswith("-")]
        if name in IN_PLACE and not any(IN_PLACE[name].match(w) for w in flags):
            continue
        if name in ("awk", "gawk") and not ("inplace" in args or "-iinplace" in args):
            continue
        directory = None
        operands = []
        skip = False
        for position, word in enumerate(args):
            if skip:
                skip = False
            elif word in ("-t", "--target-directory") and name in LAST_OPERAND and position + 1 < len(args):
                directory, skip = args[position + 1], True
            elif word.startswith("--target-directory="):
                directory = word.split("=", 1)[1]
            elif not word.startswith("-") and not REDIRECT.match(word):
                operands.append(word)
        if name in EVERY_OPERAND or name in ("awk", "gawk"):
            yield from ((w, base, fuzzy) for w in operands)
        elif name in LAST_OPERAND and (operands or directory):
            if directory is None:
                directory, operands = operands[-1], operands[:-1]
            yield (directory, base, fuzzy)
            yield from ((os.path.join(directory, os.path.basename(w.rstrip("/"))), base, fuzzy) for w in operands)
        elif name == "dd":
            yield from ((w[3:], base, fuzzy) for w in operands if w.startswith("of="))


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
