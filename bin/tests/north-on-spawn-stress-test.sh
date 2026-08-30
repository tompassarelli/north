#!/usr/bin/env bash
# Hermetic latency, fail-open, and identity-race tests for SessionStart.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
HOOK="$ROOT/bin/north-on-spawn"
ACTOR_KEY="$ROOT/bin/north-actor-key"
TMP="$(mktemp -d)"
trap 'jobs -pr | xargs -r kill 2>/dev/null || true; rm -rf -- "${TMP:?}"' EXIT

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); printf '  ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n' "$1"; }
check() {
  local label="$1"
  shift
  if "$@"; then ok "$label"; else bad "$label"; fi
}

FAKE_HOME="$TMP/home"
SHIM="$TMP/shim"
STATE="$TMP/state"
PROJECT="$TMP/project"
mkdir -p "$FAKE_HOME/code/north/cli" "$SHIM" "$STATE" "$PROJECT"
: >"$STATE/starts.log"
: >"$STATE/projections.log"
: >"$STATE/presences.log"

cat >"$SHIM/bb" <<'EOF'
#!/usr/bin/env bash
set -u
kind=other
case "${1:-}" in
  -e) kind=projection ;;
  */provider-native-session-projection.clj) kind=projection ;;
  *presence-cli.clj) kind=presence ;;
esac
marker="$HOOK_TEST_STATE/live-$kind-$$"
: >"$marker"
printf '%s %s\n' "$kind" "$$" >>"$HOOK_TEST_STATE/starts.log"
trap 'rm -f -- "${marker:?}"' EXIT
case "$kind" in
  projection)
    printf '%s\n' "${NORTH_NATIVE_SUBJECT#@agent:}" >>"$HOOK_TEST_STATE/projections.log"
    [ "${HOOK_TEST_MODE:-fast}" = reject ] && exit 1
    ;;
  presence)
    presence_id="${4:-}"
    printf '%s\n' "$presence_id" >>"$HOOK_TEST_STATE/presences.log"
    : >"$HOOK_TEST_STATE/presence-complete-$presence_id"
    ;;
esac
if [ "${HOOK_TEST_MODE:-fast}" = slow-after-presence ] &&
   [ "$kind" = projection ]; then
  projection_id="${NORTH_NATIVE_SUBJECT#@agent:}"
  for _ in $(seq 1 800); do
    [ ! -e "$HOOK_TEST_STATE/presence-complete-$projection_id" ] || break
    sleep 0.01
  done
  [ -e "$HOOK_TEST_STATE/presence-complete-$projection_id" ] || exit 2
  printf '%s\n' "$projection_id" >>"$HOOK_TEST_STATE/delayed-after-presence.log"
  sleep 30
fi
exit 0
EOF
chmod +x "$SHIM/bb"

payload() {
  local sid="$1" event="${2:-SessionStart}" agent_id="${3:-}"
  if [ -n "$agent_id" ]; then
    printf '{"session_id":"%s","agent_id":"%s","cwd":"%s","hook_event_name":"%s","model":"gpt-test","effort":{"level":"xhigh"}}' \
      "$sid" "$agent_id" "$PROJECT" "$event"
  else
    printf '{"session_id":"%s","cwd":"%s","hook_event_name":"%s","model":"gpt-test","effort":{"level":"xhigh"}}' \
      "$sid" "$PROJECT" "$event"
  fi
}

run_hook() {
  local xdg="$1" mode="$2" sid="$3" event="$4" agent_id="$5" pin="$6" out="$7"
  mkdir -p "$xdg"
  if [ -n "$pin" ]; then
    payload "$sid" "$event" "$agent_id" | env -i \
      HOME="$FAKE_HOME" PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$xdg" \
      HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE="$mode" AGENT_PROVIDER=openai \
      NORTH_AGENT_ID="$pin" bash "$HOOK" >"$out" 2>"$out.err"
  else
    payload "$sid" "$event" "$agent_id" | env -i \
      HOME="$FAKE_HOME" PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$xdg" \
      HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE="$mode" AGENT_PROVIDER=openai \
      bash "$HOOK" >"$out" 2>"$out.err"
  fi
}

await_locks() {
  local xdg="$1"
  for _ in $(seq 1 800); do
    if ! find "$xdg" -type d -name '*.lock' -print -quit 2>/dev/null | grep -q .; then
      return 0
    fi
    sleep 0.01
  done
  return 1
}

valid_context_json() {
  python3 - "$1" <<'PY'
import json
import re
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
specific = payload["hookSpecificOutput"]
assert specific["hookEventName"] in {"SessionStart", "SubagentStart"}
assert re.search(r'you are agent "[^"]+"', specific["additionalContext"])
PY
}

