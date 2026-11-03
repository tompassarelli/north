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
git add bin/threads.test.sh
git commit -qm 'add a machine-bound north-binary test'
base=$(git rev-parse HEAD)
printf 'fn g() {}\n' >crates/north-core/src/threads/calibration/x.rs
git add crates/north-core/src/threads/calibration/x.rs
git commit -qm 'change calibration code'
for mode in "" machine-bound; do
  output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" $mode)
  [[ "$output" == *'land check: bin/threads.test.sh'* ]] || { printf 'FAIL: a change under crates/north-core/src/threads/calibration selected no bin/threads.test.sh in mode %s\n' "${mode:-all}" >&2; exit 1; }
done
output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" portable)
[[ "$output" != *'land check: bin/threads.test.sh'* ]] || { printf 'FAIL: portable mode selected a machine-bound test\n' >&2; exit 1; }
printf 'PASS: a change under crates/north-core/src/threads/calibration selects bin/threads.test.sh in the all and machine-bound modes, not portable\n'
for mode in portable machine-bound; do
  grep -E "^	check = .*land-checks\.sh $mode\$" "$safe_push" | grep -qE '= [^:]*(^|[ ,])crates/,' || { printf 'FAIL: the %s land check in .safe-push does not run for a change under crates/\n' "$mode" >&2; exit 1; }
done
printf 'PASS: both land-checks.sh checks in .safe-push run for a change under crates/\n'
