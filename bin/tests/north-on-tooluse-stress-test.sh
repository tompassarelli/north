#!/usr/bin/env bash
# Hermetic latency/fail-open stress tests for the PostToolUse observability hook.
# Exercises cold cache, a delayed/down coordinator, repeated and concurrent calls,
# large envelopes, stale singleflight recovery, private output buffering, and the
# print-before-ack delivery boundary.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
HOOK="$ROOT/bin/north-on-tooluse"
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

valid_control_character_json() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
context = payload["hookSpecificOutput"]["additionalContext"]
assert payload["hookSpecificOutput"]["hookEventName"] == "PostToolUse"
assert 'deadline "proof" \\ route' in context
assert "complete\tbody before stalled ack" in context
PY
}

FAKE_HOME="$TMP/home"
SHIM="$TMP/shim"
STATE="$TMP/state"
PROJECT="$TMP/project"
mkdir -p "$FAKE_HOME/code/north/cli" "$SHIM" "$STATE" "$PROJECT"

cat >"$SHIM/bb" <<'EOF'
#!/usr/bin/env bash
set -u
kind=other
case "${1:-}" in
  -e) kind=repair ;;
  *presence-cli.clj) kind=presence ;;
  *inbox-peek.clj) kind=inbox ;;
esac
marker="$HOOK_TEST_STATE/live-$kind-$$"
: >"$marker"
printf '%s %s\n' "$kind" "$$" >>"$HOOK_TEST_STATE/starts.log"
trap 'rm -f -- "${marker:?}"' EXIT
if [ "${HOOK_TEST_MODE:-fast}" = slow ] || [ "${HOOK_TEST_MODE:-fast}" = badjson ]; then
  if [ "$kind" = inbox ]; then
    printf '✉ from peer — deadline "proof" \\ route\n'
    printf '  complete\tbody before stalled ack\n'
  fi
fi
if [ "${HOOK_TEST_MODE:-fast}" = slow ]; then
  sleep 30
fi
exit 0
EOF
chmod +x "$SHIM/bb"
: >"$STATE/starts.log"

payload() {
  local sid="$1"
  printf '{"session_id":"%s","cwd":"%s","hook_event_name":"PostToolUse","effort":{"level":"xhigh"}}' \
    "$sid" "$PROJECT"
}

run_hook() {
  local xdg="$1" mode="$2" sid="$3" out="$4"
  mkdir -p "$xdg"
  payload "$sid" | env -i \
    HOME="$FAKE_HOME" PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$xdg" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE="$mode" AGENT_PROVIDER=openai \
    bash "$HOOK" >"$out" 2>/dev/null
}
session_key() { "$ACTOR_KEY" session "$1"; }

await_locks() {
  local xdg="$1"
  for _ in $(seq 1 200); do
    if ! find "$xdg" -type d -name '*.lock' -print -quit 2>/dev/null | grep -q .; then
      return 0
    fi
    sleep 0.01
  done
  return 1
}

await_workers() {
  local state="$1" xdg="$2"
  for _ in $(seq 1 800); do
    if [ -z "$(find "$state" -name 'live-*' -print -quit)" ] &&
        [ -z "$(find "$xdg" -type d -name '*.lock' -print -quit 2>/dev/null)" ]; then
      return 0
    fi
    sleep 0.01
  done
  return 1
}

echo "== cold healthy path stays low-latency and converges maintenance =="
XDG_FAST="$TMP/xdg-fast"
OUT_FAST="$TMP/fast.out"
t0="$(date +%s%3N)"
run_hook "$XDG_FAST" fast cold-fast-0001 "$OUT_FAST"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "cold hook exits zero" test "$rc" -eq 0
check "cold healthy hook completes under 1s (${elapsed}ms)" test "$elapsed" -lt 1000
check "background maintenance completes" await_locks "$XDG_FAST"
COLD_KEY="$(session_key cold-fast-0001)"
check "route convergence cache committed" test -s "$XDG_FAST/north-agent-routes/$COLD_KEY"
check "presence throttle marker committed" test -s "$XDG_FAST/north-presence-renew/$COLD_KEY"

