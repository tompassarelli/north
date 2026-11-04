#!/usr/bin/env bash
set -euo pipefail
script=$(realpath "$(dirname "$0")/land-checks.sh")
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
git -C "$scratch" init -q
git -C "$scratch" config user.name 'Land select fixture'
git -C "$scratch" config user.email 'fixture@example.invalid'
git -C "$scratch" config core.hooksPath /dev/null
cd "$scratch"
mkdir -p bin lib harness/hooks harness/lib harness/scripts crates/north-core/src crates/north-landing/src crates/north-base/src
printf 'north="$(dirname "$0")/../../target/debug/north"; printf "direct test ran\\n"\n' >harness/hooks/direct.test.sh
printf 'binary_variable = "NORTH_BIN"\n' >harness/lib/ledger_client.py
printf 'import sys\nsys.path.insert(0, "harness/lib")\nimport ledger_client\nprint("ledger test ran")\n' >harness/lib/ledger.test.py
printf 'printf "unrelated test ran\\n"\n' >harness/hooks/unrelated.test.sh
printf 'fn main() {}\n' >crates/north-core/src/lib.rs
git add -A; git commit -qm base
for change in crates/north-landing/src/x.rs crates/north-base/src/y.rs crates/north-core/src/lib.rs crates/north-core/Cargo.toml build.rs; do
  base=$(git rev-parse HEAD); mkdir -p "$(dirname "$change")"; printf 'changed %s\n' "$change" >>"$change"; git add -A; git commit -qm "change $change"
  output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" portable)
  [[ "$output" == *'land check: harness/hooks/direct.test.sh'* && "$output" == *'land check: harness/lib/ledger.test.py'* && "$output" != *unrelated.test.sh* && "$output" == *'land check: running '*' land-tests '*'harness/hooks/direct.test.sh'* && "$output" == *'land check: started harness/hooks/direct.test.sh'* ]] || { printf 'FAIL: a change to %s did not select exactly the tests that run north, each named before it ran\n' "$change" >&2; exit 1; }
done
printf 'PASS: any crates/ change, crate manifest or build.rs selects the tests that run the north binary and not unrelated ones\n'
listed=$scratch/listed; mkdir -p "$listed/harness/scripts" "$listed/bin"; cp "$script" "$listed/harness/scripts/land-checks.sh"
printf 'printf "present\\n"\n' >"$listed/bin/present.test.sh"; printf '# listed\nbin/present.test.sh\nharness/hooks/gone.test.sh\n' >"$listed/harness/machine-bound-tests"
for mode in portable machine-bound; do
  if output=$(SAFE_PUSH_LAND_BASE=HEAD SAFE_PUSH_LAND_HEAD=HEAD bash "$listed/harness/scripts/land-checks.sh" "$mode" 2>&1); then printf 'FAIL: a listed machine-bound test that does not exist passed in mode %s\n' "$mode" >&2; exit 1; fi
  [[ "$output" == *'harness/hooks/gone.test.sh'* && "$output" != *'bin/present.test.sh'* ]] || { printf 'FAIL: only the missing machine-bound test should be named in mode %s\n' "$mode" >&2; exit 1; }
done
printf 'PASS: a listed machine-bound test file that does not exist fails the run and is named\n'
all=$scratch/all; mkdir -p "$all/bin" "$all/lib" "$all/harness/hooks" "$all/harness/lib" "$all/harness/scripts" "$all/crates/x"; git -C "$all" init -q
printf 'printf "all-a ran\\n"\n' >"$all/bin/a.test.sh"; printf 'print("all-b ran")\n' >"$all/harness/hooks/b.test.py"; printf 'printf "all-c ran\\n"\n' >"$all/lib/c.test.sh"; printf 'printf "all-d ran\\n"\n' >"$all/harness/scripts/d.test.sh"; printf 'printf "all-e ran\\n"\n' >"$all/crates/x/e.test.sh"
output=$(cd "$all" && env -u SAFE_PUSH_LAND_BASE -u SAFE_PUSH_LAND_HEAD bash "$script" portable --all 2>&1); printf '%s\n' "$output"
for test in bin/a.test.sh harness/hooks/b.test.py lib/c.test.sh harness/scripts/d.test.sh; do [ "$(printf '%s\n' "$output" | grep -Fxc "land check: $test")" -eq 1 ] || { printf 'FAIL: portable --all did not select %s\n' "$test" >&2; exit 1; }; done
[[ "$output" != *crates/x/e.test.sh* ]]
printf 'PASS: portable --all selects every portable test under the roots without a diff\n'