context_id() {
  python3 - "$1" <<'PY'
import json
import re
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    context = json.load(handle)["hookSpecificOutput"]["additionalContext"]
print(re.search(r'you are agent "([^"]+)"', context).group(1))
PY
}
session_key() { "$ACTOR_KEY" session "$1"; }

echo "== fresh normal startup emits context without waiting for maintenance =="
XDG_FAST="$TMP/xdg-fast"
OUT_FAST="$TMP/fast.out"
t0="$(date +%s%3N)"
run_hook "$XDG_FAST" fast cold0001 SessionStart "" "" "$OUT_FAST"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "fresh startup exits zero" test "$rc" -eq 0
check "fresh startup completes under 1s (${elapsed}ms)" test "$elapsed" -lt 1000
check "fresh startup emits valid context JSON" valid_context_json "$OUT_FAST"
check "fresh startup emits no stderr" test ! -s "$OUT_FAST.err"
check "fresh maintenance completes" await_locks "$XDG_FAST"
fast_id="$(context_id "$OUT_FAST")"
check "eventual projection uses the context identity" grep -Fxq "$fast_id" "$STATE/projections.log"
check "eventual presence uses the context identity" grep -Fxq "$fast_id" "$STATE/presences.log"
COLD_KEY="$(session_key cold0001)"
check "route cache commits after successful projection" test -s "$XDG_FAST/north-agent-routes/$COLD_KEY"

echo "== 20MB hook envelope stays bounded =="
XDG_LARGE="$TMP/xdg-large"
OUT_LARGE="$TMP/large.out"
mkdir -p "$XDG_LARGE"
t0="$(date +%s%3N)"
python3 -c 'import sys
sys.stdout.write("{\"session_id\":\"large001\",\"cwd\":\"'"$PROJECT"'\",\"hook_event_name\":\"SessionStart\",\"model\":\"gpt-test\",\"tool_response\":\"" + "x" * 20000000 + "\"}")' |
  env -i HOME="$FAKE_HOME" PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_LARGE" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast AGENT_PROVIDER=openai \
    bash "$HOOK" >"$OUT_LARGE" 2>"$OUT_LARGE.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "20MB startup exits zero" test "$rc" -eq 0
check "20MB startup completes under 1.5s (${elapsed}ms)" test "$elapsed" -lt 1500
check "20MB startup emits valid context JSON" valid_context_json "$OUT_LARGE"
await_locks "$XDG_LARGE" || true

echo "== typed envelope parser fails open and owns its stdin deadline =="
XDG_BADJSON="$TMP/xdg-badjson"
mkdir -p "$XDG_BADJSON"
printf '{broken' | env -i HOME="$FAKE_HOME" PATH="$SHIM:$PATH" \
  XDG_RUNTIME_DIR="$XDG_BADJSON" bash "$HOOK" \
  >"$TMP/badjson.out" 2>"$TMP/badjson.err"
check "malformed envelope emits no context" test ! -s "$TMP/badjson.out"
check "malformed envelope emits no stderr" test ! -s "$TMP/badjson.err"

fifo="$TMP/held-open.fifo"
mkfifo "$fifo"
t0="$(date +%s%3N)"
env -i HOME="$FAKE_HOME" PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_BADJSON" \
  bash "$HOOK" <"$fifo" >"$TMP/held-open.out" 2>"$TMP/held-open.err" &
hook_pid=$!
exec 9>"$fifo"
printf '%s' '{"session_id":"still-open"' >&9
wait "$hook_pid"
elapsed=$(( $(date +%s%3N) - t0 ))
exec 9>&-
check "held-open stdin is cut off under 2s (${elapsed}ms)" test "$elapsed" -lt 2000
check "held-open stdin emits no partial context" test ! -s "$TMP/held-open.out"
check "held-open stdin emits no stderr" test ! -s "$TMP/held-open.err"

echo "== delayed coordinator cannot hold startup pipes or strand workers =="
XDG_SLOW="$TMP/xdg-slow"
OUT_SLOW="$TMP/slow.out"
: >"$STATE/starts.log"
t0="$(date +%s%3N)"
run_hook "$XDG_SLOW" slow-after-presence slow0001 SessionStart "" "" "$OUT_SLOW"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "delayed startup exits zero" test "$rc" -eq 0
check "delayed startup still emits valid context JSON" valid_context_json "$OUT_SLOW"
SLOW_ID="$(context_id "$OUT_SLOW")"
for _ in $(seq 1 800); do
  [ ! -e "$STATE/delayed-after-presence.log" ] || break
  sleep 0.01
done
# The inner shell expands its positional parameters.
# shellcheck disable=SC2016
check "delayed projection starts after presence without holding startup (${elapsed}ms)" \
  bash -c 'test "$1" -lt 1000 && grep -Fxq "$2" "$3"' \
  _ "$elapsed" "$SLOW_ID" "$STATE/delayed-after-presence.log"
