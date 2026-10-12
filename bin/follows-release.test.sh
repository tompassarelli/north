#!/usr/bin/env bash
# shellcheck disable=SC2016
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
threads=$repo/bin/threads
scratch=$(mktemp -d "${TMPDIR:-/tmp}/follows-release-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export THREADS_DB=$scratch/threads.db XDG_STATE_HOME=$scratch/state LEDGER_HANDOFFS=$scratch/handoffs
export SPAWN_CAPACITY_HOME=$scratch/home SPAWN_CAPACITY_STATUS=$scratch/probe.json
export AGENTS_ORCHESTRATION=$scratch/orchestration.toml AGENTS_ORG_FILE=$scratch/org.json
export FOLLOWS_RELEASE_ROOT=$scratch/code FOLLOWS_RELEASE_START="$scratch/bin/start"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid
unset AGENT_ROLE AGENT_DELEGATION_BUDGET AGENT_ORG_NAME CODEX_THREAD_ID
: >"$AGENTS_ORCHESTRATION"
mkdir -p "$scratch/bin" "$SPAWN_CAPACITY_HOME/.local/state/agents"
printf '#!/usr/bin/env bash\nexit 0\n' >"$scratch/bin/gitleaks"
printf '#!/usr/bin/env bash\nexit 1\n' >"$scratch/bin/gh"
cat >"$scratch/bin/start" <<'SH'
#!/usr/bin/env bash
[ ! -e "${0%/*}/start-fails" ] || exit 1
printf '%s %s %s\n' "$(date +%s.%N)" "$1" "$2" >>"${0%/*}/started"
SH
chmod +x "$scratch/bin/"*
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
  | "$threads" queue follow "north#$1" --tier "$3" --lead galileo-lead; }

probe 0
queue 7 'Land the predecessor box' codex:medium >/dev/null
landed_at=$(date +%s.%N)
out=$(cd "$container/worktrees/lane" && "$repo/bin/safe-push" --to main 2>&1); status=$?
check '[1] a Follows box starts on its tier within 60 s of safe-push landing its predecessor' \
  '[ "$status" -eq 0 ] && read -r at tier lead <"$scratch/bin/started" && [ "$tier $lead" = "codex:medium galileo-lead" ] && awk -v a="$at" -v b="$landed_at" "BEGIN { exit !(a - b < 60) }" && grep -q "box 1 north#7: started" <<<"$out"' \
  || printf '%s\n' "$out"

probe 8
queue 8 'Land the predecessor box' worker >/dev/null
"$repo/bin/follows-release" >/dev/null
probe 0
touch "$scratch/bin/start-fails"
queue 9 "$(git -C "$container/main" rev-parse origin/main)" codex:high >/dev/null
"$repo/bin/follows-release" >/dev/null
first=$(grep -c . "$LEDGER_HANDOFFS/galileo-lead-inbox.md")
FOLLOWS_RELEASE_NOW=$(($(date +%s) + 301)) "$repo/bin/follows-release" >/dev/null
FOLLOWS_RELEASE_NOW=$(($(date +%s) + 601)) "$repo/bin/follows-release" >/dev/null
check '[3] a box the spawn gate denies, and one not started within 5 min, each alert their lead and the proxy in exactly one line' \
  '[ "$first" -eq 1 ] && grep -q "box 2 north#8 .*denied by the spawn gate" "$LEDGER_HANDOFFS/galileo-lead-inbox.md" && [ "$(grep -c "box 2 north#8" "$LEDGER_HANDOFFS/galileo-lead-inbox.md")" -eq 1 ] && [ "$(grep -c "box 3 north#9 .*not started within 5 minutes" "$LEDGER_HANDOFFS/galileo-lead-inbox.md")" -eq 1 ] && [ "$(grep -c . "$LEDGER_HANDOFFS/galileo-lead-inbox.md")" -eq 2 ] && cmp -s "$LEDGER_HANDOFFS/galileo-lead-inbox.md" "$LEDGER_HANDOFFS/proxy-inbox.md"'

printf 'follows-release: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
