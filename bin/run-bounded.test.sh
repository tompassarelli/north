#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
run="$repo/bin/run-bounded"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/run-bounded-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT


expect_usage() {
  set +e
  "$run" "$@" >"$scratch/out" 2>"$scratch/err"
  status=$?
  set -e
  [ "$status" -eq 2 ]
}

expect_usage 0s -- true
expect_usage 25h -- true
expect_usage 30m true

set +e
"$run" 10s -- bash -c 'exit 7'
status=$?
set -e
[ "$status" -eq 7 ]

pid_file="$scratch/host-pid"
set +e
"$run" 1s -- bash -c '
  (awk '\''/^NSpid:/ {print $2}'\'' /proc/self/status >"$1"; exec sleep 30) &
  wait
' bash "$pid_file"
status=$?
set -e
[ "$status" -ne 0 ]
[ -s "$pid_file" ]
child=$(cat "$pid_file")
for _ in $(seq 1 100); do
  [ ! -e "/proc/$child" ] && break
  sleep 0.02
done
[ ! -e "/proc/$child" ]

[ "$("$run" 10s -- stat -c %U /nix/store)" = root ]

[ "$(NOTIFY_SOCKET=/x LISTEN_FDS=3 "$run" 10s -- env | grep -cE '^(NOTIFY_SOCKET|LISTEN_FDS)=' || true)" = 0 ]

capacity="$repo/harness/skills/machine-capacity/scripts"
"$capacity/build-machine-capacity" "$scratch/machine-capacity.mjs" >/dev/null
user_runtime=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
mkdir -p "$scratch/runtime/agent-capacity-v1"
ln -s "$user_runtime/systemd" "$scratch/runtime/systemd"
cp "$user_runtime/agent-capacity-v1/mode" "$scratch/runtime/agent-capacity-v1/mode" 2>/dev/null || true
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' >"$scratch/cpu.pressure"
echo 0 >"$scratch/gpu-busy"
export XDG_RUNTIME_DIR="$scratch/runtime" AGENT_CAPACITY_CPU_PRESSURE="$scratch/cpu.pressure" \
  AGENT_CAPACITY_GPU_BUSY="$scratch/gpu-busy" AGENT_CAPACITY_GPU_CLIENTS=0 CAPACITY_WATCHDOG_DIR="$scratch/no-watchdog" \
  AGENT_CAPACITY_DB="$scratch/capacity.db" AGENT_CAPACITY_USAGE_LOG="$scratch/usage.jsonl"
"$run" 1m -- bun "$scratch/machine-capacity.mjs" run \
  --owner bus-test --class moderate --timeout-seconds 60 -- true >"$scratch/bus.out" 2>"$scratch/bus.err"
grep -q RELEASED "$scratch/bus.out" "$scratch/bus.err"

holding() { bun "$scratch/machine-capacity.mjs" status | jq '.holding | length'; }
"$run" 1m -- bun "$scratch/machine-capacity.mjs" run \
  --owner dead-wrapper --class moderate --timeout-seconds 60 -- sleep 60 >/dev/null 2>&1 &
bounded=$!
for _ in $(seq 1 100); do [ "$(holding)" -eq 1 ] && break; sleep 0.1; done
[ "$(holding)" -eq 1 ]
kill -KILL "$bounded"
wait "$bounded" || true
for _ in $(seq 1 50); do [ "$(holding)" -eq 0 ] && break; sleep 0.1; done
[ "$(holding)" -eq 0 ]

printf 'run-bounded fixture: PASS (usage rejection, exit status, timed subtree reap, root-owned files stay root, user bus reachable, dead wrapper reclaimed)\n'
