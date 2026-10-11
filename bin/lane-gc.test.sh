#!/usr/bin/env bash
set -euo pipefail

target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lane-gc"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/lane-gc-test.XXXXXX")"
sleeper=""
trap '[ -z "$sleeper" ] || kill "$sleeper" 2>/dev/null; rm -rf "${scratch:?}"' EXIT
root="$scratch/code"
main="$root/proj/main"
lanes="$root/proj/worktrees"
mkdir -p "$lanes" "$scratch/tools" "$scratch/handoffs"
export LANE_GC_STALE_FILE="$scratch/stale.json" LANE_GC_HANDOFF_DIR="$scratch/handoffs" AGENTS_ORG_FILE="$scratch/org.json"
export LANE_GC_TEST_ORIGIN="$scratch/origin.git"
export GIT_AUTHOR_DATE=2000-01-01T00:00:00Z GIT_COMMITTER_DATE=2000-01-01T00:00:00Z
printf '%s\n' '{"nodes":[{"id":"fixture-holder","role":"lead","domain":"proj","session":"alive-session","state":"active"}]}' > "$AGENTS_ORG_FILE"
cat > "$scratch/tools/gh" <<'PYGH'
#!/usr/bin/env python3
import json, os, subprocess, sys
args = sys.argv[1:]
if args[0] != "api": sys.exit(1)
endpoint = args[1]
origin = os.environ["LANE_GC_TEST_ORIGIN"]
if endpoint == "graphql" and any("object(oid:" in arg for arg in args):
    print(json.dumps({"data":{"repository":{"object":None}}}))
    sys.exit(0)
if endpoint == "graphql":
    rows = subprocess.check_output(["git", "-C", origin, "for-each-ref", "--format=%(refname:short) %(objectname)", "refs/heads"], text=True).splitlines()
    nodes = []
    for row in rows:
        branch, sha = row.split()
        date = subprocess.check_output(["git", "-C", origin, "show", "-s", "--format=%cI", sha], text=True).strip()
        status = {"contexts":[{"context":"autoland","state":"PENDING"}]} if branch in ("pending-autoland", "ancestor-pending-autoland") else None
        nodes.append({"name":branch,"target":{"oid":sha,"committedDate":date,"status":status}})
    result = [{"data":{"repository":{"refs":{"nodes":nodes,"pageInfo":{"hasNextPage":False,"endCursor":None}}}}}]
elif "/activity?" in endpoint:
    with open(os.environ["LANE_GC_TEST_PUSH_CALLS"], "a") as f: f.write(endpoint + "\n")
    if os.environ.get("LANE_GC_TEST_PUSH_FAIL"): sys.exit(1)
    result = [[{"ref":"refs/heads/pushed-old"}, {"ref":"refs/heads/ancestor-fresh"}]]
elif "/pulls?" in endpoint: result = [[{"head":{"ref":"ancestor-open-pr"}}]]
elif "/actions/runs?" in endpoint: result = [{"workflow_runs":[{"head_branch":"ancestor-actions"}]}]
elif endpoint.endswith("/comments"):
    with open(os.environ["LANE_GC_TEST_NOTES"], "a") as f: f.write(" ".join(args) + "\n")
    result = {}
else: sys.exit(1)
print(json.dumps(result))
PYGH
cat > "$scratch/tools/ssh" <<'SHSSH'
#!/bin/sh
printf '%s\n' '{"ok":true,"branches":{}}'
SHSSH
cat > "$scratch/tools/threads" <<'SHTHREADS'
#!/bin/sh
printf '%s\n' 'item owner clock eta flag' 'proj#99 alive-session 9999m 1m OVERDUE'
SHTHREADS
chmod +x "$scratch/tools/gh" "$scratch/tools/ssh" "$scratch/tools/threads"
export LANE_GC_REAL_GIT="$(command -v git)"
cat > "$scratch/tools/git" <<'PYGIT'
#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
if any(a in ("fetch", "pull", "push", "ls-remote") for a in args):
    args = ["-c", "url." + os.environ["LANE_GC_TEST_ORIGIN"] + ".insteadOf=https://github.com/tompassarelli/lane-gc-fixture.git", *args]
os.execv(os.environ["LANE_GC_REAL_GIT"], [os.environ["LANE_GC_REAL_GIT"], *args])
PYGIT
chmod +x "$scratch/tools/git"
export PATH="$scratch/tools:$PATH" LANE_GC_TEST_NOTES="$scratch/notes.txt" LANE_GC_TEST_PUSH_CALLS="$scratch/push-calls.txt"
git init -q --bare -b main "$scratch/origin.git"
git init -q -b main "$main"
git -C "$main" config user.name lane-gc-test
git -C "$main" config user.email lane-gc-test@example.invalid
git -C "$main" remote add origin https://github.com/tompassarelli/lane-gc-fixture.git
git -C "$main" commit -q --allow-empty -m base
git -C "$main" push -q -u origin main

