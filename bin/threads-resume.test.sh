#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/threads-resume-test.XXXXXX")
sleep 600 &
holder=$!
trap 'if [ -n "$holder" ]; then kill "$holder" 2>/dev/null || true; fi; rm -rf "${scratch:?}"' EXIT
export HOME=$scratch/home THREADS_DB=$scratch/threads.db AGENTS_ORG_FILE=$scratch/org.json
export NORTH_AGENT_ACTIVE=$scratch/activation.active PATH=$scratch/bin:$PATH
mkdir -p "$scratch/bin" "$HOME/code/smashcraft"
printf 'hook unlanded-work\n' >"$NORTH_AGENT_ACTIVE"
ln -s "$repo/bin/threads" "$scratch/bin/threads"
cat >"$scratch/bin/gh" <<'GH'
#!/usr/bin/env bash
case "$*" in
  "issue list -R tompassarelli/smashcraft"*) printf '[{"number":10,"title":"Resume","labels":[{"name":"priority:now"}],"body":"- [ ] finish","comments":[]}]' ;;
  "issue list"*) printf '[]' ;;
  *refPrefix*) printf '{"data":{"repository":{"refs":{"nodes":[]}}}}' ;;
  *) printf '{"data":{"repository":{"issue":{"number":10,"title":"Resume","state":"OPEN","blockedBy":{"nodes":[]}}}}}' ;;
