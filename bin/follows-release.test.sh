#!/usr/bin/env bash
# shellcheck disable=SC2016
set -uo pipefail

. "$(dirname "$0")/follows-release.lib.sh"

probe 0
queue 7 'Land the predecessor box' codex:medium >/dev/null
out=$(cd "$container/worktrees/lane" && SAFE_PUSH_TEST_REAL_NORTH="$NORTH_LOCAL" "$repo/bin/safe-push" --to main && pwd -P 2>&1); status=$?
landed_at=$(cat "$scratch/landed-at" 2>/dev/null)
check '[1] safe_push_caller_can_run_pwd_after_landing: Follows box starts on tier after the landing, by its own follows-release send' \
  '[ ! -e "$scratch/installed-calls" ] && [ "$status" -eq 0 ] && [ -n "$landed_at" ] && read -r at _ <"$scratch/bin/started" && awk -v a="$at" -v b="$landed_at" "BEGIN { exit !(a >= b) }" && grep -q "fleet send --from service:follows-release galileo-parent" "$scratch/bin/started" && grep -q "^Staff this queued box now" "$scratch/bin/started" && grep -q "fresh worker at codex:medium" "$scratch/bin/started" && grep -q "box 1 north#7: started" <<<"$out" && grep -q "safe-push: landed [0-9a-f]" <<<"$out" && grep -q "cd $container/main when finished in the lane" <<<"$out" && [ "$(tail -n 1 <<<"$out")" = "$container/worktrees/lane" ]' \
  || printf '%s\nstatus=%s landed_at=%s\nstarted:\n%s\n' "$out" "$status" "$landed_at" "$(cat "$scratch/bin/started" 2>&1)"

git -C "$container/main" push -q origin HEAD:refs/heads/after-main; pushed=$?
check '[2] the landing stamp marks the first update of main and no later ref' '[ "$pushed" -eq 0 ] && [ -n "$landed_at" ] && [ "$(cat "$scratch/landed-at")" = "$landed_at" ]'

probe 8
queue 8 'Land the predecessor box' worker >/dev/null
north follows-release >/dev/null
probe 0
touch "$scratch/bin/start-fails"
queue 9 "$(git -C "$container/main" rev-parse origin/main)" codex:high >/dev/null
north follows-release >/dev/null
FOLLOWS_RELEASE_NOW=$(($(date +%s) + 301)) north follows-release >/dev/null
FOLLOWS_RELEASE_NOW=$(($(date +%s) + 601)) north follows-release >/dev/null
check '[3] a box denied by the spawn gate and one not started within 5 min each alert the assigned parent once through Fleet' \
  '[ "$(grep -c "fleet send --from service:follows-release galileo-parent FOLLOWS .*box 2 north#8 .*denied by the spawn gate" "$scratch/bin/fleet-sends")" -eq 1 ] && [ "$(grep -c "fleet send --from service:follows-release galileo-parent FOLLOWS .*box 3 north#9 .*not started within 5 minutes" "$scratch/bin/fleet-sends")" -eq 1 ] && [ "$(grep -c "^fleet send --from service:follows-release galileo-parent FOLLOWS" "$scratch/bin/fleet-sends")" -eq 2 ]'

check '[4] no north call in the run fell back to the installed runtime' '[ ! -e "$scratch/installed-calls" ]'

printf 'follows-release: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
