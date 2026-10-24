#!/usr/bin/env bash
set -uo pipefail
scratch=$(mktemp -d "${TMPDIR:-/tmp}/tree-north-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/harness/lib" "$scratch/src" "$scratch/target/debug"
cp "$(dirname "$0")/tree-north.sh" "$scratch/harness/lib/"
bin=$scratch/target/debug/north
printf 'fn main() {}\n' >"$scratch/src/main.rs"; : >"$scratch/src/tests.rs"; : >"$scratch/harness/defaults.toml"
printf '#!/bin/sh\n' >"$bin"; chmod +x "$bin"
printf '%s: %s %s\n' "$bin" "$scratch/src/main.rs" "$scratch/harness/defaults.toml" >"$bin.d"
touch -d '2026-01-01 00:00' "$scratch/src/main.rs" "$scratch/src/tests.rs" "$scratch/harness/defaults.toml"
touch -d '2026-01-02 00:00' "$bin"
pass=0 fail=0
resolve() {
  # shellcheck disable=SC2016
  (cd /tmp && env -u CARGO_TARGET_DIR "$@" bash -c '. "$1"; printf "%s\n" "$NORTH_PACE_BIN"' resolve "$scratch/harness/lib/tree-north.sh" 2>"$scratch/stderr")
}
check() {
  if "${@:2}"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n      out=%s err=%s\n' "$1" "$out" "$(cat "$scratch/stderr")"; fi
}
build='cargo build --locked --offline --bin north'
resolved() { [ "$status" -eq 0 ] && [ "$out" = "$bin" ]; }
refused() { [ "$status" -eq 1 ] && [ -z "$out" ] && grep -qF "$build" "$scratch/stderr"; }
out=$(resolve -u NORTH_PACE_BIN); status=$?
check 'a binary newer than every source it was built from is used' resolved
touch -d '2026-01-03 00:00' "$scratch/src/tests.rs"
out=$(resolve -u NORTH_PACE_BIN); status=$?
check 'a newer file the binary was not built from, such as a test module, leaves it usable' resolved
touch -d '2026-01-03 00:00' "$scratch/src/main.rs"
out=$(resolve -u NORTH_PACE_BIN); status=$?
check 'a src/ file the binary was built from, newer than it, refuses and names the build command' refused
touch -d '2026-01-01 00:00' "$scratch/src/main.rs"; touch -d '2026-01-03 00:00' "$scratch/harness/defaults.toml"
out=$(resolve -u NORTH_PACE_BIN); status=$?
check 'an embedded file outside src/, newer than the binary, refuses' refused
out=$(resolve NORTH_PACE_BIN="$bin"); status=$?
check 'an explicit NORTH_PACE_BIN is used without an age check' resolved
touch -d '2026-01-01 00:00' "$scratch/harness/defaults.toml"; rm "$bin.d"
out=$(resolve -u NORTH_PACE_BIN); status=$?
check 'a binary without its cargo dep-info record refuses' refused
rm "$bin"
out=$(resolve -u NORTH_PACE_BIN); status=$?
check 'a missing binary refuses and names the build command' refused
printf 'tree-north: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