echo "== 20MB envelope is parsed once, not once per field =="
XDG_LARGE="$TMP/xdg-large"
OUT_LARGE="$TMP/large.out"
t0="$(date +%s%3N)"
python3 -c 'import sys
sys.stdout.write("{\"session_id\":\"large-payload-01\",\"cwd\":\"'"$PROJECT"'\",\"hook_event_name\":\"PostToolUse\",\"tool_response\":\"" + "x" * 20000000 + "\"}")' |
  env -i HOME="$FAKE_HOME" PATH="$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_LARGE" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=fast AGENT_PROVIDER=openai \
    bash "$HOOK" >"$OUT_LARGE" 2>/dev/null
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "large-payload hook exits zero" test "$rc" -eq 0
check "20MB hook completes under 1.5s (${elapsed}ms)" test "$elapsed" -lt 1500
await_locks "$XDG_LARGE" || true

echo "== malformed inner output becomes a clean no-op =="
PY_SHIM="$TMP/python-shim"
mkdir -p "$PY_SHIM"
REAL_PYTHON="$(command -v python3)"
cat >"$PY_SHIM/python3" <<EOF
#!/usr/bin/env bash
if [ "\${HOOK_TEST_HANG_TOOLUSE_VALIDATOR:-0}" = 1 ] &&
    [ "\${1:-}" = -c ] &&
    printf '%s' "\${2:-}" | grep -Fq 'specific = payload["hookSpecificOutput"]'; then
  sleep 30
  exit 1
fi
if [ "\${HOOK_TEST_BAD_SERIALIZER:-0}" = 1 ] &&
    [ "\${1:-}" = -c ] &&
    printf '%s' "\${2:-}" | grep -Fq 'context = sys.stdin.read()'; then
  printf '{malformed'
  exit 0
fi
if [ "\${HOOK_TEST_OVERSIZE_TOOLUSE_SERIALIZER:-0}" = 1 ] &&
    [ "\${1:-}" = -c ] &&
    printf '%s' "\${2:-}" | grep -Fq 'context = sys.stdin.read()'; then
  exec "$REAL_PYTHON" -c 'import json; print(json.dumps({"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"x" * 40000}}, separators=(",", ":")))'
fi
exec "$REAL_PYTHON" "\$@"
EOF
chmod +x "$PY_SHIM/python3"
XDG_BADJSON="$TMP/xdg-badjson"
OUT_BADJSON="$TMP/badjson.out"
mkdir -p "$XDG_BADJSON"
payload malformed-child-01 |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_BADJSON" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=badjson HOOK_TEST_BAD_SERIALIZER=1 \
    AGENT_PROVIDER=openai bash "$HOOK" >"$OUT_BADJSON" 2>"$OUT_BADJSON.err"
rc=$?
check "malformed-child hook exits zero" test "$rc" -eq 0
check "malformed-child stdout is empty" test ! -s "$OUT_BADJSON"
check "malformed-child stderr is empty" test ! -s "$OUT_BADJSON.err"
await_locks "$XDG_BADJSON" || true

echo "== outer validation is deadline-bounded and output is size-bounded =="
XDG_VALIDATOR="$TMP/xdg-validator"
OUT_VALIDATOR="$TMP/validator.out"
mkdir -p "$XDG_VALIDATOR"
t0="$(date +%s%3N)"
payload validator01 |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_VALIDATOR" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=badjson HOOK_TEST_HANG_TOOLUSE_VALIDATOR=1 \
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
payload oversize01 |
  env -i HOME="$FAKE_HOME" PATH="$PY_SHIM:$SHIM:$PATH" XDG_RUNTIME_DIR="$XDG_OVERSIZE" \
    HOOK_TEST_STATE="$STATE" HOOK_TEST_MODE=badjson HOOK_TEST_OVERSIZE_TOOLUSE_SERIALIZER=1 \
    AGENT_PROVIDER=openai bash "$HOOK" >"$OUT_OVERSIZE" 2>"$OUT_OVERSIZE.err"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "oversized-inner hook exits zero" test "$rc" -eq 0
check "oversized inner envelope is rejected under 2s (${elapsed}ms)" test "$elapsed" -lt 2000
check "oversized inner envelope emits no JSON prefix" test ! -s "$OUT_OVERSIZE"
check "oversized inner envelope emits no stderr" test ! -s "$OUT_OVERSIZE.err"
await_locks "$XDG_OVERSIZE" || true

