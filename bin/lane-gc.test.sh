#!/usr/bin/env bash
set -euo pipefail

target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lane-gc"
bun --no-install - "$target" <<'TS'
const { retirementVerdict } = await import(Bun.argv[2]);
let seed = 317;
const fail = (rule: string, trial: number, facts: unknown) => { throw new Error(`${rule} seed=${seed} trial=${trial} facts=${JSON.stringify(facts)}`); };
for (let trial = 0; trial < 64; trial++) {
  seed = Number((1103515245n * BigInt(seed) + 12345n) & 2147483647n);
  const minimumAge = [0, 2, 24, 72][seed % 4];
  const archiveAge = [-1, 24, 72][(seed >> 3) % 3];
  const age = [-1, 0, minimumAge - 1, minimumAge, minimumAge + 1, archiveAge - 1, archiveAge, archiveAge + 1, 100][trial % 9];
  const facts = { age, minimumAge, merged: Boolean((seed >> 7) & 1), equivalent: Boolean((seed >> 8) & 1), evilMerge: Boolean((seed >> 9) & 1), archiveAge, protections: (seed >> 10) & 3 };
  const actual = retirementVerdict(facts);
  const admissible = facts.protections === 0 && age >= minimumAge && (facts.merged || (facts.equivalent && !facts.evilMerge) || (archiveAge >= 0 && age >= archiveAge));
  if ((actual !== "keep") !== admissible) fail("retirement gates and completeness", trial, facts);
  if (actual === "retire" && !facts.merged) fail("retire proof", trial, facts);
  if (actual === "retire-equiv" && (!facts.equivalent || facts.evilMerge || facts.merged)) fail("force-delete proof", trial, facts);
  if (actual === "archive-then-delete" && (facts.merged || (facts.equivalent && !facts.evilMerge) || archiveAge < 0 || age < archiveAge)) fail("archive proof", trial, facts);
  if (!["keep", "retire", "retire-equiv", "archive-then-delete"].includes(actual)) fail("unknown verdict", trial, facts);
  for (let protection = 0; protection < 17; protection++) {
    const protectedFacts = { ...facts, protections: facts.protections | (1 << protection) };
    if (retirementVerdict(protectedFacts) !== "keep") fail(`protection=${protection} from=${actual}`, trial, protectedFacts);
  }
}
TS
printf 'lane-gc retirement property: PASS (seed 317; 64 facts; 17 protections)\n'

scratch=$(mktemp -d)
real_git=$(command -v git)
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/bin" "$scratch/code/north/main" "$scratch/code/north/worktrees" "$scratch/handoffs"; printf '#!/usr/bin/env bash\ncase "$1 $2" in "fleet list") cat "$FLEET_TEST_FILE";; "fleet send") [ ! -e "$LANE_GC_FIXTURE/ping-fault" ] || exit 1; printf "%%s\\n" "$*" >>"$FLEET_TEST_SENDS";; *) exit 1;; esac\n' >"$scratch/bin/north"
export LANE_GC_FIXTURE=$scratch LANE_GC_REAL_GIT=$real_git
export LANE_GC_STALE_FILE=$scratch/stale.json FLEET_TEST_FILE=$scratch/fleet.json FLEET_TEST_SENDS=$scratch/fleet-sends
export GIT_CONFIG_GLOBAL=$scratch/gitconfig GIT_CONFIG_NOSYSTEM=1 PYTHONDONTWRITEBYTECODE=1 LANE_GC_MIRRORS=$scratch/mirrors
export PATH=$scratch/bin:$PATH
printf '[{"name":"fixture-parent","conversation":"fixture-session","parent":null,"provider":"claude","status":"working","cwd":"%s"}]\n' "$scratch/code/north/main" >"$FLEET_TEST_FILE"
git config --global user.name fixture
git config --global user.email fixture@example.invalid
git init -q --bare "$scratch/origin.git"
main=$scratch/code/north/main
git -C "$main" init -q -b main
printf 'main\n' >"$main/readme"
git -C "$main" add readme
git -C "$main" commit -qm main
git -C "$main" remote add origin "$scratch/origin.git"
git -C "$main" push -q origin main
for lane in orphan fresh force-fresh queued running head-live author pending; do
  git -C "$main" checkout -q -b "$lane" main
  printf '%s\n' "$lane" >"$main/$lane"
  git -C "$main" add "$lane"
  git -C "$main" commit -qm "$lane"
  tip=$(git -C "$main" rev-parse HEAD)
  git -C "$main" branch "claude/land-$lane-${tip:0:12}" "$tip"
