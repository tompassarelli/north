#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
scratch=$(mktemp -d -t north-canonical-write-fast.XXXXXX)
trap 'rm -rf "${scratch:?}"' EXIT
fake_store=$scratch/store
calls=$scratch/calls
bb_calls=$scratch/bb-calls
mkdir -p "$fake_store"

# The single-quoted arguments are the source of the fake Beagle Store executable.
# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'if [ "${1:-}" = store ]; then shift; fi' \
  'printf "%s\n" "store $*" >>"$TEST_CALLS"' \
  'case "${1:-}" in' \
  '  tell-existing|untell-existing)' \
  '    if [[ "${2:-}" = 019fa4d4-93aa-7447-aae5-0a5bcfca6849 ]]; then' \
  '      printf "%s\n" "committed via coordinator (v2): ${2:-} ${3:-} = ${4:-}"' \
  '    else' \
  '      exit 3' \
  '    fi ;;' \
  '  tell|untell) printf "%s\n" "committed via coordinator (v2): ${2:-} ${3:-} = ${4:-}" ;;' \
  'esac' \
  >"$fake_store/store"
chmod +x "$fake_store/store"

# The single-quoted arguments are the source of the fake Babashka executable.
# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'printf "%s\n" "bb $*" >>"$TEST_BB_CALLS"' \
  'printf "%s\n" "@019fa4d4-93aa-7447-aae5-0a5bcfca6849"' \
  >"$scratch/bb"
chmod +x "$scratch/bb"

common_env=(
  HOME="$scratch/home"
  BEAGLE_STORE_HOME="$fake_store"
  BEAGLE_STORE_BIN="$fake_store"
  BEAGLE_STORE_OUT="$fake_store/out"
  BEAGLE_STORE_CLI="$fake_store/store"
  NORTH_BB="$scratch/bb"
  TEST_CALLS="$calls"
  TEST_BB_CALLS="$bb_calls"
)

env "${common_env[@]}" "$root/bin/north" tell \
  019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress "cli-fix probe" \
  >"$scratch/exact.out"
grep -q '^store tell-existing 019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress cli-fix probe$' "$calls"
[[ "$(wc -l <"$calls")" -eq 1 ]]
[[ ! -e "$bb_calls" ]]

: >"$calls"
env "${common_env[@]}" "$root/bin/north" retract \
  019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress "cli-fix probe" \
  >"$scratch/exact-retract.out"
grep -q '^store untell-existing 019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress cli-fix probe$' "$calls"
[[ "$(wc -l <"$calls")" -eq 1 ]]
[[ ! -e "$bb_calls" ]]

: >"$calls"
if env "${common_env[@]}" "$root/bin/north" tell \
  019fa4d4-93aa-7447-aae5-0a5bcfca6800 progress miss \
  >"$scratch/missing.out" 2>&1; then
  echo "canonical write fast path: missing UUID was not refused" >&2
  exit 1
fi
grep -q 'REFUSED — unresolved id-like ref' "$scratch/missing.out"
[[ "$(wc -l <"$calls")" -eq 1 ]]
grep -q '^store tell-existing 019fa4d4-93aa-7447-aae5-0a5bcfca6800 progress miss$' "$calls"

: >"$calls"
env "${common_env[@]}" "$root/bin/north" tell @foundation progress handle \
  >"$scratch/handle.out"
grep -q -- '-m north.main resolve @foundation' "$bb_calls"
grep -q '^store tell 019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress handle$' "$calls"

echo "north-cli-canonical-write-fast-path: PASS"
