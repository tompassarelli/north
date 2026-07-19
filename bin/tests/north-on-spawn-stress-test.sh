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
    printf '%s\n' "${4:-}" >>"$HOOK_TEST_STATE/presences.log"
    ;;
esac
if [ "${HOOK_TEST_MODE:-fast}" = slow ]; then
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

echo "== partial outer-buffer allocation leaves no input tempfile =="
MKTEMP_SHIM="$TMP/mktemp-shim"
mkdir -p "$MKTEMP_SHIM"
cat >"$MKTEMP_SHIM/mktemp" <<'EOF'
#!/usr/bin/env bash
count_file="$HOOK_TEST_STATE/mktemp-count"
count="$(cat "$count_file" 2>/dev/null || echo 0)"
count=$((count + 1))
printf '%s' "$count" >"$count_file"
if [ "$count" -eq 1 ]; then
  path="${1%.XXXXXX}.partial"
  : >"$path"
  printf '%s\n' "$path"
  exit 0
fi
exit 1
EOF
chmod +x "$MKTEMP_SHIM/mktemp"
XDG_PARTIAL="$TMP/xdg-partial"
OUT_PARTIAL="$TMP/partial.out"
mkdir -p "$XDG_PARTIAL"
rm -f -- "${STATE:?}/mktemp-count"
payload partial01 SessionStart "" |
  env -i HOME="$FAKE_HOME" PATH="$MKTEMP_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_PARTIAL" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast AGENT_PROVIDER=openai \
    bash "$HOOK" >"$OUT_PARTIAL" 2>"$OUT_PARTIAL.err"
rc=$?
check "partial-allocation hook exits zero" test "$rc" -eq 0
check "partial-allocation stdout is empty" test ! -s "$OUT_PARTIAL"
check "partial-allocation stderr is empty" test ! -s "$OUT_PARTIAL.err"
check "partial input tempfile is removed" test -z "$(find "$XDG_PARTIAL" -type f -name 'north-spawn-input.*' -print -quit)"

PY_SHIM="$TMP/python-shim"
mkdir -p "$PY_SHIM"
REAL_PYTHON="$(command -v python3)"
cat >"$PY_SHIM/python3" <<EOF
#!/usr/bin/env bash
if [ "\${HOOK_TEST_DELAY_FIRST_PYTHON:-0}" = 1 ]; then
  count_file="\$HOOK_TEST_STATE/python-count"
  count="\$(cat "\$count_file" 2>/dev/null || echo 0)"
  count=\$((count + 1))
  printf '%s' "\$count" >"\$count_file"
  [ "\$count" -ne 1 ] || sleep 0.7
fi
if [ "\${HOOK_TEST_HANG_SPAWN_VALIDATOR:-0}" = 1 ] &&
    [ "\${1:-}" = -c ] &&
    printf '%s' "\${2:-}" | grep -Fq 'specific = payload["hookSpecificOutput"]'; then
  sleep 30
  exit 1
fi
if [ "\${HOOK_TEST_BAD_SPAWN_SERIALIZER:-0}" = 1 ] &&
    [ "\${1:-}" = -c ] &&
    printf '%s' "\${2:-}" | grep -Fq 'os.environ["HOOK_EVENT_NAME"]'; then
  printf '{"hookSpecificOutput":{"hookEventName":"SubagentStart","additionalContext":"wrong event"}}\n'
  exit 0
fi
if [ "\${HOOK_TEST_OVERSIZE_SPAWN_SERIALIZER:-0}" = 1 ] &&
    [ "\${1:-}" = -c ] &&
    printf '%s' "\${2:-}" | grep -Fq 'os.environ["HOOK_EVENT_NAME"]'; then
  exec "$REAL_PYTHON" -c 'import json; print(json.dumps({"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"x" * 40000}}, separators=(",", ":")))'
fi
exec "$REAL_PYTHON" "\$@"
EOF
chmod +x "$PY_SHIM/python3"

echo "== one cold Python startup stays inside the single inner deadline =="
XDG_COLD_PYTHON="$TMP/xdg-cold-python"
OUT_COLD_PYTHON="$TMP/cold-python.out"
mkdir -p "$XDG_COLD_PYTHON"
rm -f -- "${STATE:?}/python-count"
t0="$(date +%s%3N)"
payload cold-python-01 SessionStart "" |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_COLD_PYTHON" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast HOOK_TEST_DELAY_FIRST_PYTHON=1 \
    AGENT_PROVIDER=openai bash "$HOOK" >"$OUT_COLD_PYTHON" 2>"$OUT_COLD_PYTHON.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "cold-Python startup exits zero" test "$rc" -eq 0
check "cold-Python startup survives a 700ms first interpreter start (${elapsed}ms)" valid_context_json "$OUT_COLD_PYTHON"
check "cold-Python startup stays below provider deadline (${elapsed}ms)" test "$elapsed" -lt 2500
check "cold-Python startup emits no stderr" test ! -s "$OUT_COLD_PYTHON.err"
await_locks "$XDG_COLD_PYTHON" || true