SLOW_KEY="$(session_key slow0001)"
check "identity seed remains available for PostToolUse repair" test -s "$XDG_SLOW/north-agent-routes/$SLOW_KEY.seed"
sleep 6.5
check "delayed maintenance processes are deadline-killed" test -z "$(find "$STATE" -name 'live-*' -print -quit)"
check "delayed maintenance removes its singleflight lock" test -z "$(find "$XDG_SLOW" -type d -name '*.lock' -print -quit)"
check "failed projection does not commit the route cache" test ! -e "$XDG_SLOW/north-agent-routes/$SLOW_KEY"

echo "== rejected projection preserves seed but never claims convergence =="
XDG_REJECT="$TMP/xdg-reject"
OUT_REJECT="$TMP/reject.out"
run_hook "$XDG_REJECT" reject reject01 SessionStart "" "" "$OUT_REJECT"
check "rejected startup still emits valid context JSON" valid_context_json "$OUT_REJECT"
check "rejected maintenance completes" await_locks "$XDG_REJECT"
REJECT_KEY="$(session_key reject01)"
check "rejected projection preserves exact observation seed" test -s "$XDG_REJECT/north-agent-routes/$REJECT_KEY.seed"
check "rejected projection leaves route cache absent" test ! -e "$XDG_REJECT/north-agent-routes/$REJECT_KEY"

echo "== concurrent inherited-pin burst has one owner and distinct actors =="
XDG_BURST="$TMP/xdg-burst"
mkdir -p "$XDG_BURST"
: >"$STATE/projections.log"
: >"$STATE/presences.log"
agents=(
  agent-a1111111-0000-4000-8000-000000000001
  agent-b2222222-0000-4000-8000-000000000002
  agent-c3333333-0000-4000-8000-000000000003
  agent-d4444444-0000-4000-8000-000000000004
  agent-e5555555-0000-4000-8000-000000000005
  agent-f6666666-0000-4000-8000-000000000006
  agent-a7777777-0000-4000-8000-000000000007
  agent-b8888888-0000-4000-8000-000000000008
)
pids=()
outs=()
t0="$(date +%s%3N)"
for i in "${!agents[@]}"; do
  out="$TMP/burst-$i.out"
  outs+=("$out")
  run_hook "$XDG_BURST" fast parent-session SubagentStart "${agents[$i]}" shared-parent-pin "$out" &
  pids+=("$!")
done
burst_ok=1
for pid in "${pids[@]}"; do wait "$pid" || burst_ok=0; done
elapsed=$(( $(date +%s%3N) - t0 ))
check "all concurrent starts exit zero" test "$burst_ok" -eq 1
check "concurrent starts return under 2s (${elapsed}ms)" test "$elapsed" -lt 2000
check "concurrent maintenance completes" await_locks "$XDG_BURST"
if python3 - "$XDG_BURST" "$STATE/projections.log" "$STATE/presences.log" "${agents[@]}" -- "${outs[@]}" <<'PY'
import hashlib
import json
import pathlib
import re
import sys

xdg = pathlib.Path(sys.argv[1])
projections = set(pathlib.Path(sys.argv[2]).read_text().splitlines())
presences = set(pathlib.Path(sys.argv[3]).read_text().splitlines())
separator = sys.argv.index("--")
agents = sys.argv[4:separator]
outputs = [pathlib.Path(path) for path in sys.argv[separator + 1:]]

def actor_key(namespace, raw):
    preimage = b"north-actor-key-v1\0" + namespace.encode("ascii") + b"\0" + raw.encode("utf-8")
    return hashlib.sha256(preimage).hexdigest()

ids = []
for output in outputs:
    context = json.loads(output.read_text())["hookSpecificOutput"]["additionalContext"]
    ids.append(re.search(r'you are agent "([^"]+)"', context).group(1))
assert len(ids) == 8
assert len(set(ids)) == 8
assert ids.count("shared-parent-pin") == 1
assert set(ids) <= projections
assert set(ids) <= presences
claims = list((xdg / "north-agent-ids" / ".pin-owners").iterdir())
assert len(claims) == 1
assert claims[0].name == actor_key("managed", "shared-parent-pin")
owner = claims[0].read_text()
expected_keys = {actor_key("agent", raw) for raw in agents}
assert owner in expected_keys
cache_values = {
    path.name: path.read_text()
    for path in (xdg / "north-agent-ids").iterdir()
    if path.is_file()
}
assert set(cache_values) == expected_keys
assert set(cache_values.values()) == set(ids)
PY
then
  ok "atomic pin claim yields exactly one owner, eight distinct context/projection identities"
else
  bad "atomic pin claim yields exactly one owner, eight distinct context/projection identities"
fi

echo
echo "north-on-spawn-stress-test: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
