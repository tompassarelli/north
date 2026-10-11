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

printf 'run-bounded fixture: PASS (usage rejection, exit status, timed subtree reap)\n'
