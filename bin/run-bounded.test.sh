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

capacity="$repo/harness/skills/machine-capacity/scripts"
"$capacity/build-machine-capacity" "$scratch/machine-capacity.mjs" >/dev/null
mkdir -p "$scratch/runtime"
ln -s "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/systemd" "$scratch/runtime/systemd"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' >"$scratch/cpu.pressure"
echo 0 >"$scratch/gpu-busy"
XDG_RUNTIME_DIR="$scratch/runtime" AGENT_CAPACITY_CPU_PRESSURE="$scratch/cpu.pressure" \
  AGENT_CAPACITY_GPU_BUSY="$scratch/gpu-busy" AGENT_CAPACITY_GPU_CLIENTS=0 CAPACITY_WATCHDOG_DIR="$scratch/no-watchdog" \
  AGENT_CAPACITY_DB="$scratch/capacity.db" AGENT_CAPACITY_USAGE_LOG="$scratch/usage.jsonl" \
  "$run" 1m -- bun "$scratch/machine-capacity.mjs" run \
  --owner bus-test --class moderate --timeout-seconds 60 -- true >"$scratch/bus.out" 2>"$scratch/bus.err"
grep -q RELEASED "$scratch/bus.out" "$scratch/bus.err"

printf 'run-bounded fixture: PASS (usage rejection, exit status, timed subtree reap, user bus reachable)\n'
