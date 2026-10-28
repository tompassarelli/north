#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/threads-resume-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export HOME=$scratch/home THREADS_DB=$scratch/threads.db FLEET_FILE=$scratch/fleet.json
export NORTH_AGENT_ACTIVE=$scratch/activation.active NORTH_FLEET_FIXTURE=$FLEET_FILE NORTH_BIN=$scratch/bin/north PATH=$scratch/bin:$PATH BUILT_NORTH=${NORTH_PACE_BIN:-$repo/target/debug/north}
mkdir -p "$scratch/bin" "$HOME/code/smashcraft"
printf 'hook north-hook\n' >"$NORTH_AGENT_ACTIVE"
ln -s "$repo/bin/threads" "$scratch/bin/threads"
cat >"$scratch/bin/north" <<'NORTH'
#!/usr/bin/env bash
[[ "$*" == "fleet list --json" ]] || exec "$BUILT_NORTH" "$@"
cat "$NORTH_FLEET_FIXTURE"
NORTH
chmod +x "$scratch/bin/north"
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
fleet() { printf '[{"name":"holder","conversation":"fixture-session","task_ref":"smashcraft#10","provider":"codex","parent":null,"status":"%s"}]\n' "$1" >"$FLEET_FILE"; }
fleet working
threads claim smashcraft#10 --by codex:fixture-session --eta 5 >/dev/null
sqlite3 "$THREADS_DB" "UPDATE claims SET since = '2020-01-01T00:00:00Z'; UPDATE events SET ts = '2020-01-01T00:00:00Z'"
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tcodex:fixture-session\t' <<<"$out"
fleet exited
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tUNOWNED\t' <<<"$out"
fleet working
out=$(threads unowned)
grep -q $'smashcraft#10\tpriority:now\tcodex:fixture-session\t' <<<"$out"
printf 'PASS [integration] expired claims stay held for exact active holders and expire for dead or finished holders\n'
mkdir -p "$HOME/.config/threads" "$HOME/.local/state/threads" "$scratch/remote-home"
printf 'fixture-host\n' >"$HOME/.config/threads/remote"
cp "$THREADS_DB" "$scratch/remote.db"
export REMOTE_DB=$scratch/remote.db REMOTE_HOME=$scratch/remote-home SSH_TRACE=$scratch/ssh.trace REMOTE_FLEET=$scratch/remote-fleet.json
printf '[]\n' >"$REMOTE_FLEET"
cat >"$scratch/bin/ssh" <<'SSH'
#!/usr/bin/env bash
printf '%s\n' "${@: -1}" >>"$SSH_TRACE"
HOME=$REMOTE_HOME THREADS_DB=$REMOTE_DB NORTH_BIN=$NORTH_BIN NORTH_FLEET_FIXTURE=$REMOTE_FLEET exec bash -c "${@: -1}"
SSH
chmod +x "$scratch/bin/ssh"
out=$(env -u THREADS_DB threads unowned)
grep -q $'smashcraft#10\tpriority:now\tcodex:fixture-session\t' <<<"$out"
grep -q 'THREADS_ALIVE_OWNERS=' "$SSH_TRACE"
! grep -q 'pid' "$SSH_TRACE"
fleet exited
out=$(env -u THREADS_DB threads unowned)
grep -q $'smashcraft#10\tpriority:now\tUNOWNED\t' <<<"$out"
out=$(env -u THREADS_DB threads show smashcraft#10)
grep -q 'archive/feature/resume-10' <<<"$out"
restore='archive/feature/resume-10:refs/heads/feature/resume-10'
git -C "$HOME/code/smashcraft/main" push -q origin "$restore"
[ "$(git -C "$scratch/origin.git" rev-parse refs/heads/feature/resume-10)" = "$(git -C "$scratch/origin.git" rev-parse refs/tags/archive/feature/resume-10)" ]
printf 'PASS [integration] proxy relays exact caller liveness without PIDs, discovers local archives, and the printed restore works on the scratch remote\n'
printf 'threads resume: 2 passed, 0 failed\n'