esac
GH
chmod +x "$scratch/bin/gh"
git init -q --bare --initial-branch=main "$scratch/origin.git"
git init -q --initial-branch=main "$scratch/seed"
git -C "$scratch/seed" config user.name fixture
git -C "$scratch/seed" config user.email fixture@example.invalid
git -C "$scratch/seed" commit -qm base --allow-empty
git -C "$scratch/seed" remote add origin "$scratch/origin.git"
git -C "$scratch/seed" push -q origin main
git -C "$scratch/seed" checkout -qb feature/resume-10
git -C "$scratch/seed" commit -qm $'Resume fixture\n\nRefs smashcraft#10' --allow-empty
git -C "$scratch/seed" tag archive/feature/resume-10
git -C "$scratch/seed" push -q origin refs/tags/archive/feature/resume-10
git -C "$scratch/seed" checkout -q main
git -C "$scratch/seed" checkout -qb feature/other-10
git -C "$scratch/seed" commit -qm $'Other repository fixture\n\nRefs wisp#10' --allow-empty
git -C "$scratch/seed" tag -a archive/feature/other-10 -m 'archived fixture'
git -C "$scratch/seed" push -q origin refs/tags/archive/feature/other-10
git clone -q --no-tags "$scratch/origin.git" "$HOME/code/smashcraft/main"
printf 'Item: smashcraft#10\n' >"$scratch/brief.md"
org() {
  jq -n --argjson pid "$holder" --arg brief "$scratch/brief.md" --arg state "$1" \
    '{version:1,nodes:[{id:"holder",provider:"codex",session:"fixture-session",state:$state,pid:$pid,brief:$brief}]}' >"$AGENTS_ORG_FILE"
}
org active
out=$(threads show smashcraft#10)
grep -q 'archived: archive/feature/resume-10' <<<"$out"
grep -q 'restore: git push origin archive/feature/resume-10:refs/heads/feature/resume-10' <<<"$out"
! grep -q 'archive/feature/other-10' <<<"$out"
printf 'PASS [integration] show fetches only archive tags and prints the correct origin archive and restore command\n'
hook() { printf '{"session_id":"fixture-session","cwd":"%s"}' "$HOME/code/smashcraft/main" | bash "$1"; }
out=$(hook "$repo/socrates/hooks/unlanded-work.sh")
grep -q 'archived: archive/feature/resume-10' <<<"$out"
grep -q 'git push origin archive/feature/resume-10:refs/heads/feature/resume-10' <<<"$out"
mkdir -p "$scratch/provider-hooks/lib"
cp "$repo/socrates/hooks/unlanded-work.sh" "$scratch/provider-hooks/"
cp "$repo/socrates/lib/authoring-killswitch.sh" "$repo/socrates/lib/north-agent-activation.sh" "$scratch/provider-hooks/lib/"
out=$(hook "$scratch/provider-hooks/unlanded-work.sh")
grep -q 'archive/feature/resume-10' <<<"$out"
printf '48h\tsmashcraft\tfixture\tfeature/resume-10\n' >"$scratch/unlanded.txt"
out=$(LANE_GC_UNLANDED="$scratch/unlanded.txt" hook "$repo/socrates/hooks/unlanded-work.sh")
grep -q '^1 unlanded worktrees/branches older than 24 h' <<<"$out"
[ -z "$(AGENT_NO_AUTHORING_HOOKS=1 hook "$repo/socrates/hooks/unlanded-work.sh")" ]
printf 'PASS [integration] source and projected SessionStart list archives, retain the lane note and honor the kill switch\n'
threads claim smashcraft#10 --by codex:fixture-session --eta 5 >/dev/null
sqlite3 "$THREADS_DB" "UPDATE claims SET since = '2020-01-01T00:00:00Z'"
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tcodex:fixture-session\t' <<<"$out"
org finished
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tUNOWNED\t' <<<"$out"
org active
kill "$holder"
wait "$holder" 2>/dev/null || true
holder=""
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tUNOWNED\t' <<<"$out"
jq 'del(.nodes[0].pid)' "$AGENTS_ORG_FILE" >"$scratch/active.json"
mv "$scratch/active.json" "$AGENTS_ORG_FILE"
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tcodex:fixture-session\t' <<<"$out"
printf 'PASS [integration] expired claims stay held for exact active holders and expire for dead or finished holders\n'
mkdir -p "$HOME/.config/threads" "$HOME/.local/state/threads" "$scratch/remote-home"
printf 'fixture-host\n' >"$HOME/.config/threads/remote"
cp "$THREADS_DB" "$scratch/remote.db"
export REMOTE_DB=$scratch/remote.db REMOTE_ORG=$scratch/remote-org.json REMOTE_HOME=$scratch/remote-home SSH_TRACE=$scratch/ssh.trace
printf '{"version":1,"nodes":[]}' >"$REMOTE_ORG"
cat >"$scratch/bin/ssh" <<'SSH'
#!/usr/bin/env bash
printf '%s\n' "${@: -1}" >>"$SSH_TRACE"
HOME=$REMOTE_HOME THREADS_DB=$REMOTE_DB AGENTS_ORG_FILE=$REMOTE_ORG exec bash -c "${@: -1}"
SSH
chmod +x "$scratch/bin/ssh"
out=$(env -u THREADS_DB threads unowned)
grep -q $'smashcraft#10\tpriority:now\tcodex:fixture-session\t' <<<"$out"
grep -q 'THREADS_ALIVE_OWNERS=' "$SSH_TRACE"
! grep -q 'pid' "$SSH_TRACE"
jq '.nodes[0].state = "finished"' "$AGENTS_ORG_FILE" >"$scratch/finished.json"
mv "$scratch/finished.json" "$AGENTS_ORG_FILE"
out=$(env -u THREADS_DB threads unowned)
grep -q $'smashcraft#10\tpriority:now\tUNOWNED\t' <<<"$out"
out=$(env -u THREADS_DB threads show smashcraft#10)
grep -q 'archive/feature/resume-10' <<<"$out"
restore='archive/feature/resume-10:refs/heads/feature/resume-10'
git -C "$HOME/code/smashcraft/main" push -q origin "$restore"
[ "$(git -C "$scratch/origin.git" rev-parse refs/heads/feature/resume-10)" = "$(git -C "$scratch/origin.git" rev-parse refs/tags/archive/feature/resume-10)" ]
printf 'PASS [integration] proxy relays exact caller liveness without PIDs, discovers local archives, and the printed restore works on the scratch remote\n'
printf 'threads resume: 4 passed, 0 failed\n'
