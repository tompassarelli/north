#!/usr/bin/env bash
# shellcheck disable=SC2016
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
threads=$repo/bin/threads
scratch=$(mktemp -d "${TMPDIR:-/tmp}/follows-release-test.XXXXXX")
export scratch
export NORTH_LOCAL=$repo/target/debug/north
trap 'rm -rf "${scratch:?}"' EXIT
export THREADS_DB=$scratch/threads.db XDG_STATE_HOME=$scratch/state
export SPAWN_CAPACITY_HOME=$scratch/home SPAWN_CAPACITY_STATUS=$scratch/probe.json
export AGENTS_ORCHESTRATION=$scratch/orchestration.toml
export FOLLOWS_RELEASE_ROOT=$scratch/code
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid
REAL_NORTH=$(command -v north)
export REAL_NORTH
: >"$AGENTS_ORCHESTRATION"
mkdir -p "$scratch/bin" "$SPAWN_CAPACITY_HOME/.local/state/agents"
printf '#!/usr/bin/env bash\nexit 0\n' >"$scratch/bin/gitleaks"
printf '#!/usr/bin/env bash\nexit 1\n' >"$scratch/bin/gh"
cat >"$scratch/bin/north" <<'SH'
#!/usr/bin/env bash
if [ "$1 $2" = "fleet send" ]; then
  if [ -e "$scratch/bin/start-fails" ] && [[ "$*" == *"Staff this queued box now"* ]]; then exit 1; fi
printf '%s\n' "$*" >>"$scratch/bin/fleet-sends"
  if [[ "$*" == *"Staff this queued box now"* ]]; then printf '%s %s\n' "$(date +%s.%N)" "$*" >>"$scratch/bin/started"; fi
elif [ "$1" = follows-release ]; then
exec "$NORTH_LOCAL" "$@"
else
exec "$REAL_NORTH" "$@"
fi
SH
chmod +x "$scratch/bin/north"
export PATH=$scratch/bin:$PATH
printf '{"claude":{"epoch":%s,"stale_min":600,"refusal":null},"codex":{"epoch":%s,"stale_min":600,"refusal":null}}\n' \
  "$(date +%s)" "$(date +%s)" >"$SPAWN_CAPACITY_HOME/.local/state/agents/usage-gate.json"
probe() { printf '{"committedBatchCpus":%s,"aggregateCpuLimit":8,"protectedCpuSomeAvg10":0}\n' "$1" >"$SPAWN_CAPACITY_STATUS"; }

container=$scratch/code/north
git init -q --bare -b main "$container/remote/north.git"
git init -q -b main "$container/main"
git -C "$container/main" commit -q --allow-empty -m base
git -C "$container/main" remote add origin "$container/remote/north.git"
git -C "$container/main" push -q -u origin main
git -C "$container/main" worktree add -q "$container/worktrees/lane" -b lane
git -C "$container/worktrees/lane" commit -q --allow-empty -m 'Land the predecessor box'

pass=0 fail=0
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n' "$1"; fi
}
queue() { printf 'Delegation: role=worker budget=0\nItem: north#%s\nFollows: %s\nCategory: feature\n' "$1" "$2" \
  | "$threads" queue follow "north#$1" --tier "$3" --parent galileo-parent; }

probe 0
queue 7 'Land the predecessor box' codex:medium >/dev/null
landed_at=$(date +%s.%N)
out=$(cd "$container/worktrees/lane" && "$repo/bin/safe-push" --to main 2>&1); status=$?
check '[1] a Follows box starts on its tier within 60 s of safe-push landing its predecessor' \
  '[ "$status" -eq 0 ] && read -r at _ <"$scratch/bin/started" && awk -v a="$at" -v b="$landed_at" "BEGIN { exit !(a - b < 60) }" && grep -q "fleet send galileo-parent" "$scratch/bin/started" && grep -q "fresh worker at codex:medium" "$scratch/bin/started" && grep -q "box 1 north#7: started" <<<"$out"' \
  || printf '%s\n' "$out"

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
  '[ "$(grep -c "fleet send galileo-parent FOLLOWS .*box 2 north#8 .*denied by the spawn gate" "$scratch/bin/fleet-sends")" -eq 1 ] && [ "$(grep -c "fleet send galileo-parent FOLLOWS .*box 3 north#9 .*not started within 5 minutes" "$scratch/bin/fleet-sends")" -eq 1 ] && [ "$(grep -c "^fleet send galileo-parent FOLLOWS" "$scratch/bin/fleet-sends")" -eq 2 ]'

printf 'follows-release: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
