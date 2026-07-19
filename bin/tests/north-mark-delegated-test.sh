#!/usr/bin/env bash
# Hermetic fail-open tests for the delegated-work PostToolUse marker.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
HOOK="$ROOT/bin/north-mark-delegated"
ACTOR_KEY="$ROOT/bin/north-actor-key"
TMP="$(mktemp -d)"
trap 'jobs -pr | xargs -r kill 2>/dev/null || true; rm -rf "$TMP"' EXIT

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); printf '  ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n' "$1"; }
check() {
  local label="$1"
  shift
  if "$@"; then ok "$label"; else bad "$label"; fi
}

payload() {
  printf '{"session_id":"delegated-marker-01","cwd":"%s","hook_event_name":"PostToolUse"}' "$TMP/project"
}
SESSION="delegated-marker-01"
SESSION_KEY="$("$ACTOR_KEY" session "$SESSION")"
SESSION_ID="native-$SESSION_KEY"

mkdir -p "$TMP/project"
XDG_FAST="$TMP/xdg-fast"
mkdir -p "$XDG_FAST"
t0="$(date +%s%3N)"
payload | XDG_RUNTIME_DIR="$XDG_FAST" bash "$HOOK" >"$TMP/fast.out" 2>"$TMP/fast.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "normal marker hook exits zero" test "$rc" -eq 0
check "normal marker hook completes under 1s (${elapsed}ms)" test "$elapsed" -lt 1000
check "normal marker hook emits no stdout" test ! -s "$TMP/fast.out"
check "normal marker hook emits no stderr" test ! -s "$TMP/fast.err"
check "normal marker is written" \
  test -s "$XDG_FAST/north-delegated/$SESSION_KEY"
check "marker stores the resolved graph identity separately" \
  grep -Fxq "$SESSION_ID" "$XDG_FAST/north-delegated/$SESSION_KEY"

echo "== staked provider agent_id owns the delegated marker =="
AGENT_RAW="agent:delegated-marker-01"
AGENT_KEY="$("$ACTOR_KEY" agent "$AGENT_RAW")"
AGENT_ID="native-$AGENT_KEY"
mkdir -p "$XDG_FAST/north-agent-ids"
printf '%s\n' "$AGENT_ID" >"$XDG_FAST/north-agent-ids/$AGENT_KEY"
printf '{"session_id":"%s","agent_id":"%s","cwd":"%s","hook_event_name":"PostToolUse"}' \
  "$SESSION" "$AGENT_RAW" "$TMP/project" |
  XDG_RUNTIME_DIR="$XDG_FAST" bash "$HOOK" >/dev/null 2>&1
check "spawn-staked agent marker uses its typed digest" \
  grep -Fxq "$AGENT_ID" "$XDG_FAST/north-delegated/$AGENT_KEY"

UNSTAKED_RAW="agent:unseen-invocation"
UNSTAKED_KEY="$("$ACTOR_KEY" agent "$UNSTAKED_RAW")"
printf '{"session_id":"%s","agent_id":"%s","cwd":"%s","hook_event_name":"PostToolUse"}' \
  "$SESSION" "$UNSTAKED_RAW" "$TMP/project" |
  XDG_RUNTIME_DIR="$XDG_FAST" bash "$HOOK" >/dev/null 2>&1
check "unseen invocation agent_id cannot mint a delegated actor marker" \
  test ! -e "$XDG_FAST/north-delegated/$UNSTAKED_KEY"

MAX_AGENT="$(printf 'a%.0s' $(seq 1 512))"
MAX_KEY="$("$ACTOR_KEY" agent "$MAX_AGENT")"
MAX_ID="native-$MAX_KEY"
printf '%s\n' "$MAX_ID" >"$XDG_FAST/north-agent-ids/$MAX_KEY"
printf '{"session_id":"%s","agent_id":"%s","cwd":"%s","hook_event_name":"PostToolUse"}' \
  "$SESSION" "$MAX_AGENT" "$TMP/project" |
  XDG_RUNTIME_DIR="$XDG_FAST" bash "$HOOK" >/dev/null 2>&1
check "512-byte staked agent marker remains NAME_MAX-safe" \
  grep -Fxq "$MAX_ID" "$XDG_FAST/north-delegated/$MAX_KEY"

echo "== malformed input is a clean no-op =="
XDG_INVALID="$TMP/xdg-invalid"
mkdir -p "$XDG_INVALID"
printf '{broken' | XDG_RUNTIME_DIR="$XDG_INVALID" bash "$HOOK" >"$TMP/invalid.out" 2>"$TMP/invalid.err"
rc=$?
check "malformed input exits zero" test "$rc" -eq 0
check "malformed input emits no stdout" test ! -s "$TMP/invalid.out"
check "malformed input emits no stderr" test ! -s "$TMP/invalid.err"
check "malformed input writes no marker" test ! -d "$XDG_INVALID/north-delegated"

echo "== hostile PATH git is outside the marker authority path =="
SHIM="$TMP/shim"
mkdir -p "$SHIM"
cat >"$SHIM/git" <<'EOF'
#!/usr/bin/env bash
sleep 30
EOF
chmod +x "$SHIM/git"
XDG_HANG="$TMP/xdg-hang"
mkdir -p "$XDG_HANG"
t0="$(date +%s%3N)"
payload | PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_HANG" \
  bash "$HOOK" >"$TMP/hang.out" 2>"$TMP/hang.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "hanging-git hook exits zero" test "$rc" -eq 0
check "hostile-git hook completes under 1s (${elapsed}ms)" test "$elapsed" -lt 1000
check "hanging-git hook emits no stdout" test ! -s "$TMP/hang.out"
check "hanging-git hook emits no stderr" test ! -s "$TMP/hang.err"
check "hostile-git hook still writes its bounded marker" \
  test -s "$XDG_HANG/north-delegated/$SESSION_KEY"
check "hostile git is never executed" test ! -e "$TMP/hung-git.pid"

echo
echo "north-mark-delegated-test: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
