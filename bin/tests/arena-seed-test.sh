#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FRAM="${FRAM_HOME:-/home/tom/code/beagle/main/branch-core}"
TMP="$(mktemp -d)"
LOG="$TMP/history.framlog"
SERVER_LOG="$TMP/server.log"
SPACE="arena-seed-test"
PID=""

cleanup() {
  if [[ -n "$PID" ]]; then
    kill "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

PORT=17970
while ss -tlnH "sport = :$PORT" 2>/dev/null | grep -q .; do
  PORT=$((PORT + 1))
done

FRAM_SINGLE_VALUED="title exp_id arm task_id state tokens wall_s updated" \
FRAM_SERVER_RUNTIME=jvm-dev \
  "$FRAM/bin/fram-server" serve "$PORT" "$LOG" "$SPACE" \
  >"$SERVER_LOG" 2>&1 &
PID=$!

for _ in $(seq 1 160); do
  if ss -tlnH "sport = :$PORT" 2>/dev/null | grep -q .; then
    break
  fi
  if ! kill -0 "$PID" 2>/dev/null; then
    cat "$SERVER_LOG" >&2
    exit 1
  fi
  sleep 0.25
done
ss -tlnH "sport = :$PORT" 2>/dev/null | grep -q . || {
  cat "$SERVER_LOG" >&2
  echo "arena seed test: Fram server did not listen" >&2
  exit 1
}

EXP="arena-seed-test-$$"
OUTPUT="$(
  FRAM_HOME="$FRAM" FRAM_OUT="$FRAM/out" FRAM_SPACE_ID="$SPACE" \
    NORTH_PORT="$PORT" NORTH_ARENA_NO_SLEEP=1 \
    timeout 30s "$ROOT/bin/arena-seed" "$EXP"
)"
grep -Fq "done. control landed 3/5" <<<"$OUTPUT"

RESULT="$(
  bb -cp "$ROOT/out:$FRAM/out" -e '
    (load-file (str (first *command-line-args*) "/cli/framrpc-client.clj"))
    (require (quote [fram.types :as t])
             (quote [north.framrpc-client :as rpc]))
    (let [port (parse-long (second *command-line-args*))
          space (nth *command-line-args* 2)
          exp (nth *command-line-args* 3)
          client (rpc/connect "127.0.0.1" port space)]
      (try
        (println
         (pr-str
          (mapv
           (fn [[arm idx]]
             (some-> (:rows (rpc/scan-all!
                             client (str "@arena-" exp "-" arm "-" idx)
                             "state" nil))
                     first t/triple-t3))
           [["graph" 4] ["control" 4] ["control" 2]])))
        (finally (rpc/close! client))))' \
    "$ROOT" "$PORT" "$SPACE" "$EXP"
)"
[[ "$RESULT" == '["green" "failed" "blocked"]' ]]

echo "arena seed test: PASS (canonical FRAMRPC/FRAMLOG, current Fram main, no 60s delay)"
