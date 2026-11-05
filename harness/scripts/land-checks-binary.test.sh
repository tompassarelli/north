#!/usr/bin/env bash
set -euo pipefail
script=$(realpath "$(dirname "$0")/land-checks.sh")
safe_push=$(dirname "$script")/../../.safe-push
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
git -C "$scratch" init -q
git -C "$scratch" config user.name 'Land binary fixture'
git -C "$scratch" config user.email 'fixture@example.invalid'
git -C "$scratch" config core.hooksPath /dev/null
cd "$scratch"
mkdir -p bin lib harness/hooks harness/lib harness/scripts crates/north-core/src/threads/calibration
printf 'north="$NORTH_BIN"; printf "threads test ran\\n"\n' >bin/threads.test.sh
printf 'north="$NORTH_BIN"; printf "bounded test ran\\n"\n' >bin/run-bounded.test.sh
git add bin/threads.test.sh bin/run-bounded.test.sh
git commit -qm 'add a portable and a machine-bound north-binary test'
base=$(git rev-parse HEAD)
printf 'fn g() {}\n' >crates/north-core/src/threads/calibration/x.rs
git add crates/north-core/src/threads/calibration/x.rs
git commit -qm 'change calibration code'
expect() {
  local mode=$1 selected=$2 absent=$3 output
  output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" $mode)
  [[ "$output" == *"land check: $selected"* ]] || { printf 'FAIL: a change under crates/north-core/src/threads/calibration selected no %s in mode %s\n' "$selected" "${mode:-all}" >&2; exit 1; }
  [[ "$output" != *"land check: $absent"* ]] || { printf 'FAIL: mode %s selected %s, which belongs to the other mode\n' "${mode:-all}" "$absent" >&2; exit 1; }
}
expect portable bin/threads.test.sh bin/run-bounded.test.sh
expect machine-bound bin/run-bounded.test.sh bin/threads.test.sh
output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script")
[[ "$output" == *'land check: bin/threads.test.sh'* && "$output" == *'land check: bin/run-bounded.test.sh'* ]] || { printf 'FAIL: the all mode did not select both north-binary tests\n' >&2; exit 1; }
printf 'PASS: a change under crates/north-core/src/threads/calibration selects bin/threads.test.sh in portable mode and bin/run-bounded.test.sh in machine-bound mode, both in the all mode\n'
for mode in portable machine-bound; do
  grep -E "^	(check|after) = .*land-checks\.sh $mode\$" "$safe_push" | grep -qE '= [^:]*(^|[ ,])crates/,' || { printf 'FAIL: the %s land check in .safe-push does not run for a change under crates/\n' "$mode" >&2; exit 1; }
done
printf 'PASS: both land-checks.sh checks in .safe-push run for a change under crates/\n'
