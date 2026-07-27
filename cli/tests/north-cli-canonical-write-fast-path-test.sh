#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
scratch=$(mktemp -d -t north-canonical-write-fast.XXXXXX)
trap 'rm -rf "${scratch:?}"' EXIT
fake_fram=$scratch/fram
calls=$scratch/calls
bb_calls=$scratch/bb-calls
mkdir -p "$fake_fram"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'printf "%s\n" "fram $*" >>"$TEST_CALLS"' \
  'case "${1:-}" in' \
  '  show)' \
  '    if [[ "${2:-}" = 019fa4d4-93aa-7447-aae5-0a5bcfca6849 ]]; then' \
  '      printf "%s\n" "  title  existing thread"' \
  '    else' \
  '      printf "%s\n" "no facts for @${2:-}"' \
  '    fi ;;' \
  '  tell|untell) printf "%s\n" "committed via coordinator (v2): ${2:-} ${3:-} = ${4:-}" ;;' \
  'esac' \
  >"$fake_fram/fram"
chmod +x "$fake_fram/fram"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'printf "%s\n" "bb $*" >>"$TEST_BB_CALLS"' \
  'printf "%s\n" "@019fa4d4-93aa-7447-aae5-0a5bcfca6849"' \
  >"$scratch/bb"
chmod +x "$scratch/bb"

common_env=(
  HOME="$scratch/home"
  FRAM_BIN="$fake_fram"
  NORTH_BB="$scratch/bb"
  TEST_CALLS="$calls"
  TEST_BB_CALLS="$bb_calls"
)

env "${common_env[@]}" "$root/bin/north" tell \
  019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress "cli-fix probe" \
  >"$scratch/exact.out"
grep -q '^fram show 019fa4d4-93aa-7447-aae5-0a5bcfca6849$' "$calls"
grep -q '^fram tell 019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress cli-fix probe$' "$calls"
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
grep -q '^fram show 019fa4d4-93aa-7447-aae5-0a5bcfca6800$' "$calls"

: >"$calls"
env "${common_env[@]}" "$root/bin/north" tell @foundation progress handle \
  >"$scratch/handle.out"
grep -q -- '-m north.main resolve @foundation' "$bb_calls"
grep -q '^fram tell 019fa4d4-93aa-7447-aae5-0a5bcfca6849 progress handle$' "$calls"

echo "north-cli-canonical-write-fast-path: PASS"
