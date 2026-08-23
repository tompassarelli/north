#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "${scratch:?}"' EXIT
conf="$scratch/harness.conf"

printf 'theme=dark\ndispatch=managed\ncustom=value\n' > "$conf"
before="$(cat "$conf")"
output="$(NORTH_HARNESS_CONF="$conf" "$repo/bin/north" panic)"
after="$(cat "$conf")"

test "$before" = $'theme=dark\ndispatch=managed\ncustom=value'
test "$after" = $'theme=dark\ncustom=value\ndispatch=native'
grep -F 'north config dispatch managed' <<<"$output" >/dev/null

printf 'north panic test: PASS\n'
