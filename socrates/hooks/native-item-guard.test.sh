#!/usr/bin/env bash
# Deny/pass matrix for the native item guard: gh writes naming a native ref or
# quoting a native item's text (in arguments, heredocs or body files) are
# denied; reads, other commands and unrelated text are allowed.
set -uo pipefail
# Hooks run with the managed hook runtime first on PATH; so do their tests.
export PATH="/etc/codex/hooks/runtime:$PATH"

HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$HERE/native-item-guard.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/native-item-guard-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT
mkdir -p "$SCRATCH/home/.local/state/north" "$SCRATCH/home/.local/state/threads"
DB="$SCRATCH/home/.local/state/threads/threads.db"
sqlite3 "$DB" "CREATE TABLE items(ns TEXT, number INT, title TEXT, body TEXT);
  CREATE TABLE events(kind TEXT, data TEXT);
  INSERT INTO items VALUES ('life', 1, 'Fake test item about the orange bicycle', 'Fake body: repaint the garden shed blue');
  INSERT INTO events VALUES ('note', json_object('text', 'Fake note: the parcel went to the neighbour'));"
printf 'Context: repaint the garden shed blue soon\n' >"$SCRATCH/body.md"
printf 'Context: a plain unrelated body\n' >"$SCRATCH/plain.md"
ACTIVATION="$SCRATCH/activation.active"

pass=0 fail=0
set_active() {
  if [ "$1" = true ]; then printf 'hook native-item-guard\n' >"$ACTIVATION"; else : >"$ACTIVATION"; fi
}
set_active true

# run EXPECT DESCRIPTION COMMAND [ENV...]
run() {
  local expect="$1" desc="$2" cmd="$3"; shift 3
  local input out decision ok=0
  input="$(python3 -c 'import json,sys; print(json.dumps({"tool_name":"Bash","cwd":sys.argv[2],"tool_input":{"command":sys.argv[1]}}))' "$cmd" "$SCRATCH")"
  out="$(printf '%s' "$input" | env -u AGENT_NO_AUTHORING_HOOKS \
    HOME="$SCRATCH/home" NORTH_AGENT_ACTIVE="$ACTIVATION" \
    NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 "$@" "$HOOK" 2>&1)"
  decision="$(python3 -c 'import json,sys
try:
    d = json.loads(sys.argv[1] or "null")
except Exception:
    print("malformed"); raise SystemExit
print((d or {}).get("hookSpecificOutput", {}).get("permissionDecision", "silent"))' "${out:-}")"
  case "$expect" in
    deny)  [ "$decision" = deny ] && ok=1 ;;
    allow) [ "$decision" != deny ] && [ "$decision" != malformed ] && ok=1 ;;
  esac
  if [ "$ok" = 1 ]; then
    pass=$((pass + 1)); printf 'PASS  %-5s  %s\n' "$expect" "$desc"
  else
    fail=$((fail + 1))
    printf 'FAIL  %-5s  %s\n      cmd=%q\n      decision=%s out=%s\n' "$expect" "$desc" "$cmd" "$decision" "$out"
  fi
}

echo '== gh writes naming a native ref or quoting item text are denied =='
run deny 'comment names a ref' 'gh issue comment 5 --body "done, see life#1"'
run deny 'create title names a ref' 'gh issue create -R tompassarelli/firn --title "ops#12 follow-up" --body x'
run deny 'pr edit names a ref' 'gh pr edit 3 --body "travel#2"'
run deny 'api body names a ref' 'gh api repos/x/y/issues/1/comments -f body="accounts#7"'
run deny 'heredoc body names a ref' "$(printf 'gh issue comment 5 --body-file - <<EOF\nrelates to life#1\nEOF')"
run deny 'comment quotes an item title' 'gh issue comment 5 --body "the orange bicycle needs work"'
run deny 'comment quotes an item note' 'gh issue comment 5 --body "the parcel went somewhere"'
run deny 'body file quotes an item body' 'gh issue comment 5 --body-file body.md'
run deny 'api field file quotes an item body' 'gh api repos/x/y/issues -F body=@body.md'
run deny 'gh behind env' 'GH_REPO=x/y gh issue comment 5 --body "life#1"'

echo '== reads, other commands and unrelated text are allowed =='
run allow 'issue view' 'gh issue view 1'
run allow 'issue list' 'gh issue list -R tompassarelli/firn'
run allow 'unrelated comment' 'gh issue comment 5 --body "landed in abc123"'
run allow 'unrelated body file' 'gh issue comment 5 --body-file plain.md'
run allow 'short overlap only' 'gh issue comment 5 --body "the orange car"'
run allow 'threads note' 'threads note life#1 "progress"'
run allow 'non-native ref' 'gh issue comment 5 --body "see firn#1"'
run allow 'word-joined ref' 'gh issue comment 5 --body "xlife#1"'

echo '== malformed and unavailable state =='
out_raw="$(printf 'not json gh' | env HOME="$SCRATCH/home" NORTH_AGENT_ACTIVE="$ACTIVATION" "$HOOK" 2>&1)"
if [ -z "$out_raw" ]; then pass=$((pass + 1)); echo 'PASS  allow  malformed payload'; else fail=$((fail + 1)); echo "FAIL  allow  malformed payload: $out_raw"; fi
mv "$DB" "$DB.off"
run allow 'quote without a database' 'gh issue comment 5 --body "the orange bicycle needs work"'
run deny 'ref without a database' 'gh issue comment 5 --body "life#1"'
mv "$DB.off" "$DB"

echo '== kill-switch =='
run allow 'guards off via env' 'gh issue comment 5 --body "life#1"' AGENT_NO_AUTHORING_HOOKS=1
set_active false
run allow 'UnitId off via activation' 'gh issue comment 5 --body "life#1"'
set_active true
run deny 'UnitId back on via activation' 'gh issue comment 5 --body "life#1"'

echo
printf '== result: %s passed, %s failed ==\n' "$pass" "$fail"
[ "$fail" = 0 ]