echo "== down coordinator is bounded, private, and singleflight-coalesced =="
XDG_SLOW="$TMP/xdg-slow"
OUT_SLOW1="$TMP/slow1.out"
OUT_SLOW2="$TMP/slow2.out"
mkdir -p "$XDG_SLOW"
: >"$STATE/starts.log"
t0="$(date +%s%3N)"
run_hook "$XDG_SLOW" slow down-daemon-01 "$OUT_SLOW1" &
hook_pid=$!
sleep 0.2
buffer="$(find "$XDG_SLOW" -maxdepth 1 -type f -name 'north-tooluse-output.*' -print -quit)"
if [ -n "$buffer" ] && [ "$(stat -c %a "$buffer" 2>/dev/null)" = 600 ]; then
  ok "peer-mail buffer is an unpredictable mktemp file with mode 0600"
else
  bad "peer-mail buffer is an unpredictable mktemp file with mode 0600"
fi
wait "$hook_pid"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "down-daemon hook exits zero" test "$rc" -eq 0
check "down-daemon hook stays under 2.5s (${elapsed}ms)" test "$elapsed" -lt 2500
check "message printed before stalled ack is still delivered" grep -Fq "body before stalled ack" "$OUT_SLOW1"
check "quotes, backslashes, and control characters remain valid JSON" valid_control_character_json "$OUT_SLOW1"
check "private buffer is removed after delivery" test -z "$(find "$XDG_SLOW" -maxdepth 1 -name 'north-tooluse-output.*' -print -quit)"

t0="$(date +%s%3N)"
run_hook "$XDG_SLOW" slow down-daemon-01 "$OUT_SLOW2"
rc=$?
elapsed=$(( $(date +%s%3N) - t0 ))
check "repeated down-daemon hook exits zero" test "$rc" -eq 0
check "repeated down-daemon hook stays under 2.5s (${elapsed}ms)" test "$elapsed" -lt 2500
repair_starts="$(grep -c '^repair ' "$STATE/starts.log" || true)"
presence_starts="$(grep -c '^presence ' "$STATE/starts.log" || true)"
check "repeated hooks start only one live repair worker" test "$repair_starts" -eq 1
check "repeated hooks start only one live presence worker" test "$presence_starts" -eq 1
check "all delayed workers and locks settle within the shared deadline" \
  await_workers "$STATE" "$XDG_SLOW"

echo "== stale locks recover and a concurrent cold burst does not duplicate workers =="
XDG_STALE="$TMP/xdg-stale"
STALE_KEY="$(session_key stale-lo)"
mkdir -p "$XDG_STALE/north-agent-routes/$STALE_KEY.repair.lock"
mkdir -p "$XDG_STALE/north-presence-renew/$STALE_KEY.lock"
touch -d '20 seconds ago' \
  "$XDG_STALE/north-agent-routes/$STALE_KEY.repair.lock" \
  "$XDG_STALE/north-presence-renew/$STALE_KEY.lock"
: >"$STATE/starts.log"
run_hook "$XDG_STALE" fast stale-lo "$TMP/stale.out"
check "stale-lock repair completes" await_locks "$XDG_STALE"
check "stale route lock is reclaimed and cache commits" test -s "$XDG_STALE/north-agent-routes/$STALE_KEY"
check "stale renewal lock is reclaimed and marker commits" test -s "$XDG_STALE/north-presence-renew/$STALE_KEY"

XDG_BURST="$TMP/xdg-burst"
mkdir -p "$XDG_BURST"
: >"$STATE/starts.log"
t0="$(date +%s%3N)"
pids=()
for i in $(seq 1 8); do
  run_hook "$XDG_BURST" fast concurrent-cold "$TMP/burst-$i.out" &
  pids+=("$!")
done
burst_ok=1
for pid in "${pids[@]}"; do wait "$pid" || burst_ok=0; done
elapsed=$(( $(date +%s%3N) - t0 ))
check "eight concurrent hooks all exit zero" test "$burst_ok" -eq 1
check "eight concurrent hooks finish under 2s (${elapsed}ms)" test "$elapsed" -lt 2000
check "concurrent maintenance completes" await_locks "$XDG_BURST"
repair_starts="$(grep -c '^repair ' "$STATE/starts.log" || true)"
presence_starts="$(grep -c '^presence ' "$STATE/starts.log" || true)"
check "cold burst publishes identity once" test "$repair_starts" -eq 1
check "cold burst renews presence once" test "$presence_starts" -eq 1

