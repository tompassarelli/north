#!/usr/bin/env bash
# shellcheck disable=SC2016
set -uo pipefail

. "$(dirname "$0")/follows-release.lib.sh"
probe 0
sha=$(git -C "$container/main" rev-parse HEAD)
sent() { grep -c "Item: north#$1" "$scratch/bin/started"; }
listed() { "$threads" queue follow-list | grep -c "\"item\": \"north#$1\""; }

: >"$scratch/bin/started"
touch "$scratch/bin/start-fails"
queue 30 "$sha" claude:opus-medium >/dev/null
north follows-release >/dev/null
rm -f "$scratch/bin/start-fails"
printf 'CLOSED\n' >"$scratch/gh-state"
out=$(north follows-release 2>&1)
north follows-release >/dev/null
check '[11] a released box whose own issue is closed is marked done and never sent again' \
  '[ "$(sent 30)" -eq 0 ] && [ "$(listed 30)" -eq 0 ] && grep -q "box 1 north#30: its issue is closed" <<<"$out"'

rm -f "$scratch/gh-state"
touch "$scratch/bin/start-fails"
id=$(queue 31 "$sha" claude:opus-medium)
north follows-release >/dev/null
rm -f "$scratch/bin/start-fails"
north threads queue follow-started "$id" >/dev/null
north follows-release >/dev/null
check '[12] the parent marks a released box staffed elsewhere with one threads command, after which it is never sent again' \
  '[ "$(sent 31)" -eq 0 ] && [ "$(listed 31)" -eq 0 ]'

queue 32 "$sha" claude:opus-medium >/dev/null
north follows-release >/dev/null
check '[13] the staffing prompt names that command with the box id' \
  'grep -q "north threads queue follow-started 3[^0-9]" "$scratch/bin/started"'

check '[4] no north call in the run fell back to the installed runtime' '[ ! -e "$scratch/installed-calls" ]'

printf 'follows-release: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
