repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
threads=$repo/bin/threads
scratch=$(mktemp -d "${TMPDIR:-/tmp}/follows-release-test.XXXXXX")
export scratch
export NORTH_LOCAL=$(cd "$repo" && realpath "${CARGO_TARGET_DIR:-target}")/debug/north
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/installed"
cat >"$scratch/installed/north" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$scratch/installed-calls"
exit 97
SH
chmod +x "$scratch/installed/north"
export PATH=$scratch/installed:$PATH
export THREADS_DB=$scratch/threads.db XDG_STATE_HOME=$scratch/state
export SPAWN_CAPACITY_HOME=$scratch/home SPAWN_CAPACITY_STATUS=$scratch/probe.json
export AGENTS_ORCHESTRATION=$scratch/orchestration.toml
export FOLLOWS_RELEASE_ROOT=$scratch/code
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid
: >"$AGENTS_ORCHESTRATION"
mkdir -p "$scratch/bin" "$SPAWN_CAPACITY_HOME/.local/state/agents"
printf '#!/usr/bin/env bash\nexit 0\n' >"$scratch/bin/gitleaks"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >>"$scratch/gh-calls"\ncat "$scratch/gh-state" 2>/dev/null && exit 0\nexit 1\n' >"$scratch/bin/gh"
cat >"$scratch/bin/north" <<'SH'
#!/usr/bin/env bash
if [ "$1 $2" = "fleet send" ]; then
  if [ -e "$scratch/bin/start-fails" ] && [[ "$*" == *"Staff this queued box now"* ]]; then exit 1; fi
printf '%s\n' "$*" >>"$scratch/bin/fleet-sends"
  if [[ "$*" == *"Staff this queued box now"* ]]; then printf '%s %s\n' "$(date +%s.%N)" "$*" >>"$scratch/bin/started"; fi
else
exec "$NORTH_LOCAL" "$@"
fi
SH
chmod +x "$scratch/bin/north" "$scratch/bin/gh"
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
printf '#!/usr/bin/env bash\nwhile read -r _ _ ref; do [ "$ref" = refs/heads/main ] && [ ! -e "%s/landed-at" ] && date +%%s.%%N >"%s/landed-at"; done\nexit 0\n' "$scratch" "$scratch" >"$container/remote/north.git/hooks/post-receive"
chmod +x "$container/remote/north.git/hooks/post-receive"
git -C "$container/main" worktree add -q "$container/worktrees/lane" -b lane
git -C "$container/worktrees/lane" commit -q --allow-empty -m 'Land the predecessor box'

pass=0 fail=0
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n' "$1"; fi
}
queue() { printf 'Delegation: role=worker budget=0\nItem: north#%s\nFollows: %s\nCategory: feature\n' "$1" "$2" \
  | "$threads" queue follow "north#$1" --tier "$3" --parent galileo-parent; }