lane() { git -C "$main" worktree add -q -b "$1" "$lanes/$1" main; }
commit() { printf '%s\n' "$2" >"$lanes/$1/$2"; git -C "$lanes/$1" add "$2"; git -C "$lanes/$1" commit -qm "$2 Refs proj#7"; }

lane merged; commit merged landed.txt
git -C "$lanes/merged" push -q origin merged:main merged:claude/merged
git -C "$main" pull -q --ff-only origin main
lane cherry; commit cherry picked.txt
git -C "$main" cherry-pick cherry >/dev/null
git -C "$main" push -q origin main
lane unique; commit unique work.txt
lane dirty; printf 'wip\n' >"$lanes/dirty/scratch.txt"
lane live; (cd "$lanes/live" && exec sleep 300) & sleeper=$!
git -C "$main" branch stale-merged main
git -C "$main" branch stale-unique unique

out="$("$target" --root "$root" --idle-hours 0 --state "$scratch/unlanded.txt")"
fail=0
check() { if eval "$2"; then :; else printf 'FAIL: %s\n%s\n' "$1" "$out" >&2; fail=1; fi; }
check "merged+clean worktree retired" '[ ! -e "$lanes/merged" ] && ! git -C "$main" rev-parse -q --verify refs/heads/merged >/dev/null'
check "patch-equivalent worktree retired" '[ ! -e "$lanes/cherry" ]'
check "unique commit kept" '[ -e "$lanes/unique" ] && git -C "$main" rev-parse -q --verify refs/heads/stale-unique >/dev/null'
check "dirty kept" '[ -e "$lanes/dirty/scratch.txt" ]'
check "live cwd kept" '[ -e "$lanes/live" ]'
check "merged branch without worktree deleted" '! git -C "$main" rev-parse -q --verify refs/heads/stale-merged >/dev/null'
check "merged origin claude branch deleted" '! git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/claude/merged >/dev/null'
check "unlanded report lists unique with ref" 'grep -P "\tunique\t\+1\t-\twork.txt Refs proj#7\tproj#7" "$scratch/unlanded.txt" >/dev/null'
check "unlanded report lists dirty" 'grep -P "/dirty\tdirty\t\+0\tdirty\t" "$scratch/unlanded.txt" >/dev/null'
git -C "$main" switch -q -c orphan main
printf 'orphan\n' > "$main/orphan.txt"
git -C "$main" add orphan.txt
old_date="$(date -u -d '25 hours ago' +%FT%TZ)"
GIT_AUTHOR_DATE="$old_date" GIT_COMMITTER_DATE="$old_date" git -C "$main" commit -qm 'orphan'
git -C "$main" push -q origin orphan
orphan_tip="$(git -C "$main" rev-parse HEAD)"
git -C "$main" switch -q main
for branch in pending-autoland handoff protected-holder fresh orphan-deferred pushed-old; do
  git -C "$main" switch -q -c "$branch" main
  printf '%s\n' "$branch" > "$main/$branch.txt"
  git -C "$main" add "$branch.txt"
  if [ "$branch" = fresh ]; then
    fresh_date="$(date -u +%FT%TZ)"
    GIT_AUTHOR_DATE="$fresh_date" GIT_COMMITTER_DATE="$fresh_date" git -C "$main" commit -qm fresh
  elif [ "$branch" = protected-holder ]; then
    git -C "$main" commit -qm 'working Refs proj#99'
  else
    git -C "$main" commit -qm "$branch"
  fi
  git -C "$main" push -q origin "$branch"
  git -C "$main" switch -q main
