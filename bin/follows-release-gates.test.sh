#!/usr/bin/env bash
# shellcheck disable=SC2016
set -uo pipefail

. "$(dirname "$0")/follows-release.lib.sh"
printf '{"claude":{"epoch":%s,"stale_min":600,"refusal":null},"codex":{"epoch":1,"stale_min":12168,"refusal":null}}\n' \
  "$(date +%s)" >"$SPAWN_CAPACITY_HOME/.local/state/agents/usage-gate.json"
probe 0
: >"$scratch/bin/started"
rm -f "$scratch/bin/start-fails"
queue 20 "$(git -C "$container/main" rev-parse HEAD)" claude:opus-medium >/dev/null
north follows-release >/dev/null
check '[5] a follow box queued with a claude tier passes the Claude spawn gate while codex is stale' \
  'grep -q "fresh worker at claude:opus-medium" "$scratch/bin/started"'

queue 21 'north#50' claude:opus-medium >/dev/null
git -C "$container/main" pull -q --ff-only origin main
git -C "$container/main" commit -q --allow-empty -m 'Unrelated change (#50)'
git -C "$container/main" push -q origin main
: >"$scratch/bin/started"
printf 'OPEN\n' >"$scratch/gh-state"
north follows-release >/dev/null
open_starts=$(grep -c "Item: north#21" "$scratch/bin/started")
printf 'CLOSED\n' >"$scratch/gh-state"
north follows-release >/dev/null
check '[6] a follow box on an issue ignores an unrelated (#N) subject while the issue is open and starts once it is closed' \
  '[ "$open_starts" -eq 0 ] && [ "$(grep -c "Item: north#21" "$scratch/bin/started")" -eq 1 ]'

queue 22 'wisp#51' claude:opus-medium >/dev/null
north follows-release >/dev/null
check '[7] a follow box whose ref names another repository reads that repository issue, not its own' \
  'grep -qE "^issue view 51 --repo [^ /]+/wisp " "$scratch/gh-calls" && ! grep -qE "^issue view 51 --repo [^ /]+/north " "$scratch/gh-calls" && grep -q "Item: north#22" "$scratch/bin/started"'

queue 23 'north#240 (n240-ledger, north worker-ledger recording fix)' claude:opus-medium >/dev/null
: >"$scratch/bin/started"
printf 'OPEN\n' >"$scratch/gh-state"
north follows-release >/dev/null
note_open=$(grep -c "Item: north#23" "$scratch/bin/started")
printf 'CLOSED\n' >"$scratch/gh-state"
north follows-release >/dev/null
check '[8] a Follows line naming an issue then a note is released once that issue is closed, and not before' \
  '[ "$note_open" -eq 0 ] && [ "$(grep -c "Item: north#23" "$scratch/bin/started")" -eq 1 ] && grep -qE "^issue view 240 --repo [^ /]+/north " "$scratch/gh-calls"'

: >"$scratch/bin/started"
queue 24 "$(git -C "$container/main" rev-parse origin/main) the predecessor commit" claude:opus-medium >/dev/null
north follows-release >/dev/null
check '[9] a Follows line naming a SHA then a note is released once that commit is on origin/main' \
  '[ "$(grep -c "Item: north#24" "$scratch/bin/started")" -eq 1 ]'

queue 26 'Unrelated change (#50)' claude:opus-medium >/dev/null
: >"$scratch/bin/started"
north follows-release >/dev/null
check '[10] a Follows line naming the exact subject of a commit on origin/main is released' \
  '[ "$(grep -c "Item: north#26" "$scratch/bin/started")" -eq 1 ]'

check '[4] no north call in the run fell back to the installed runtime' '[ ! -e "$scratch/installed-calls" ]'

printf 'follows-release: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