echo "== real inbox helper flushes before a stalled ACK =="
cat >"$TMP/ack-stall-fixture.clj" <<'CLJ'
(require '[clojure.java.io :as io])

(def root (System/getenv "NORTH_TEST_ROOT"))
(def inbox-file (str root "/cli/inbox-peek.clj"))
(let [fixture-file (System/getProperty "babashka.file")]
  (System/setProperty "north.inbox-peek.lib" "1")
  (System/setProperty "babashka.file" inbox-file)
  (try
    (load-file inbox-file)
    (finally
      (System/setProperty "babashka.file" fixture-file))))

(def status-file (System/getenv "NORTH_TEST_STATUS"))
(def calls (atom []))
(defn mismatch! [message operation]
  (spit status-file (str "mismatch: " message " " (pr-str operation)))
  (System/exit 2))
(defn fake-send-op [_port operation]
  (let [step (count (swap! calls conj operation))]
    (case (:op operation)
      :query-page
      (do
        (when-not (and (= 1 step)
                       (= 256 (:limit operation))
                       (nil? (:after operation))
                       (= "pending_message" (get-in operation [:query :find]))
                       (some #(= [{:var "e"} "to" "flush-agent"]
                                 (get-in % [:body 0 :args]))
                             (get-in operation [:query :strata 0])))
          (mismatch! "query-page framing" operation))
        {:ok [["@msg:flush-proof"]]
         :more false :next nil :version 1 :engine "scan"})

      :resolved
      (cond
        (contains? #{"acked_by" "delivery_rejected_by"} (:p operation))
        {:values []}
        (= "to" (:p operation)) {:value "flush-agent"}
        (= "from" (:p operation)) {:value "peer"}
        (= "subject" (:p operation)) {:value "flush proof"}
        (= "body" (:p operation)) {:value "complete body"}
        :else (mismatch! "resolved predicate" operation))

      :acquire-lease
      (if (and (string? (:res operation))
               (.startsWith ^String (:res operation) "message-delivery:")
               (.startsWith ^String (:holder operation)
                            "message-consumer:flush-agent:"))
        {:ok true :epoch 1}
        (mismatch! "delivery lease" operation))

      :assert
      (if (and (= "@msg:flush-proof" (:te operation))
               (= "acked_by" (:p operation))
               (= "flush-agent" (:r operation))
               (= 11 step))
        (do
          (spit status-file "query-page-ok\nack-reached\n")
          (Thread/sleep 30000)
          {:ok true})
        (mismatch! "pre-ACK sequence" operation))

      (mismatch! "unexpected operation" operation))))

(doseq [actor ["" "../escape" (apply str (repeat 513 "a"))]]
  (let [error (try (managed-actor-key actor) nil (catch Exception cause cause))]
    (when-not (= :invalid-inbox-actor (:type (ex-data error)))
      (mismatch! "inbox actor validation" actor))))
(when (= (managed-actor-key "actor:a") (managed-actor-key "actor_a"))
  (mismatch! "inbox actor domain collision" nil))

(with-redefs [north.coord/send-op fake-send-op]
  (run-peek! 1 "flush-agent"))
CLJ
STATUS_FILE="$TMP/ack-stall.status"
set +e
mkdir -p "$TMP/flush-runtime"
timeout --signal=TERM --kill-after=0.1s 0.7s \
  env XDG_RUNTIME_DIR="$TMP/flush-runtime" NORTH_TEST_ROOT="$ROOT" \
  NORTH_TEST_STATUS="$STATUS_FILE" \
  bb "$TMP/ack-stall-fixture.clj" >"$TMP/flush.out" 2>"$TMP/flush.err"
flush_rc=$?
set -e
check "fixture observes exact query-page framing before the pre-ACK claim protocol" \
  grep -Fxq "query-page-ok" "$STATUS_FILE"
check "fixture reaches ACK only after the ten bounded pre-ACK operations" \
  grep -Fxq "ack-reached" "$STATUS_FILE"
check "ACK stall reaches the helper deadline" test "$flush_rc" -eq 124
check "complete subject is flushed before ACK" grep -Fq "✉ from peer — flush proof" "$TMP/flush.out"
check "complete body is flushed before ACK" grep -Fq "  complete body" "$TMP/flush.out"

echo
echo "north-on-tooluse-stress-test: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
