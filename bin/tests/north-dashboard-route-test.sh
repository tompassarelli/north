#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "${scratch:?}"' EXIT
args="$scratch/args"
fake_bb="$scratch/bb"

printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$@" > "${NORTH_TEST_ARGS:?}"\n' > "$fake_bb"
chmod +x "$fake_bb"

printf 'north dashboard route test: invoking dashboard\n'
NORTH_BB="$fake_bb" \
NORTH_TEST_ARGS="$args" \
NORTH_VERB_SLOTS=0 \
BEAGLE_STORE_HOME="$scratch/store" \
BEAGLE_STORE_BIN="$scratch/store/bin/beagle" \
BEAGLE_STORE_OUT="$scratch/store/out" \
  "$repo/bin/north" dashboard

expected=(
  -cp
  "$repo/out:$scratch/store/out"
  --init
  "$repo/cli/coord.clj"
  -m
  north.main
  cockpit
)
mapfile -t actual < "$args"
test "${actual[*]}" = "${expected[*]}"

printf 'north dashboard route test: PASS\n'
