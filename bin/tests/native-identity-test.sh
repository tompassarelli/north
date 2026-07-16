#!/usr/bin/env bash
# Exact native-Claude identity observations: SessionStart owns model, while
# tool-context hooks own effective effort. Fully hermetic: fake HOME contains the
# north writer and PATH contains a no-op bb, so no production facts or network.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$(cd "$HERE/.." && pwd)"
SPAWN="$BIN/north-on-spawn"
TOOLUSE="$BIN/north-on-tooluse"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FAKE_HOME="$TMP/home"
XDG="$TMP/xdg"
SHIM="$TMP/shim"
LOG="$TMP/north.log"
REPO_DIR="$TMP/project"
mkdir -p "$FAKE_HOME/code/north/bin" "$SHIM" "$XDG" "$REPO_DIR"
: > "$LOG"

cat > "$FAKE_HOME/code/north/bin/north" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$NORTH_IDENTITY_LOG"
EOF
cat > "$SHIM/bb" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$FAKE_HOME/code/north/bin/north" "$SHIM/bb"

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); printf '  ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n' "$1"; }
has() { if grep -Fq "$2" "$LOG"; then ok "$1"; else bad "$1"; fi; }
lacks() { if grep -Fq "$2" "$LOG"; then bad "$1"; else ok "$1"; fi; }

run_hook() {
  local hook="$1" payload="$2"
  shift 2
  printf '%s' "$payload" | env -i HOME="$FAKE_HOME" PATH="$SHIM:$PATH" \
    XDG_RUNTIME_DIR="$XDG" NORTH_PORT=1 NORTH_IDENTITY_LOG="$LOG" "$@" \
    bash "$hook" >/dev/null 2>&1
}

SID="11112222-3333-4444-8555-666677778888"
ID="session-project-11112222"

echo "== SessionStart exact input outranks ambient adapter dials =="
run_hook "$SPAWN" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"SessionStart\",\"model\":\"claude-opus-4-8\",\"effort\":{\"level\":\"xhigh\"}}" \
  CLAUDECODE=1 AGENT_MODEL=wrong-model CLAUDE_EFFORT=high AGENT_EFFORT=low
has "records exact SessionStart model" "tell agent:$ID model claude-opus-4-8"
has "records exact structured effort" "tell agent:$ID effort xhigh"
lacks "does not record ambient model over exact input" "tell agent:$ID model wrong-model"

echo "== SessionStart uses CLAUDE_EFFORT when structured effort is absent =="
: > "$LOG"
run_hook "$SPAWN" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"SessionStart\",\"model\":\"claude-sonnet-5\"}" \
  CLAUDECODE=1 CLAUDE_EFFORT=high AGENT_EFFORT=low
has "records CLAUDE_EFFORT" "tell agent:$ID effort high"
lacks "does not substitute generic effort" "tell agent:$ID effort low"

echo "== missing observations stay missing =="
: > "$LOG"
run_hook "$SPAWN" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"SessionStart\"}" \
  CLAUDECODE=1
lacks "does not guess a model fact" "tell agent:$ID model "
lacks "does not guess an effort fact" "tell agent:$ID effort "

echo "== PostToolUse refreshes only exact effective effort =="
: > "$LOG"
run_hook "$TOOLUSE" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"PostToolUse\",\"effort\":{\"level\":\"medium\"},\"tool_input\":{\"level\":\"wrong\",\"model\":\"also-wrong\"}}" \
  CLAUDE_EFFORT=low ANTHROPIC_MODEL=ambient-wrong
has "records structured effective effort" "tell agent:$ID effort medium"
lacks "does not use unrelated nested level" "tell agent:$ID effort wrong"
lacks "does not claim a PostToolUse model" "tell agent:$ID model "

: > "$LOG"
run_hook "$TOOLUSE" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"PostToolUse\"}" \
  CLAUDE_EFFORT=low
has "falls back to exact CLAUDE_EFFORT" "tell agent:$ID effort low"

echo "== exact effort changes refresh stored projections once =="
: > "$LOG"
run_hook "$SPAWN" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"SessionStart\",\"model\":\"claude-opus-4-8\",\"effort\":{\"level\":\"high\"}}" \
  CLAUDECODE=1
: > "$LOG"
run_hook "$TOOLUSE" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"PostToolUse\",\"effort\":{\"level\":\"xhigh\"}}" \
  CLAUDE_EFFORT=xhigh
has "updates stored display_handle with exact effort" \
  "tell agent:$ID display_handle anthropic-claude-opus-4-8-xhigh-native-11112222"
has "updates stored display_name with exact effort" \
  "tell agent:$ID display_name anthropic-claude-opus-4-8-xhigh-native-11112222"
lacks "does not retain the stale high projection" \
  "tell agent:$ID display_handle anthropic-claude-opus-4-8-high-native-11112222"

: > "$LOG"
run_hook "$TOOLUSE" \
  "{\"session_id\":\"$SID\",\"cwd\":\"$REPO_DIR\",\"hook_event_name\":\"PostToolUse\",\"effort\":{\"level\":\"xhigh\"}}" \
  CLAUDE_EFFORT=xhigh
lacks "unchanged effort does not rewrite identity" "tell agent:$ID "

echo
echo "native-identity-test: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