echo "== wrong inner envelope becomes a clean no-op =="
XDG_BADJSON="$TMP/xdg-badjson"
OUT_BADJSON="$TMP/badjson.out"
mkdir -p "$XDG_BADJSON"
payload wrong-envelope-01 SessionStart "" |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_BADJSON" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast HOOK_TEST_BAD_SPAWN_SERIALIZER=1 \
    AGENT_PROVIDER=openai bash "$HOOK" >"$OUT_BADJSON" 2>"$OUT_BADJSON.err"
rc=$?
check "wrong-envelope hook exits zero" test "$rc" -eq 0
check "wrong-envelope stdout is empty" test ! -s "$OUT_BADJSON"
check "wrong-envelope stderr is empty" test ! -s "$OUT_BADJSON.err"
await_locks "$XDG_BADJSON" || true

echo "== outer validation is deadline-bounded and output is size-bounded =="
XDG_VALIDATOR="$TMP/xdg-validator"
OUT_VALIDATOR="$TMP/validator.out"
mkdir -p "$XDG_VALIDATOR"
t0="$(date +%s%3N)"
payload validator01 SessionStart "" |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_VALIDATOR" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast HOOK_TEST_HANG_SPAWN_VALIDATOR=1 \
    AGENT_PROVIDER=openai bash "$HOOK" >"$OUT_VALIDATOR" 2>"$OUT_VALIDATOR.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "hanging-validator hook exits zero" test "$rc" -eq 0
check "hanging validator is cut off under 2s (${elapsed}ms)" test "$elapsed" -lt 2000
check "hanging validator emits no stdout" test ! -s "$OUT_VALIDATOR"
check "hanging validator emits no stderr" test ! -s "$OUT_VALIDATOR.err"
await_locks "$XDG_VALIDATOR" || true

XDG_OVERSIZE="$TMP/xdg-oversize"
OUT_OVERSIZE="$TMP/oversize.out"
mkdir -p "$XDG_OVERSIZE"
t0="$(date +%s%3N)"
payload oversize01 SessionStart "" |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_OVERSIZE" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast HOOK_TEST_OVERSIZE_SPAWN_SERIALIZER=1 \
    AGENT_PROVIDER=openai bash "$HOOK" >"$OUT_OVERSIZE" 2>"$OUT_OVERSIZE.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "oversized-inner hook exits zero" test "$rc" -eq 0
check "oversized inner envelope is rejected under 2s (${elapsed}ms)" test "$elapsed" -lt 2000
check "oversized inner envelope emits no JSON prefix" test ! -s "$OUT_OVERSIZE"
check "oversized inner envelope emits no stderr" test ! -s "$OUT_OVERSIZE.err"
await_locks "$XDG_OVERSIZE" || true

echo "== delayed coordinator cannot hold startup pipes or strand workers =="
XDG_SLOW="$TMP/xdg-slow"
OUT_SLOW="$TMP/slow.out"
: >"$STATE/starts.log"
t0="$(date +%s%3N)"
run_hook "$XDG_SLOW" slow slow0001 SessionStart "" "" "$OUT_SLOW"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "delayed startup exits zero" test "$rc" -eq 0
check "delayed startup returns under 1s with workers live (${elapsed}ms)" test "$elapsed" -lt 1000
check "delayed startup still emits valid context JSON" valid_context_json "$OUT_SLOW"
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

echo "== completed context survives an inner post-emit stall =="
STALL_SHIM="$TMP/stall-shim"
mkdir -p "$STALL_SHIM"
REAL_MKDIR="$(command -v mkdir)"
cat >"$STALL_SHIM/mkdir" <<EOF
#!/usr/bin/env bash
case "\${*: -1}" in
  *.spawn.lock) sleep 30; exit 1 ;;
esac
exec "$REAL_MKDIR" "\$@"
EOF
chmod +x "$STALL_SHIM/mkdir"
XDG_STALL="$TMP/xdg-stall"
OUT_STALL="$TMP/stall.out"
mkdir -p "$XDG_STALL"
t0="$(date +%s%3N)"
payload stall001 SessionStart "" |
  env -i HOME="$FAKE_HOME" PATH="$STALL_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_STALL" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast AGENT_PROVIDER=openai \
    bash "$HOOK" >"$OUT_STALL" 2>"$OUT_STALL.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "outer circuit breaker exits zero after inner stall" test "$rc" -eq 0
check "outer circuit breaker fires before provider deadline (${elapsed}ms)" test "$elapsed" -lt 5000
check "complete pre-stall context is still emitted as valid JSON" valid_context_json "$OUT_STALL"
check "post-stall path emits no stderr" test ! -s "$OUT_STALL.err"

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