done
git -C "$main" branch ancestor-fresh main
GIT_AUTHOR_DATE="$(date -u +%FT%TZ)" GIT_COMMITTER_DATE="$(date -u +%FT%TZ)" git -C "$main" commit -q --allow-empty -m fresh-main
git -C "$main" branch -f ancestor-fresh main
git -C "$main" branch ancestor-pending-autoland main~1
git -C "$main" branch ancestor-open-pr main~2
git -C "$main" branch ancestor-actions main~3
git -C "$main" push -q origin main ancestor-fresh ancestor-pending-autoland ancestor-open-pr ancestor-actions
printf '%s\n' 'REVIEW fixture proj:handoff queued' 'REVIEW fixture proj:ancestor-fresh queued' > "$scratch/handoffs/fixture-peer.md"
out="$("$target" --remote --root "$root" --repo proj --state "$scratch/unlanded.txt")"
check "fresh commit with no claim is LIVE(a)" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/fresh >/dev/null && [[ "$out" == *"fresh: LIVE(a fresh commit)"* ]]'
check "lapsed claim with active holder is LIVE(d)" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/protected-holder >/dev/null && [[ "$out" == *"protected-holder: LIVE(d live holder"* ]]'
check "pending autoland kept" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/pending-autoland >/dev/null && [[ "$out" == *"pending-autoland: LIVE(b pending autoland)"* ]]'
check "handoff-listed branch kept" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/handoff >/dev/null && [[ "$out" == *"handoff: LIVE(handoff)"* ]]'
check "one pass archives multiple stale orphan branches" '! git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/orphan-deferred >/dev/null && git -C "$scratch/origin.git" rev-parse -q --verify refs/tags/archive/orphan-deferred >/dev/null'
check "ancestor ignores fresh commit push and handoff" '! git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/ancestor-fresh >/dev/null && [[ "$out" == *"ancestor-fresh: STALE"* ]]'
check "ancestor still keeps pending autoland" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/ancestor-pending-autoland >/dev/null && [[ "$out" == *"ancestor-pending-autoland: LIVE(b pending autoland)"* ]]'
check "ancestor still keeps open PR" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/ancestor-open-pr >/dev/null && [[ "$out" == *"ancestor-open-pr: LIVE(b open PR)"* ]]'
check "ancestor still keeps active Actions" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/ancestor-actions >/dev/null && [[ "$out" == *"ancestor-actions: LIVE(b active Actions)"* ]]'
check "old unique branch pushed under24h is kept" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/pushed-old >/dev/null && [[ "$out" == *"pushed-old: LIVE(a branch pushed under 24h)"* ]]'
check "25h orphan archived before deletion" '! git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/orphan >/dev/null && [ "$(git -C "$scratch/origin.git" rev-parse refs/tags/archive/orphan)" = "$orphan_tip" ]'
git -C "$main" push -q origin archive/orphan:refs/heads/orphan
check "archive restores identical tip" '[ "$(git -C "$scratch/origin.git" rev-parse refs/heads/orphan)" = "$orphan_tip" ]'
check "archive notice includes restore command" 'rg -F "git push origin archive/orphan:refs/heads/orphan" "$scratch/notices.md" >/dev/null'
fixture_at="$(date -u +%FT%TZ)"
flag_at="$(date -u -d "$fixture_at +24 hours" +%FT%TZ)"
retire_at="$(date -u -d "$fixture_at +73 hours" +%FT%TZ)"
LANE_GC_NOW="$flag_at" "$target" --root "$root" --repo proj --idle-hours 0 --state "$scratch/unlanded.txt" > "$scratch/flag.out"
check "unlanded lane flagged once at 24h" "python3 -c 'import json,sys; rows=json.load(open(sys.argv[1]))[\"repos\"][\"proj\"][\"local_stale\"]; assert any(r[\"branch\"]==\"unique\" for r in rows)' \"$scratch/stale.json\""
flagged="$(python3 -c 'import json,sys; print(next(r["flagged_at"] for r in json.load(open(sys.argv[1]))["repos"]["proj"]["local_stale"] if r["branch"]=="dirty"))' "$scratch/stale.json")"
out="$(LANE_GC_NOW="$retire_at" "$target" --root "$root" --repo proj --idle-hours 0 --state "$scratch/unlanded.txt")"
check "clean lane archived and retired at 72h" '[ ! -e "$lanes/unique" ] && git -C "$scratch/origin.git" rev-parse -q --verify refs/tags/archive/unique >/dev/null'
check "dirty lane at 72h stays flagged" "[ -e \"$lanes/dirty/scratch.txt\" ] && python3 -c 'import json,sys; r=next(r for r in json.load(open(sys.argv[1]))[\"repos\"][\"proj\"][\"local_stale\"] if r[\"branch\"]==\"dirty\"); assert r[\"dirty\"] and r[\"flagged_at\"]==sys.argv[2]' \"$scratch/stale.json\" \"$flagged\""
out="$(LANE_GC_TEST_PUSH_FAIL=1 "$target" --remote --root "$root" --repo proj --state "$scratch/unlanded.txt")"
check "failed push activity keeps every branch" 'git -C "$scratch/origin.git" rev-parse -q --verify refs/heads/orphan >/dev/null && [[ "$out" == *"orphan: LIVE(a push activity unavailable)"* ]]'
check "JSON notices are current run and markdown log persists" "python3 -c 'import json,sys; assert json.load(open(sys.argv[1]))[\"notices\"]==[]' \"$scratch/stale.json\" && rg -F 'git push origin archive/orphan:refs/heads/orphan' \"$scratch/notices.md\" >/dev/null"
check "one paginated push activity call per repo per pass" '[ "$(wc -l < "$scratch/push-calls.txt")" -eq 5 ]'
[ "$fail" -eq 0 ] && echo "lane-gc.test: ok (9 existing checks; 19 remote/local lifecycle checks)"
exit "$fail"
