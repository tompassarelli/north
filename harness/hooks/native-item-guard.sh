#!/usr/bin/env bash
# PreToolUse guard: refuses a Bash `gh` command that writes to GitHub (issue/pr
# create, comment, edit, review; api; gist; release) when its arguments, heredoc
# bodies or named body files name a native NorthThread item (life#N, accounts#N,
# travel#N, ops#N) or quote six consecutive words or a sensitive token from a
# native item's title, body or note (read-only from threads.db and its snapshot).
#
# Kill-switch: persistent `north config agents off native-item-guard` OR env
# AGENT_NO_AUTHORING_HOOKS (any value but 0/false; 0/false forces guards live).
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
  *gh*) ;;
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
import json, os, re, sqlite3, sys
sys.path.insert(0, sys.argv[1])
from shellcmd import segments, strip_heredocs, strip_quotes, words

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(65)

if data.get("tool_name", "") != "Bash":
    sys.exit(0)
cmd = (data.get("tool_input", {}) or {}).get("command", "") or ""
cwd = data.get("cwd") or os.getcwd()

NATIVE_REF = re.compile(r"\b(?:life|accounts|travel|ops)#\d+\b")
GH = re.compile(r"(?:^|[\n;&|({`])\s*(?:[A-Za-z_]\w*=\S*\s+|sudo\s+|env\s+|command\s+)*(?:\S*/)?gh"
                r"((?:\s+(?:-R|--repo)\s+\S+)*)\s+(issue|pr|api|gist|release)\b([^\n;&|)}`]*)")
WRITES = {"create", "comment", "edit", "review", "close", "reopen", "merge", "upload"}
SHINGLE = 6

def gh_writes(cleaned):
    for m in GH.finditer(cleaned):
        group, rest = m.group(2), m.group(3).split()
        if group in ("api", "gist") or (rest and rest[0] in WRITES):
            return True
    return False

def body_files(cmd):
    tokens = words(cmd) or []
    out = []
    for seg in segments(tokens):
        if not any(os.path.basename(t) == "gh" for t in seg):
            continue
        for i, t in enumerate(seg):
            nxt = seg[i + 1] if i + 1 < len(seg) else ""
            if t in ("--body-file", "-F", "--input", "--field", "--raw-field", "-f"):
                val = nxt
            elif t.startswith(("--body-file=", "--input=", "--field=")):
                val = t.split("=", 1)[1]
            else:
                continue
            if t in ("-F", "--field", "-f", "--raw-field") or t.startswith("--field="):
                if "=@" not in val:
                    continue
                val = val.split("=@", 1)[1]
            if val and val != "-":
                out.append(os.path.join(cwd, os.path.expanduser(val)))
    return out

def read_bounded(path):
    try:
        with open(path, "rb") as f:
            return f.read(1048576).decode("utf-8", "replace")
    except OSError:
        return ""

def word_shingles(text):
    tokens = re.findall(r"\w+", text.casefold())
    return {tuple(tokens[i:i + SHINGLE]) for i in range(len(tokens) - SHINGLE + 1)}

def sensitive_tokens(text):
    return {t for t in re.findall(r"[\w@]+(?:[.+/-][\w@]+)*", text.casefold())
            if len(t) >= 6 and ("@" in t or any(c.isdigit() for c in t)) and not re.fullmatch(r"\d{4}-\d\d-\d\d", t)}

def native_texts():
    home = os.path.expanduser("~/.local/state/threads")
    paths = [os.environ.get("THREADS_DB") or "", f"{home}/threads.db", f"{home}/snapshot.db"]
    texts = []
    for path in dict.fromkeys(p for p in paths if p and os.path.exists(p)):
        try:
            db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
            try:
                texts += [r for row in db.execute("SELECT title, body FROM items") for r in row if r]
                texts += [r[0] for r in db.execute("SELECT json_extract(data, '$.text') FROM events WHERE kind = 'note'"
                                                   " AND json_extract(data, '$.text') IS NOT NULL")]
            finally:
                db.close()
        except sqlite3.Error:
            continue
    return texts

if not gh_writes(strip_quotes(strip_heredocs(cmd))):
    sys.exit(0)

text = "\n".join([cmd] + [read_bounded(p) for p in body_files(cmd)])
hit = bool(NATIVE_REF.search(text))
if not hit:
    shingles = set()
    sensitive = set()
    for t in native_texts():
        shingles.update(word_shingles(t))
        sensitive.update(sensitive_tokens(t))
    hit = bool(shingles & word_shingles(text) or sensitive & sensitive_tokens(text))
if not hit:
    sys.exit(0)

print(json.dumps({"hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": (
        "No. This gh command names a native NorthThread item (life/accounts/travel/ops#N) or quotes its text, and "
        "native NorthThread items never reach GitHub. Record it with `threads note REF TEXT` instead, and rewrite the GitHub "
        "text without the ref or the quoted item text."),
}}))
PYEOF

hook_decide python3 -c "$PY" "$(dirname "$0")/lib"