done
git -C "$main" checkout -q main
git -C "$main" worktree add -q "$scratch/code/north/worktrees/author" author
cat >"$scratch/bin/git" <<'WRAP'
#!/usr/bin/env bash
args=("$@")
if [[ " $* " == *" remote get-url origin "* ]]; then echo https://github.com/tompassarelli/north.git; exit 0; fi
if [[ " $* " == *" push "* ]]; then
  printf '%s\n' "$*" >>"$LANE_GC_FIXTURE/pushes"
  for arg in "$@"; do [[ "$arg" != :refs/heads/* ]] || exit 0; done
fi
exec "$LANE_GC_REAL_GIT" "${args[@]}"
WRAP
cat >"$scratch/bin/gh" <<'PY'
#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit
root = Path(os.environ['LANE_GC_FIXTURE'])
args = sys.argv[1:]
facts = json.loads((root / 'facts.json').read_text())
if (root / 'api-fault').exists():
    fault = (root / 'api-fault').read_text()
    if fault == 'error': sys.exit(1)
    if fault == 'refs-incomplete' and args[1] == 'graphql': print(json.dumps([{'data': {'repository': {'refs': {'nodes': facts['branches'], 'pageInfo': {'hasNextPage': True}}}}}])); sys.exit(0)
    if fault == 'runs-capped' and '/actions/runs?' in args[1]: print(json.dumps([{'workflow_runs': [], 'total_count': 1001}])); sys.exit(0)
    if fault == 'malformed' and 'activity?' in args[1]: print(json.dumps([[{'ref': 'refs/heads/bad'}]])); sys.exit(0)
if args[1] == 'graphql': print(json.dumps([{'data': {'repository': {'refs': {'nodes': facts['branches'], 'pageInfo': {'hasNextPage': False}}}}}]))
elif 'activity?' in args[1]:
    activity_type = parse_qs(urlsplit(args[1]).query).get('activity_type', [None])[0]
    rows = [row for row in facts['activity'] if activity_type is None or row['activity_type'] == activity_type]
    print(json.dumps([rows[:3], rows[3:]]))
elif '/pulls?' in args[1]: print('[[]]')
elif '/actions/runs?' in args[1]:
    status = args[1].split('status=')[1].split('&')[0]
    runs = facts['runs'].get(status, [])
    print(json.dumps([{'workflow_runs': runs, 'total_count': len(runs)}]))
else: sys.exit(1)
PY
printf '#!/usr/bin/env bash\ncat >/dev/null\necho '\''{"ok":true,"branches":{}}'\''\n' >"$scratch/bin/ssh"
printf '#!/usr/bin/env bash\nprintf "item\\tholder\\n"\n' >"$scratch/bin/threads"
cat >"$scratch/bin/supply-alert" <<'SHIM'
#!/usr/bin/env bash
[ ! -e "$LANE_GC_FIXTURE/ping-fault" ] || exit 1
printf '%s\n' "$*" >>"$LANE_GC_FIXTURE/alerts"
SHIM
chmod +x "$scratch/bin/"*
python3 - "$main" <<'PY'
import json, os, subprocess, sys
from pathlib import Path
root = Path(os.environ['LANE_GC_FIXTURE'])
rows = subprocess.check_output([os.environ['LANE_GC_REAL_GIT'], '-C', sys.argv[1], 'for-each-ref', '--format=%(refname:short) %(objectname)', 'refs/heads/claude/'], text=True).splitlines()
branches = [{'name': row.split()[0], 'target': {'oid': row.split()[1], 'committedDate': '2026-10-14T12:00:00Z', 'status': {'contexts': [{'context': 'autoland', 'state': 'PENDING'}]}}} for row in rows]
facts = {'branches': branches, 'activity': [{'ref': 'refs/heads/' + b['name'], 'activity_type': 'branch_creation' if '/land-fresh-' in b['name'] or '/land-force-fresh-' in b['name'] else 'push', 'timestamp': '2026-10-14T09:00:00Z' if '/land-fresh-' in b['name'] else '2026-10-14T06:00:00Z', 'before': '0' * 40, 'after': b['target']['oid']} for b in branches], 'runs': {}}
forced = next(b for b in branches if '/land-force-fresh-' in b['name'])
facts['activity'].append({'ref': 'refs/heads/' + forced['name'], 'activity_type': 'force_push', 'timestamp': '2026-10-14T10:30:00Z', 'before': forced['target']['oid'], 'after': forced['target']['oid']})
orphan = next(b for b in branches if '/land-orphan-' in b['name'])
facts['activity'].append({'ref': 'refs/heads/' + orphan['name'], 'activity_type': 'branch_deletion', 'timestamp': '2026-10-14T10:59:00Z', 'before': orphan['target']['oid'], 'after': '0' * 40})
for status, lane in [('queued', 'queued'), ('in_progress', 'running'), ('waiting', 'head-live')]:
    b = next(b for b in branches if '/land-' + lane + '-' in b['name'])
    facts['runs'][status] = [{'head_branch': b['name'] if lane != 'head-live' else 'train', 'head_sha': b['target']['oid']}]
(root / 'facts.json').write_text(json.dumps(facts))
PY
export LANE_GC_NOW=2026-10-14T11:00:00Z
run() { "$target" --root "$scratch/code" --repo north --state "$scratch/unlanded.txt" "$@"; }
run --dry-run >"$scratch/dry.log"
[ ! -e "$scratch/pushes" ] && [ ! -e "$scratch/alerts" ]
grep -q 'would notify fixture-parent' "$scratch/dry.log"
grep 'land-fresh-' "$scratch/dry.log" | grep -q 'LIVE'
grep 'land-force-fresh-' "$scratch/dry.log" | grep -q 'LIVE'
touch "$scratch/ping-fault"
run >"$scratch/ping-fault.log"
[ ! -e "$scratch/pushes" ] && [ ! -e "$scratch/alerts" ]
grep -q 'Fleet parent notification failed' "$scratch/ping-fault.log"
rm "$scratch/ping-fault"
run >"$scratch/ping.log"
[ ! -e "$scratch/pushes" ]
grep -q 'fleet send fixture-parent .*orphan landing' "$FLEET_TEST_SENDS"
export LANE_GC_NOW=2026-10-14T11:30:00Z
run >"$scratch/grace.log"
[ ! -e "$scratch/pushes" ]
grep -q 'owner ping grace' "$scratch/grace.log"
for fault in error malformed refs-incomplete runs-capped; do
  printf '%s' "$fault" >"$scratch/api-fault"
  export LANE_GC_NOW=2026-10-14T12:01:00Z
  run >"$scratch/$fault.log"
  [ ! -e "$scratch/pushes" ]
done
rm "$scratch/api-fault"
run >"$scratch/recovery-ping.log"
[ ! -e "$scratch/pushes" ]
grep -q 'notified fixture-parent' "$scratch/recovery-ping.log"
export LANE_GC_NOW=2026-10-14T13:00:00Z
run >"$scratch/recovery-grace.log"
[ ! -e "$scratch/pushes" ]
grep -q 'owner ping grace' "$scratch/recovery-grace.log"
grep 'land-fresh-' "$scratch/recovery-grace.log" | grep -q 'notified fixture-parent'
export LANE_GC_NOW=2026-10-14T13:01:00Z
run >"$scratch/archive.log"
! grep -q ":refs/heads/claude/land-fresh-" "$scratch/pushes"
export LANE_GC_NOW=2026-10-14T14:00:00Z
run >"$scratch/creation-archive.log"
python3 - "$scratch" <<'PY'
import json, subprocess, sys
from pathlib import Path
root = Path(sys.argv[1]); facts = json.loads((root / 'facts.json').read_text()); pushes = (root / 'pushes').read_text().splitlines()
for b in facts['branches']:
    name, tip = b['name'], b['target']['oid']
    deletions = [i for i, line in enumerate(pushes) if ':refs/heads/' + name in line]
    eligible = any('/land-' + lane + '-' in name for lane in ['orphan', 'pending', 'fresh'])
    assert bool(deletions) == eligible, (name, pushes)
    if eligible:
        ref = 'refs/archive/2026-10-14/' + name
        archived = [i for i, line in enumerate(pushes) if tip + ':' + ref in line]
        assert archived and archived[0] < deletions[0], (name, pushes)
        assert subprocess.check_output(['git', '--git-dir=' + str(root / 'mirrors' / 'north.git'), 'rev-parse', ref + '^{commit}'], text=True).strip() == tip, name
        assert subprocess.run(['git', '--git-dir=' + str(root / 'origin.git'), 'rev-parse', '-q', '--verify', 'refs/tags/archive/north/' + name + '/' + tip[:12]], capture_output=True).returncode != 0, 'archive tags stay off origin'
        assert '--force-with-lease=refs/heads/' + name + ':' + tip in pushes[deletions[0]]
print('lane-gc orphan landing journey: PASS (creation and force-push age, owner ping, grace, mirror archive recovery, live safety, unavailable evidence)')
PY
