#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "${scratch:?}"' EXIT
conf="$scratch/harness.conf"

printf 'theme=dark\ndispatch=north\nguards=on\ncustom=value\n' > "$conf"
before="$(cat "$conf")"
output="$(NORTH_HARNESS_CONF="$conf" "$repo/bin/north" panic)"
after="$(cat "$conf")"

test "$before" = $'theme=dark\ndispatch=north\nguards=on\ncustom=value'
test "$after" = $'theme=dark\ncustom=value\ndispatch=native\nguards=off'
grep -F 'north config dispatch north' <<<"$output" >/dev/null
grep -F 'north config guards on' <<<"$output" >/dev/null

printf 'north panic test: PASS\n'
