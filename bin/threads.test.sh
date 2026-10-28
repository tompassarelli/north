#!/usr/bin/env bash
# Checks are eval strings, expanded when they run.
# shellcheck disable=SC2016
# threads against a scratch database and a fake gh: claims, handoffs, ready
# from GitHub's open issues and blocked-by, run ingest, parallel writers.
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
threads=$repo/bin/threads
scratch=$(mktemp -d "${TMPDIR:-/tmp}/threads-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export THREADS_DB=$scratch/threads.db GH_TRACE=$scratch/gh.trace PATH=$scratch/bin:$PATH
export AGENTS_ORCHESTRATION=$scratch/orchestration.toml NORTH_BIN=${NORTH_PACE_BIN:-$repo/target/debug/north}
: >"$AGENTS_ORCHESTRATION"

# smashcraft: #1 blocked by open #2, #2 open, #3 open, #4 blocked by closed #5.
mkdir "$scratch/bin"
cat >"$scratch/bin/gh" <<'GH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$GH_TRACE"
issue() { printf '{"number":%s,"title":"T%s","blockedBy":{"nodes":[%s]}}' "$1" "$1" "${2:-}"; }
blocker() { printf '{"number":%s,"state":"%s","repository":{"nameWithOwner":"tompassarelli/smashcraft"}}' "$1" "$2"; }
now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
iss() { printf '{"number":%s,"title":"T%s","labels":[%s],"body":"%s","comments":[%s]}' "$@"; }
lab() { printf '{"name":"priority:%s"}' "$1"; }
case "$*" in
  "issue list -R tompassarelli/smashcraft"*)
    printf '[%s]\n' "$(iss 15 15 "$(lab next)" '- [ ] a' ''),$(iss 16 16 '' '- [ ] a' ''),$(iss 17 17 "$(lab now)" '- [x] done' ''),$(iss 18 18 "$(lab later)" '- [ ] a' ''),$(iss 10 10 "$(lab now)" '- [ ] a\n- [ ] b' ''),$(iss 11 11 "$(lab now)" '- [ ] a' ''),$(iss 12 12 "$(lab now)" '- [ ] a' ''),$(iss 13 13 "$(lab now)" '- [ ] a' "{\"body\":\"Blocked: waits on Tom\",\"createdAt\":\"$now\"}"),$(iss 14 14 "$(lab now)" '- [ ] a' '')"
    exit ;;
  "issue list -R tompassarelli/wisp"*) printf '[%s]\n' "$(iss 20 20 "$(lab now)" '- [ ] a' '')"; exit ;;
  *refPrefix*name=smashcraft*)
    printf '{"data":{"repository":{"refs":{"nodes":[{"name":"main","target":{"committedDate":"%s"}},{"name":"claude/fix-12","target":{"committedDate":"%s"}},{"name":"claude/other","target":{"committedDate":"%s"}}]}}}}\n' "$now" "$now" "$now"
    exit ;;
  *name=smashcraft*number=3*) printf '{"data":{"repository":{"issue":{"number":3,"title":"T3","state":"OPEN","blockedBy":{"nodes":[]}}}}}\n'; exit ;;
  *refPrefix*) printf '{"data":{"repository":{"refs":{"nodes":[]}}}}\n'; exit ;;
  *compare*)
    printf '{"data":{"repository":{"defaultBranchRef":{"b0":{"commits":{"nodes":[]}},"b1":{"commits":{"nodes":[{"message":"Tidy\\n\\nUnlike #14, this leaves the ledger alone."}]}}}}}}\n'
    exit ;;
esac
case "$*" in
  *name=smashcraft*)
    nodes="$(issue 1 "$(blocker 2 OPEN)"),$(issue 2),$(issue 3),$(issue 4 "$(blocker 5 CLOSED)")" ;;
  *) nodes="" ;;
esac
printf '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[%s]}}}}\n' "$nodes"
GH
chmod +x "$scratch/bin/gh"

pass=0 fail=0 out=""
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n%s\n' "$1" "$out"; fi
}
t() { "$threads" "$@" 2>&1; }

t claim smashcraft#3 --by alice >/dev/null

t release smashcraft#3 --by alice --to bob >/dev/null

out=$(t claim nixos-config#9 --by alice >/dev/null; t claim tompassarelli/firn#9 --by bob); status=$?
check '[spec] a checkout alias and the GitHub name are the same issue' \
  '[ "$status" -eq 1 ]'
t release firn#9 --by alice >/dev/null

brief=$'Goal: x\nItem: tompassarelli/smashcraft#3. Follows: a111. Category: Tooling. Arm: escalate.\nETA 45 min.'
printf '%s\n' \
  "$(jq -cn '{id:"a111",ended:"2026-10-08T01:00:00Z",tier:"high",actual_min:30,eta_min:20,tokens:1000,outcome:"handoff",note:"~/h/a111.md",brief:"Item: smashcraft#3"}')" \
  "$(jq -cn --arg b "$brief" '{id:"a222",ended:"2026-10-08T02:00:00Z",tier:"xhigh",actual_min:50,eta_min:45,tokens:2000,outcome:"done",brief:$b}')" |
  "$threads" ingest 2>/dev/null
out=$(t show smashcraft#3; t summary)
check '[spec] runs take Item, Follows, Category and Arm from the brief; a new agent is a change of hands' \
  'grep -q "a222 xhigh follows a111  50m/45m ETA  done" <<<"$out" && grep -q "handoff  a111 -> a222" <<<"$out" && grep -q "note ~/h/a111.md" <<<"$out" && grep -qE "^tooling +xhigh +escalate +1 +1 +100%" <<<"$out"'

row() { jq -cn "$@"; }
row '{id:"a333",status:"running",model:"gpt-6.1-sol",effort:"medium",tier:"medium",started:"2026-10-08T03:00:00.511000Z",staffed:"2026-10-08T03:00:00.511000Z",eta_min:30,brief:"Item: smashcraft#4\nCategory: tooling\nSpec: measured\nScope: module\nSurface: shell",brief_text:"Item: smashcraft#4\nCategory: tooling\nSpec: measured\nScope: module\nSurface: shell",brief_origin:"codex.PostToolUse.spawn_agent",brief_path:"/durable/original.txt",brief_sha256:"exact-original-hash"}' |
  "$threads" ingest 2>/dev/null
check 'creation row retains original brief while running and remains unsettled' \
  '[ "$(sqlite3 "$THREADS_DB" "select outcome || '"'|'"' || category || '"'|'"' || coalesce(verify, '"'unknown'"') from runs where agent='"'a333'"'")" = "running|tooling|unknown" ] && ! t run-ids --settled | grep -qx a333'
check 'native creation records supplied model and effort' \
  '[ "$(sqlite3 "$THREADS_DB" "select model,effort from runs where agent='"'a333'"'")" = "gpt-6.1-sol|medium" ]'
out=$(t list)
check '[spec] a running worker whose brief has Item: holds the issue with no claim command' \
  'grep -qE "^smashcraft#4 +a333 .* 30m" <<<"$out"'
row '{id:"a333",status:"finished",started:"2026-10-08T03:00:00.512Z",ended:"2026-10-08T03:10:00Z",tier:"medium",actual_min:10,outcome:"running"}' | "$threads" ingest 2>/dev/null
check 'partial collector replacement retains observed creation model and effort' \
  '[ "$(sqlite3 "$THREADS_DB" "select model,effort from runs where agent='"'a333'"'")" = "gpt-6.1-sol|medium" ]'
row --arg s "$(date -u -d '45 minutes ago' +%Y-%m-%dT%H:%M:%S.000Z)" \
  '{id:"a444",status:"running",tier:"high",started:$s,eta_min:20,brief:"Item: smashcraft#2"}' | "$threads" ingest 2>/dev/null
out=$(t list)
check '[spec] the clock runs from when the worker started, and past 2x ETA it is overdue' \
  'grep -qE "^smashcraft#2 +a444 +4[56]m +20m +OVERDUE" <<<"$out"'
row '{id:"a333",status:"finished",started:"2026-10-08T03:00:00.512Z",model:"gpt-6-sol",effort:"high",ended:"2026-10-08T03:15:00Z",tier:"medium",actual_min:15,outcome:"running"}' | "$threads" ingest 2>/dev/null
check 'partial replacement accepts explicit observed model and effort' \
  '[ "$(sqlite3 "$THREADS_DB" "select model,effort from runs where agent='"'a333'"'")" = "gpt-6-sol|high" ]'
row '{id:"a333",status:"finished",started:"2026-10-08T03:00:00.512Z",ended:"2026-10-08T03:20:00Z",tier:"medium",actual_min:20,tokens:1,outcome:"done",staffed:"2026-10-08T02:59:59Z",brief:"Item: smashcraft#4\nVerify: native"}' |
  "$threads" ingest 2>/dev/null

check 'terminal replacement retains the latest observed model and effort' \
  '[ "$(sqlite3 "$THREADS_DB" "select model,effort from runs where agent='"'a333'"'")" = "gpt-6-sol|high" ]'

check 'completion promotes the creation row once and preserves the original brief facets' \
  '[ "$(sqlite3 "$THREADS_DB" "select outcome || '"'|'"' || category || '"'|'"' || coalesce(verify, '"'unknown'"') || '"'|'"' || brief_origin || '"'|'"' || brief_path || '"'|'"' || brief_sha256 from runs where agent='"'a333'"'")" = "done|tooling|unknown|codex.PostToolUse.spawn_agent|/durable/original.txt|exact-original-hash" ] && t run-ids --settled | grep -qx a333'

check 'completion retains exact native creation started and collector assignment staffed' \
  '[ "$(sqlite3 "$THREADS_DB" "select started,staffed from runs where agent='"'a333'"'")" = "2026-10-08T03:00:00.511000Z|2026-10-08T02:59:59Z" ]'

row '{id:"a333",backfill:true,status:"finished",ended:"2026-10-08T03:20:00Z",tier:"medium",actual_min:20,outcome:"done",brief:"Category: feature\nSpec: exact\nScope: cross-repo\nSurface: ts\nVerify: native"}' | "$threads" ingest 2>/dev/null
check 'later backfill preserves the creation metadata including an absent original facet' \
  '[ "$(sqlite3 "$THREADS_DB" "select category || '"'|'"' || spec || '"'|'"' || scope || '"'|'"' || surface || '"'|'"' || coalesce(verify, '"'unknown'"') from runs where agent='"'a333'"'")" = "tooling|measured|module|shell|unknown" ]'

check 'absent backfill retains the latest observed model and effort' \
  '[ "$(sqlite3 "$THREADS_DB" "select model,effort from runs where agent='"'a333'"'")" = "gpt-6-sol|high" ]'
row '{id:"a333",backfill:true,status:"finished",ended:"2026-10-08T03:20:00Z",tier:"medium",model:"gpt-6.1-sol",effort:"low",actual_min:20,outcome:"done"}' | "$threads" ingest 2>/dev/null
row '{id:"a333",backfill:true,status:"finished",ended:"2026-10-08T03:20:00Z",tier:"medium",actual_min:20,outcome:"done",model:null,effort:null}' | "$threads" ingest 2>/dev/null
check 'explicit backfill observations win and later null observations retain them' \
  '[ "$(sqlite3 "$THREADS_DB" "select model,effort from runs where agent='"'a333'"'")" = "gpt-6.1-sol|low" ]'
row '{id:"a333-unknown",status:"running",tier:"gpt-6.1-sol medium",brief_text:"Goal: unknown observations",brief_origin:"codex.PostToolUse.spawn_agent"}' | "$threads" ingest 2>/dev/null
row '{id:"a333-unknown",status:"finished",started:"2026-10-08T03:00:00.512Z",ended:"2026-10-08T03:20:00Z",tier:"gpt-6.1-sol medium",actual_min:20,outcome:"running"}' | "$threads" ingest 2>/dev/null
check 'unknown native creation started accepts the collector timestamp' \
  '[ "$(sqlite3 "$THREADS_DB" "select started from runs where agent='"'a333-unknown'"'")" = "2026-10-08T03:00:00.512Z" ]'
row '{id:"a333-unknown",backfill:true,status:"finished",ended:"2026-10-08T03:20:00Z",tier:"gpt-6.1-sol medium",actual_min:20,outcome:"running"}' | "$threads" ingest 2>/dev/null
check 'unknown creation observations remain null through replacement and backfill despite supplied tier' \
  '[ "$(sqlite3 "$THREADS_DB" "select model is null,effort is null from runs where agent='"'a333-unknown'"'")" = "1|1" ]'

row '{id:"a333-claude",status:"running",tier:"medium",started:"2026-10-08T03:00:00.511000Z",brief_text:"Goal: Claude creation",brief_origin:"claude.PostToolUse.Agent"}' | "$threads" ingest 2>/dev/null
row '{id:"a333-claude",status:"finished",tier:"medium",started:"2026-10-08T03:00:00.512Z",ended:"2026-10-08T03:20:00Z",actual_min:20,outcome:"running"}' | "$threads" ingest 2>/dev/null
check 'Claude lifecycle retains collector started behavior' \
  '[ "$(sqlite3 "$THREADS_DB" "select started from runs where agent='"'a333-claude'"'")" = "2026-10-08T03:00:00.512Z" ]'

# smashcraft#5 is closed (not in the open set): a medium run, then a high one after it.
{ row '{id:"b1",ended:"2026-10-08T04:00:00Z",tier:"medium",actual_min:10,tokens:3000,outcome:"not done",brief:"Item: smashcraft#5 Category: bug-known-cause"}'
  row '{id:"b2",ended:"2026-10-08T05:00:00Z",tier:"high",actual_min:10,tokens:5000,outcome:"done",brief:"Item: smashcraft#5 Follows: b1 Category: frobnicate"}'
} | "$threads" ingest 2>/dev/null
out=$(t summary)
check '[spec] a category outside the fixed list counts as unknown' 'grep -qE "^unknown +high +1 " <<<"$out"'
check '[spec] closure groups by the first run: issues, closed, runs and tokens per closed issue, escalations' \
  'grep -qE "^bug-known-cause +medium +1 +1 +2.0 +8k +100%" <<<"$out"'

{
  row '{id:"s0",ended:"2026-10-08T06:00:00Z",tier:"medium",actual_min:10,outcome:"split",brief:"Category: feature. Arm: split."}'
  row '{id:"s1",ended:"2026-10-08T06:20:00Z",tier:"medium",actual_min:15,outcome:"done",brief:"Follows: s0 Category: feature"}'
  row '{id:"s2",ended:"2026-10-08T06:30:00Z",tier:"medium",actual_min:20,outcome:"done",brief:"Follows: s0 Category: feature"}'
  row '{id:"s3",ended:"2026-10-08T06:40:00Z",tier:"medium",actual_min:25,outcome:"not done",brief:"Follows: s0 Category: feature"}'
} | "$threads" ingest 2>/dev/null
out=$(t summary)
check '[recorded] a Split run and its 3 parts show parts closure and an Arm: split row' \
  'grep -qE "^feature +1 +3 +67% +60 +40$" <<<"$out" && grep -qE "^feature +medium +split +3 +2 +66%" <<<"$out" && ! grep -qE "^feature +medium +- " <<<"$out"'

row '{id:"attribution",landed:1,landed_sha:"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",ended:"2026-10-08T06:40:00Z",tier:"medium",actual_min:25,outcome:"done",report:"Done: attributed work."}' | "$threads" ingest 2>/dev/null
row '{id:"attribution",backfill:true,landed:0,item:"wisp#71",category:"tooling",eta_min:25,ended:"2026-10-08T06:40:00Z",tier:"medium",actual_min:25,outcome:"not done",report:"Not done: later worker evidence."}' | "$threads" ingest 2>/dev/null
worker_attribution=$(sqlite3 "$THREADS_DB" "select item,category,eta_min,landed,landed_sha,outcome,report from runs where agent='attribution'")
row '{id:"nonworker-attribution",role:"parent",ended:"2026-10-08T06:40:00Z",tier:"medium",actual_min:25,outcome:"unclear",report:"Running: initial evidence.",category:"unknown"}' | "$threads" ingest 2>/dev/null
row '{id:"nonworker-attribution",role:"parent",ended:"2026-10-08T07:00:00Z",outcome:"done",report:"Done: current explicit evidence.",category:"tooling",compactions:2}' | "$threads" ingest 2>/dev/null
nonworker_updated=$(sqlite3 "$THREADS_DB" "select ended,category,role,compactions,outcome,report from runs where agent='nonworker-attribution'")
row '{id:"nonworker-attribution",role:"parent",ended:"2026-10-08T07:20:00Z"}' | "$threads" ingest 2>/dev/null
nonworker_preserved=$(sqlite3 "$THREADS_DB" "select ended,category,role,compactions,outcome,report from runs where agent='nonworker-attribution'")
out=$(printf '%s\n' "$worker_attribution" "$nonworker_updated" "$nonworker_preserved")
check '[recorded] backfill fills attribution and ETA while preserving worker evidence; nonworker reports follow supplied evidence and preserve absent fields' \
  '[ "$worker_attribution" = "wisp#71|tooling|25|1|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|done|Done: attributed work." ] && [ "$nonworker_updated" = "2026-10-08T07:00:00Z|tooling|parent|2|done|Done: current explicit evidence." ] && [ "$nonworker_preserved" = "2026-10-08T07:20:00Z|tooling|parent|2|done|Done: current explicit evidence." ]'

# Recommend: tier from first-assignment runs that closed without escalation and landed.
runs() {  # runs TIER CATEGORY-AND-FACETS OK FAIL
  local i
  for i in $(seq 1 "$3"); do row --arg t "$1" --arg b "$2" --arg id "$1-done-$EPOCHREALTIME-$i" '{id:$id,ended:"2026-10-09T00:00:00Z",tier:$t,actual_min:5,outcome:"done",landed:1,brief:$b}'; done
  for i in $(seq 1 "$4"); do row --arg t "$1" --arg b "$2" --arg id "$1-failed-$EPOCHREALTIME-$i" '{id:$id,ended:"2026-10-09T00:00:00Z",tier:$t,actual_min:5,outcome:"not done",landed:0,brief:$b}'; done
}
{ runs haiku 'Category: research. Spec: measured.' 5 0; runs medium 'Category: research. Spec: measured. Scope: one-file.' 2 0; } |
  "$threads" ingest 2>/dev/null
out=$(t recommend 'Category: research, Spec: measured, Scope: one-file'; t summary --group category,spec,scope)
check '[spec] recommend backs off to the finest cell with 5 runs and names it and n'   'grep -qx "tier claude:haiku  cell \[Category: research, Spec: measured\]  n 7" <<<"$out" && grep -qx "success threshold 80%  rule rank (one tier passes)" <<<"$out" && grep -qE "^research +measured +one-file +medium +- +2 " <<<"$out"'
row '{id:"eta-coarse",ended:"2026-10-09T00:00:00Z",tier:"medium",actual_min:20,outcome:"done",brief:"Category: research. Spec: measured. Scope: cross-module."}' | "$threads" ingest 2>/dev/null
out=$(python3 - "$threads" <<'PYETA'
import importlib.machinery, importlib.util, sys
loader = importlib.machinery.SourceFileLoader("threads", sys.argv[1])
spec = importlib.util.spec_from_loader("threads", loader)
mod = importlib.util.module_from_spec(spec)
loader.exec_module(mod)
runs = [dict(r) for r in mod.connect().execute("SELECT * FROM runs WHERE category = 'research'")]
for cell in ("Category: research, Spec: measured", "Category: research, Spec: measured, Scope: one-file"):
    eta, n, used = mod.recommend_eta(runs, cell, "medium")
    print(f"eta {eta}m (p75 of n={n}, cell {used}, tier medium)")
PYETA
)
check '[measurement] ETA p75 uses three fixture durations 5,5,20 and backs off the two-run scope cell' \
  '[ "$(grep -cx "eta 20m (p75 of n=3, cell Category: research, Spec: measured, tier medium)" <<<"$out")" -eq 2 ] && t recommend "Category: research, Spec: measured, Scope: one-file" | grep -q "^eta 5m (p75 of n=5, cell Category: research, Spec: measured, tier claude:haiku, uncalibrated: no eta_pred calibration)" && t recommend "Category: balance-tuning" | grep -q "^insufficient evidence  "'

python3 - "$threads" "$scratch/planner.db" "$scratch/planner-state" <<'PYBAND' >"$scratch/planner.out" 2>&1
import hashlib, importlib.machinery, importlib.util, json, math, os, random, re, sqlite3, subprocess, sys
from pathlib import Path
threads, db_path, state = sys.argv[1:]
os.environ["THREADS_DB"] = db_path
loader = importlib.machinery.SourceFileLoader("planner_threads", threads)
mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("planner_threads", loader))
loader.exec_module(mod)
db = mod.connect()
for i in range(1, 101):
    category = "mechanical" if i <= 90 else "feature"
    copies = 2 if i > 95 else 1
    for j in range(copies):
        db.execute("INSERT INTO runs(agent,item,follows,tier,category,started,ended,minutes,eta_min,outcome,report,scope,surface,compactions) "
                   "VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                   (f"b{i}-{j}", f"north#{i}", f"b{i}-0" if j else None, "high" if j else "medium", category,
                    f"2026-10-01T00:{j * 30:02d}:00Z", f"2026-10-01T0{1 if j else 0}:{0 if j else 10 if i <= 90 else 30:02d}:00Z",
                    10 if i <= 90 else 30, 10, "not done" if copies == 2 else "done",
                    "Not done: Stop: reasoning." if copies == 2 else "Done:", "module", "shell", 2 if i == 100 else 0))
for i in range(2):
    db.execute("INSERT INTO runs(agent,tier,category,ended,minutes,outcome) VALUES(?,?,?,?,?,?)",
               (f"no-item-{i}", "medium", "unknown", "2026-10-01T01:00:00Z", 10, "unclear"))
items, run_n, no_item = mod.planner_items(db)
db.close()
env = {**os.environ, "XDG_STATE_HOME": state, "PYTHONDONTWRITEBYTECODE": "1"}
before = hashlib.sha256(Path(db_path).read_bytes()).digest()
proc = subprocess.run([threads, "planner-band"], env=env, capture_output=True, text=True)
case_failures = []
try:
    assert proc.returncode == 0, proc.stderr
    assert hashlib.sha256(Path(db_path).read_bytes()).digest() == before, "readout changed the ledger"
    c = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    expected = c.execute("SELECT count(*),count(distinct item),sum(item is null) FROM runs").fetchone()
    observed = re.search(r"backfill runs=(\d+) items=(\d+)", proc.stdout)
    assert tuple(map(int, observed.groups())) == expected[:2], (proc.stdout, expected)
    assert f"no Item runs={expected[2]}\n" in proc.stdout and run_n == expected[0] and len(items) == expected[1]
    last = next(r for r in items if r["item"] == "north#100")
    assert (last["wall_min"], last["eta_min"], last["not_done"], last["stops"], last["restaffs"], last["escalations"],
            last["compactions"]) == (60, 20, 2, 2, 1, 1, 4), last
    assert math.isclose(last["difficulty"], math.log(4) + math.log(3) + 2 * math.log(2) + math.log(5)), last
except AssertionError as error:
    case_failures.append(str(error))
else:
    print("backfill ok")
try:
    high, maximum = mod.planner_cuts(items)
    assert math.isclose(high, math.log(4)) and maximum > high
    assert "measured high/xhigh/max=90.00/5.00/5.00% items=90/5/5 prior=90/8/2%" in proc.stdout, proc.stdout
    assert all((0 if r["difficulty"] < high else 1 if r["difficulty"] < maximum else 2) == r["benefit"] for r in items)
except AssertionError as error:
    case_failures.append(str(error))
else:
    print("cuts ok")
try:
    held = mod.planner_holdout(items)
    assert (held["training"], held["withheld"], held["top_decile"], held["off_rails"], held["hits"]) == (80, 20, 2, 2, 2), held
    assert math.isclose(held["p"], .01) and held["passed"], held
    shuffled = sorted(items, key=lambda r: r["item"])
    random.Random(54).shuffle(shuffled)
    withheld_ids = {r["item"] for r in shuffled[:20]}
    changed = [{**r, **({"difficulty": 1e6, "not_done": 900, "ratio": 100, "escalations": 90} if r["item"] in withheld_ids else {})}
               for r in items]
    assert mod.planner_holdout(changed) == held, "held-out outcomes leaked into rankings"
    for r in changed:
        r["off_rails"] = True
    failed = mod.planner_holdout(changed)
    assert (failed["hits"], failed["off_rails"]) == (2, 20) and not failed["passed"] and failed["p"] > .05, failed
    assert "held-out seed=54 training=80 items=20 top_decile=2 off_rails=2 hits=2 rate=100.00% chance=10% binomial_p=0.01 alpha=0.05 PASS" in proc.stdout
except AssertionError as error:
    case_failures.append(str(error))
else:
    print("holdout ok")
try:
    recommendation = subprocess.run([str(Path(threads).with_name("worker-ledger")), "--recommend", "--planner",
        "Category: feature, Scope: module, Surface: shell, Summed box ETA: 10m, Prior Not dones: 2"],
        env=env, capture_output=True, text=True)
    assert recommendation.returncode == 0, recommendation.stderr
    assert recommendation.stdout.startswith("planner xhigh  "), recommendation.stdout
    assert "prior Not dones: 2 given; history has no planning-time prior Not dones for 100 of 100 items, so matching ignores it for those\n" in recommendation.stdout, recommendation.stdout
    refused = subprocess.run([str(Path(threads).with_name("worker-ledger")), "--planner"],
                             env=env, capture_output=True, text=True)
    assert refused.returncode == 2 and refused.stderr.startswith("usage: worker-ledger"), refused
    assert f"cut points high=[0,{high:.6f}) xhigh=[{high:.6f},{maximum:.6f}) max=[{maximum:.6f},inf)" in recommendation.stdout, recommendation.stdout
    assert recommendation.stdout.splitlines()[-1] == "held-out: 2 of 2 off-rails items in the top decile (100.00%), p=0.01; PASS at alpha 0.05", recommendation.stdout
    assert hashlib.sha256(Path(db_path).read_bytes()).digest() == before, "planner recommendation changed the ledger"
except AssertionError as error:
    case_failures.append(str(error))
else:
    print("recommend planner ok")
assert not case_failures, case_failures
PYBAND
out=$(cat "$scratch/planner.out")
check '[recorded] planner backfill counts every run and distinct item without ledger writes; realized components include linked Follows runs' \
  'grep -qx "backfill ok" <<<"$out"'
check '[measurement] planner cut points separate known zero/one/multiple payoff signals and print their measured split beside the prior' \
  'grep -qx "cuts ok" <<<"$out"'
check '[recorded] planner fixed-seed held-out binomial check keeps withheld outcomes out of ranking and reports a failed chance-level fixture' \
  'grep -qx "holdout ok" <<<"$out"'

check '[recorded] planner recommendation uses measured bands and prints the held-out verdict through worker-ledger' \
  'grep -qx "recommend planner ok" <<<"$out"'

out=$(python3 - "$repo" "$scratch" <<'PYMODE'
import os, pathlib, subprocess, sys
repo, scratch = map(pathlib.Path, sys.argv[1:])
env = dict(os.environ, THREADS_DB=str(scratch / "recommend-mode.db"), AGENTS_ORCHESTRATION=str(scratch / "recommend-mode.toml"))
config = (repo / "harness/lib/fixtures/orchestration.toml").read_text()
facets = "Category: bug-known-cause, Spec: exact, Scope: module, Surface: shell, Verify: local-test"
def run(*args):
    return subprocess.check_output(args, env=env, text=True)
subprocess.run([str(repo / "bin/threads"), "ingest"], input="", env=env, text=True, capture_output=True, check=True)
import sqlite3
with sqlite3.connect(env["THREADS_DB"]) as db:
    for tier, minutes in (("haiku", 5), ("gpt-6.1-sol medium", 11)):
        for i in range(5):
            db.execute("INSERT INTO runs(agent,tier,category,ended,minutes,outcome,spec,scope,surface,verify) VALUES (?,?,?,?,?,?,?,?,?,?)",
                       (f"{tier}-{i}", tier, "bug-known-cause", "2026-10-09T00:00:00Z", minutes + i, "done", "exact", "module", "shell", "local-test"))
pathlib.Path(env["AGENTS_ORCHESTRATION"]).write_text(config)
split = run(str(repo / "bin/worker-ledger"), "--recommend", facets)
expected = """tier claude:haiku  cell [Category: bug-known-cause, Spec: exact, Scope: module, Verify: local-test, Surface: shell]  n 10
success threshold 80%  rule rank (uncalibrated: thin $ claude:haiku n=0, codex:sol-medium n=0 < 5)
  claude:haiku       100% of 5  $ n/a (priced n=0)
  codex:sol-medium   100% of 5  $ n/a (priced n=0)
eta 8m (p75 of n=5, cell Category: bug-known-cause, Spec: exact, Scope: module, Verify: local-test, Surface: shell, tier claude:haiku, uncalibrated: no eta_pred calibration)  $ n/a (uncalibrated: no usd calibration)
"""
assert split == expected, repr(split)
print("split unchanged")
pathlib.Path(env["AGENTS_ORCHESTRATION"]).write_text(config.replace('mode = "split"', 'mode = "claude-only"', 1))
assert run(str(repo / "bin/worker-ledger"), "--recommend", facets) == split
pathlib.Path(env["AGENTS_ORCHESTRATION"]).write_text(config.replace('mode = "split"', 'mode = "codex-only"', 1))
codex = run(str(repo / "bin/worker-ledger"), "--recommend", facets)
assert "claude:" not in codex and codex.startswith("tier codex:sol-medium "), codex
assert "eta 13m (actual_med of n=5, cell Category: bug-known-cause, Spec: exact, Scope: module, Verify: local-test, Surface: shell, tier codex:sol-medium)" in codex, codex
assert run(str(repo / "bin/worker-ledger"), "--recommend", "Category: balance-tuning").startswith("insufficient evidence  ")
print("codex-only median")
PYMODE
)
check '[recorded] worker-ledger recommend split stdout stays unchanged' 'grep -qx "split unchanged" <<<"$out"'
check '[recorded] worker-ledger recommend codex-only drops Claude and uses the chosen Codex cell median and n' 'grep -qx "codex-only median" <<<"$out"'

out=$(python3 - "$threads" <<'PYFALLBACK'
import importlib.machinery, importlib.util, random, sys
loader = importlib.machinery.SourceFileLoader("threads_fallback", sys.argv[1])
mod = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
loader.exec_module(mod)
rng = random.Random(131013)
asked = dict(category="feature", spec="measured", scope="module", surface="shell", verify="local-test")
for case in range(240):
    runs = []
    for tier in ("codex:sol-medium", "codex:sol-high", "codex:sol-xhigh"):
        n = rng.randrange(0, 21)
        wins = rng.randrange(max(1, (n * mod.SUCCESS_THRESHOLD + 99) // 100))
        for i in range(n):
            runs.append(dict(asked, tier=tier, ok=i < wins,
                             cost_usd=None if rng.random() < .35 else rng.randrange(1, 100) / 100))
    cell, _, pick, stats, rule = mod.recommend(runs, asked, {})
    eligible = {t: st for t, st in stats.items() if st["n"] >= mod.MIN_RUNS and st["usd"] is not None}
    expected = min(eligible, key=lambda t: (eligible[t]["usd"], mod.tier_rank(t))) if eligible else None
    assert pick == expected, (case, pick, expected, stats)
    if pick is not None:
        assert stats[pick]["n"] >= mod.MIN_RUNS and stats[pick]["usd_n"] >= mod.USD_MIN
    assert mod.recommend(runs, asked, {cell: "codex:sol-xhigh"})[2] == expected
print("sampled fallback property ok")
PYFALLBACK
)
check '[property] no passing tier selects lowest measured cost from sampled tiers or gives insufficient evidence' \
  'grep -qx "sampled fallback property ok" <<<"$out"'

python3 - "$threads" "$scratch/cal.db" <<'PYCAL' >"$scratch/cal.out" 2>&1
import importlib.machinery, importlib.util, json, random, subprocess, sys
from datetime import datetime, timedelta, timezone
threads, db_path = sys.argv[1:]
rng = random.Random(35)
end = datetime.now(timezone.utc)
rows = []
for i in range(720):
    progress = i / 720
    minutes = max(1, round(30 * 2 ** (-1.5 * progress) * rng.lognormvariate(0, .5)))
    ended = (end - timedelta(days=30 - i / 24, hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ")
    rows.append({"id": f"c{i}", "ended": ended, "tier": "medium", "actual_min": minutes, "eta_min": 30,
                 "outcome": "done" if rng.random() < .85 else "not done", "brief": "Category: mechanical. Spec: measured."})
env = {**__import__("os").environ, "THREADS_DB": db_path}
run = lambda *a, **k: subprocess.run([threads, *a], env=env, capture_output=True, text=True, **k).stdout
run("ingest", input="\n".join(json.dumps(r) for r in rows))
import sqlite3
con = sqlite3.connect(db_path)
con.execute("UPDATE runs SET cost_usd = minutes * 0.04")
con.commit()
__import__("os").environ["THREADS_DB"] = db_path
loader = importlib.machinery.SourceFileLoader("threads", threads)
mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("threads", loader))
loader.exec_module(mod)
runs = mod.calibration_runs(mod.connect())
now = mod.now()
pairs = mod.walk_forward_eta(runs, mod.since_start("7d", now), now)
raw = sum(r["minutes"] <= eta for r, eta in pairs) / len(pairs)
cal = sum(r["minutes"] <= mod.scaled(eta, next(x for x in mod.calibrate_eta_pred(runs, mod.since_start("7d", r["ended"]), r["ended"])
                                                 if x["cell"] == "all")["value"]) for r, eta in pairs) / len(pairs)
print(f"walk-forward coverage over n={len(pairs)}: uncalibrated {raw:.0%} calibrated {cal:.0%}")
assert abs(cal - .75) < abs(raw - .75), "calibrated coverage is not closer to 75%"
run("calibrate", "--since", "1h", "--write")
thin = run("recommend", "Category: mechanical, Spec: measured")
assert "uncalibrated: eta_pred all n=0 < 100" in thin, thin
doc = json.loads(run("calibrate", "--since", "7d", "--json", "--write"))
for kind in ("eta", "eta_pred", "usd"):
    row = next(r for r in doc["rows"] if r["kind"] == kind and r["cell"] == "all")
    assert row["n"] > 0 and all(row[k] is not None for k in ("p25", "median", "p75")), row
    print(f"{kind} n={row['n']} p25={row['p25']:.2f} median={row['median']:.2f} p75={row['p75']:.2f}")
good = run("recommend", "Category: mechanical, Spec: measured")
assert __import__("re").search(r" eta_pred claude n=\d+ ", good) and "uncalibrated" not in good and "$" in good, good
print(good.splitlines()[-1])
print("calibration ok")
PYCAL
out=$(cat "$scratch/cal.out")
check '[property] seeded drifting fixture: calibrated walk-forward ETA coverage is closer to 75% than uncalibrated, thin data falls back, calibrate --json gives n and quartiles per kind' \
  'grep -qx "calibration ok" <<<"$out"'

{ runs haiku 'Category: performance' 4 1; runs medium 'Category: performance' 4 1; } | "$threads" ingest 2>/dev/null
first=$(t recommend 'Category: performance' | head -1)
runs haiku 'Category: performance' 0 1 | "$threads" ingest 2>/dev/null
held=$(t recommend 'Category: performance' | head -1)
runs haiku 'Category: performance' 0 1 | "$threads" ingest 2>/dev/null
moved=$(t recommend 'Category: performance' | head -1)
out="$first / $held / $moved"
check '[spec] hysteresis: haiku at 80% wins by rank with no $, holds at 66% against medium 80%, and yields at 57%'   '[ "$out" = "tier claude:haiku  cell [Category: performance]  n 10 / tier claude:haiku  cell [Category: performance]  n 11 / tier claude:opus-medium  cell [Category: performance]  n 12" ]'

{ for i in 1 2 3; do row --arg id "n$i" '{id:$id,ended:"2026-10-09T00:00:00Z",tier:"haiku",actual_min:5,outcome:"done",landed:0,expects_landing:0,brief:"Category: native-check"}'; done
  row '{id:"n4",ended:"2026-10-09T00:00:00Z",tier:"haiku",actual_min:5,outcome:"done",landed:0,expects_landing:1,brief:"Category: native-check"}'
  row '{id:"n5",ended:"2026-10-09T00:00:00Z",tier:"haiku",actual_min:5,outcome:"done",landed:1,expects_landing:1,brief:"Category: native-check"}'
  row '{id:"n6",ended:"2026-10-09T01:00:00Z",tier:"medium",actual_min:5,outcome:"done",landed:1,brief:"Category: native-check Follows: n5"}'
} | "$threads" ingest 2>/dev/null
out=$(t recommend 'Category: native-check')
check '[spec] success is Done with no later Follows: run, and landed only when the run committed' \
  'grep -qx "  claude:haiku       60% of 5  \$ n/a (priced n=0)" <<<"$out"'
{ row '{id:"u1",ended:"2026-10-09T00:00:00Z",tier:"haiku",actual_min:5,outcome:"unclear",brief:"Category: native-check"}'
  row '{id:"u2",ended:"2026-10-09T00:00:00Z",tier:"haiku",actual_min:5,outcome:"unclear",brief:"Item: smashcraft#7 Category: native-check"}'
} | "$threads" ingest 2>/dev/null
out=$(t recommend 'Category: native-check'; t summary)
check '[recorded] an unclear run with no Item is unjudged and left out of success; a briefed unclear run is a failure' \
  'grep -qx "  claude:haiku       50% of 6  \$ n/a (priced n=0)" <<<"$out" && grep -qE "^native-check +haiku +- +6 +5 +83% +50%" <<<"$out"'

week() {  # week PREFIX TIER CATEGORY OK FAIL COST: history runs on 1 Oct, COST $ each (- for none)
  local i
  for i in $(seq 1 "$4"); do row --arg id "$1$i" --arg t "$2" --arg b "Category: $3" '{id:$id,started:"2026-10-01T00:00:00Z",ended:"2026-10-01T01:00:00Z",tier:$t,actual_min:5,outcome:"done",landed:1,brief:$b}'; done
  for i in $(seq 1 "$5"); do row --arg id "$1f$i" --arg t "$2" --arg b "Category: $3" '{id:$id,started:"2026-10-01T00:00:00Z",ended:"2026-10-01T01:00:00Z",tier:$t,actual_min:5,outcome:"not done",landed:0,brief:$b}'; done
}
{ week rm medium research 5 0; week rh haiku research 5 0
  week tm medium tooling 5 0; week ts 'gpt-6.1-sol medium' tooling 5 0
  week dm medium docs-policy 5 0; week dx 'gpt-6.1-sol xhigh' docs-policy 5 0
  row '{id:"d-research",started:"2026-10-05T00:00:00Z",ended:"2026-10-05T01:00:00Z",tier:"haiku",actual_min:5,outcome:"not done",landed:0,brief:"Category: research"}'
  row '{id:"d-tooling",started:"2026-10-05T00:00:00Z",ended:"2026-10-05T01:00:00Z",tier:"gpt-6.1-sol medium",actual_min:5,outcome:"done",landed:1,brief:"Category: tooling"}'
  row '{id:"d-docs",started:"2026-10-05T00:00:00Z",ended:"2026-10-05T01:00:00Z",tier:"medium",actual_min:5,outcome:"done",landed:1,brief:"Category: docs-policy"}'
} | THREADS_DB=$scratch/replay.db "$threads" ingest 2>/dev/null
sqlite3 "$scratch/replay.db" "update runs set cost_usd = case substr(agent, 1, 2) when 'rh' then 3.0 when 'rm' then 1.0
  when 'tm' then 2.0 when 'ts' then 0.5 when 'dm' then 2.0 end where substr(agent, 1, 2) in ('rh','rm','tm','ts','dm')"
out=$(THREADS_DB=$scratch/replay.db t replay-staffing --since 2026-10-04)
check '[golden] replay-staffing: $ per success moves research off haiku and tooling onto SOL; thin $ falls back to rank for docs' \
  '[ "$out" = "$(cat <<'"'GOLDEN'"'
replay-staffing since 2026-10-04: 3 first-assignment runs, picks from strictly earlier runs
[Category: docs-policy]  claude:opus-medium -> codex:sol-xhigh  runs 1  rule rank (uncalibrated: thin $ codex:sol-xhigh n=0 < 5)
  codex:sol-xhigh 100% of 5, $ n/a (priced n=0)
  claude:opus-medium 100% of 5, $2.00/success (priced n=5)
  actual: new pick 0 (0 ok), old pick 1 (1 ok), other 0
[Category: research]  claude:haiku -> claude:opus-medium  runs 1  rule $ per success
  claude:opus-medium 100% of 5, $1.00/success (priced n=5)
  claude:haiku 100% of 5, $3.00/success (priced n=5)
  actual: new pick 0 (0 ok), old pick 1 (0 ok), other 0
[Category: tooling]  claude:opus-medium -> codex:sol-medium  runs 1  rule $ per success
  codex:sol-medium 100% of 5, $0.50/success (priced n=5)
  claude:opus-medium 100% of 5, $2.00/success (priced n=5)
  actual: new pick 1 (1 ok), old pick 0 (0 ok), other 0
changed 3 of 3 decisions in 3 cells; actual tier was the new pick 1 (1 ok), the old pick 2 (1 ok), another 0
GOLDEN
)" ]'

t claim smashcraft#11 --by alice --eta 30 >/dev/null
t block wisp#20 waits on smashcraft#13 >/dev/null
out=$(t unowned)
check '[spec] unowned: priority:now first, then unlabeled, next, later; owner from a claim or a branch name, blocked from a comment or a block record' \
  '[ "$(cut -f1-4 <<<"$out")" = "$(printf "%s\n" "smashcraft#10	priority:now	UNOWNED	2" "smashcraft#11	priority:now	alice	1" "smashcraft#12	priority:now	branch:claude/fix-12	1" "smashcraft#13	priority:now	blocked:waits on Tom	1" "smashcraft#14	priority:now	UNOWNED	1" "wisp#20	priority:now	blocked:waits on smashcraft#13	1" "smashcraft#16	-	UNOWNED	1" "smashcraft#15	priority:next	UNOWNED	1" "smashcraft#18	priority:later	UNOWNED	1" "priority:now unowned=2 blocked=2 owned=2")" ]'


python3 - "$threads" <<'PY' >"$scratch/prop.out" 2>&1
import importlib.machinery, importlib.util, random, string, sys
loader = importlib.machinery.SourceFileLoader("threads", sys.argv[1])
spec = importlib.util.spec_from_loader("threads", loader)
mod = importlib.util.module_from_spec(spec)
loader.exec_module(mod)
rng = random.Random(7)
db = mod.connect()
word = lambda: "".join(rng.choice(string.ascii_lowercase) for _ in range(rng.randint(3, 9)))
items, refused = [], 0
for i in range(1000):
    ns = rng.choice(mod.NATIVE)
    number = 1000 + i
    title = " ".join(word() for _ in range(rng.randint(3, 6)))
    db.execute("INSERT INTO items(ns, number, title, body, created) VALUES (?,?,?,?,?)", (ns, number, title, "", mod.now()))
    items.append((mod.ref(ns, number), title))
    ref = mod.ref(ns, number)
    args = rng.choice([["issue", "comment", "1", "--body", f"see {ref}"], ["issue", "create", "--title", f"{ref} {title}"],
                       ["api", "graphql", "-f", f"query={ref}"], ["pr", "edit", "2", f"--body={title} ({ref})"]])
    try:
        mod.gh(*args)
    except mod.Refused:
        refused += 1
db.commit()
for ref, _ in rng.sample(items, 50):
    assert mod.main(["show", ref]) == 0
    assert mod.main(["need", ref, "smashcraft#3"]) == 0
assert mod.main(["ready"]) == 0
trace = open(mod.os.environ["GH_TRACE"]).read()
leaked = [r for r, t in items if r in trace or t in trace]
print(f"refused={refused} leaked={len(leaked)}")
PY
out=$(tail -1 "$scratch/prop.out")
check '[property] 1,000 generated native refs and titles: gh refuses every argv naming one and none appears in the logged gh argv' \
  '[ "$out" = "refused=1000 leaked=0" ]'

# 40 processes, each making 25 claims through NorthThreads' own entry point.
for w in $(seq 1 40); do
  python3 - "$threads" "$w" <<'PY' >/dev/null 2>>"$scratch/writers.err" &
import importlib.machinery, importlib.util, sys
loader = importlib.machinery.SourceFileLoader("threads", sys.argv[1])
spec = importlib.util.spec_from_loader("threads", loader)
mod = importlib.util.module_from_spec(spec)
loader.exec_module(mod)
for n in range(25):
    assert mod.main(["claim", f"wisp#{int(sys.argv[2]) * 100 + n}", "--by", f"w{sys.argv[2]}"]) == 0
PY
done
wait
kept=$(sqlite3 "$THREADS_DB" "select count(*) from claims where item like 'wisp#%'"); out="$kept of 1000 kept; writer errors:"$'\n'"$(sort "$scratch/writers.err" 2>/dev/null | uniq -c | head -5)"
check '[invariant] 40 writers x 25 parallel writes keep all 1000' '[ "$kept" = 1000 ]'

# The remote ledger over a fake ssh: ingest spools while it is unreachable, then flushes in order.
mkdir -p "$scratch/home/.config/threads" "$scratch/remote"
echo nexus >"$scratch/home/.config/threads/remote"
ln -s "$threads" "$scratch/remote/threads"
ln -s "$NORTH_BIN" "$scratch/remote/north"
cat >"$scratch/bin/ssh" <<'SSH'
#!/usr/bin/env bash
[ -e "$SSH_DOWN" ] && exit 255
if [ -f "$HOME/.config/threads/ssh-config" ]; then
  [ "${1:-}" = -F ] && [ "${2:-}" = "$HOME/.config/threads/ssh-config" ] || exit 255
fi
printf '%s\n' "${@: -1}" >>"$SSH_TRACE"
THREADS_DB=$REMOTE_DB PATH=$REMOTE_BIN:$PATH exec bash -c "${@: -1}"
SSH
chmod +x "$scratch/bin/ssh"
proxied() { env -u THREADS_DB HOME="$scratch/home" SSH_DOWN="$scratch/down" SSH_TRACE="$scratch/ssh.trace" REMOTE_DB="$scratch/remote.db" REMOTE_BIN="$scratch/remote" "$threads" "$@" 2>&1; }
touch "$scratch/down"
for n in 1 2; do
  jq -nc --arg id "s$n" '{id:$id,ended:"2026-10-09T01:00:00Z",tier:"medium",actual_min:5,outcome:"done",brief:"Category: tooling"}' | proxied ingest >/dev/null
done
out=$(proxied claim wisp#9 --by alice); status=$?
spooled=$(ls "$scratch/home/.local/state/threads/spool" | wc -l)
rm "$scratch/down"
proxied claim wisp#7 --by bob >/dev/null
: >"$scratch/ssh.trace"
proxied calibrate --write --goal-row '{"kind":"goal_eta","cell":"wisp","n":1,"value":5,"source":"fixture"}' >/dev/null; goalstatus=$?
goalsnapshots=$(grep -c '^north threads snapshot-pages$' "$scratch/ssh.trace")
proxied calibrate --write >/dev/null; calibratestatus=$?
writesnapshots=$(grep -c '^north threads snapshot-pages$' "$scratch/ssh.trace")
touch "$scratch/down"
out2=$(proxied list); status2=$?
check '[integration] remote down: ingest spools and exits 0, a claim fails in one line, a read serves the last snapshot marked STALE; back up: the spool flushes in order; goal rows avoid refresh while other calibration writes refresh' \
  '[ "$goalstatus" -eq 0 ] && [ "$calibratestatus" -eq 0 ] && [ "$goalsnapshots" -eq 0 ] && [ "$writesnapshots" -eq 1 ] && [ "$status" -eq 255 ] && [ "$spooled" -eq 2 ] && [ -z "$(ls "$scratch/home/.local/state/threads/spool")" ] && [ "$(sqlite3 "$scratch/remote.db" "select group_concat(agent) from (select agent from runs order by rowid)")" = s1,s2 ] && [ "$status2" -eq 0 ] && grep -q "^STALE: ledger snapshot from .*; nexus unreachable" <<<"$out2" && grep -qE "^wisp#7 +bob" <<<"$out2"'

rm "$scratch/down"
printf 'Host nexus\n' >"$scratch/home/.config/threads/ssh-config"
proxied claim wisp#8 --by configured-namespace-transport >"$scratch/configured-write.out"; configured_write=$?
proxied calibrate --write >"$scratch/configured-refresh.out"; configured_refresh=$?
configured_claim=$(sqlite3 "$scratch/remote.db" "select count(*) from claims where item='wisp#8' and owner='configured-namespace-transport'")
check '[integration] configured SSH namespace route preserves canonical writes and snapshot refresh' \
  '[ "$configured_write" -eq 0 ] && [ "$configured_refresh" -eq 0 ] && [ "$configured_claim" -eq 1 ]'
rm "$scratch/home/.config/threads/ssh-config"

mux_argv=$(XDG_RUNTIME_DIR=/run/user/4242 PYTHONDONTWRITEBYTECODE=1 python3 -I -c 'import sys; sys.path.insert(0, sys.argv[1]); from ssh_transport import ssh_argv; print("\n".join(ssh_argv("-o", "BatchMode=yes", "nexus", "true")))' "$repo/harness/lib")
check '[unit] ssh_argv carries ControlMaster=auto and a ControlPath under XDG_RUNTIME_DIR, ahead of the caller options and destination' \
  'grep -qx ControlMaster=auto <<<"$mux_argv" && grep -qx "ControlPath=/run/user/4242/north-ssh-%C" <<<"$mux_argv" && [ "$(tail -n 3 <<<"$mux_argv" | tr "\n" " ")" = "BatchMode=yes nexus true " ]'

python3 -I - "$repo" "$scratch/stalled-ssh" <<'PYSTALL' >"$scratch/stalled-ssh.out" 2>&1
import importlib.machinery, importlib.util, io, os, subprocess, sys, time
from pathlib import Path
from unittest.mock import patch
repo, root = map(Path, sys.argv[1:])
root.mkdir()
fake = root / "ssh"
fake.write_text('#!/bin/sh\nexec sleep 60\n')
fake.chmod(0o755)
os.environ["PATH"] = str(root) + os.pathsep + os.environ["PATH"]
def load(name):
    loader = importlib.machinery.SourceFileLoader(name, str(repo / "bin" / name))
    mod = importlib.util.module_from_spec(importlib.util.spec_from_loader(name, loader))
    loader.exec_module(mod)
    return mod
threads = load("threads")
laptop = load("laptop")
nexus = load("nexus-run")
threads.SPOOL = str(root / "spool")
threads.SNAPSHOT = str(root / "snapshot")
laptop.REMOTE = root / "remote"
laptop.REMOTE.write_text("nexus")
real_run = subprocess.run
calls = []
def bounded(argv, **kwargs):
    if argv[0] == "ssh" or argv[1:3] == ["threads", "snapshot-pull"]:
        bound = kwargs.get("timeout")
        calls.append(bound)
        assert bound is not None and 0 < bound <= 90, f"unbounded SSH: {argv}"
        kwargs["timeout"] = min(bound, 0.1)
    return real_run(argv, **kwargs)
with patch.object(subprocess, "run", bounded):
    started = time.monotonic()
    for args in (["claim", "wisp#99", "--by", "stall"], ["job-run", "stall", "--started", "2026-10-15T00:00:00Z"], ["ingest"]):
        with patch.object(sys, "stdin", io.TextIOWrapper(io.BytesIO(b'{}\n'))):
            before = len(calls)
            assert threads.proxy("nexus", args) == 124, args
            assert len(calls) == before + 1, "timed-out write was replayed"
        assert not Path(threads.SPOOL).exists(), "timed-out write was queued for replay"
    snapshot = root / "snapshot"
    snapshot.write_bytes(b"keep existing snapshot"); os.utime(snapshot, (0, 0))
    threads.refresh_snapshot("nexus", force=True, snapshot=str(snapshot))
    assert snapshot.read_bytes() == b"keep existing snapshot"; threads.refresh_snapshot("nexus", snapshot=str(snapshot))
    try:
        laptop.forward(["close"])
    except SystemExit as error:
        assert error.code == 124
    else:
        raise AssertionError("laptop timeout succeeded")
    with patch.object(sys, "argv", ["nexus-run", "--source", str(repo), "--cwd", ".", "--", "true"]):
        try:
            nexus.main()
        except subprocess.TimeoutExpired:
            pass
        else:
            raise AssertionError("nexus timeout succeeded")
    assert calls == [90] * 7, calls
    assert time.monotonic() - started < 10
print("stalled SSH waits fail within their bounds and writes are never replayed")
PYSTALL
stall_status=$?
check '[integration] stalled SSH fails within its bound without replaying writes' '[ "$stall_status" -eq 0 ] || { cat "$scratch/stalled-ssh.out"; false; }'

python3 -I - "$repo/harness/lib" <<'PYEXPLICIT' >"$scratch/explicit-ssh.out" 2>&1
import os, sys
sys.path.insert(0, sys.argv[1])
from ssh_transport import ssh_argv
os.environ["XDG_RUNTIME_DIR"] = "/run/user/4242"
for explicit in (["-o", "ControlMaster=no"], ["-oControlMaster=no"], ["-o", "ControlPath=none"], ["-oControlPath=none"], ["-S", "none"], ["-Snone"]):
    argv = ssh_argv(*explicit, "nexus", "true")
    assert argv[-len(explicit) - 2:] == [*explicit, "nexus", "true"]
    assert not any("ControlMaster=auto" == arg or arg.startswith("ControlPath=/run/") or arg == "ControlPersist=10m" for arg in argv), argv
for name, value in (("ControlMaster", "no"), ("ControlPath", "none"), ("ControlPersist", "no")):
    for separator in ("=", " ", "\t"):
        for explicit in (["-o", name + separator + value], ["-o" + name.swapcase() + separator + value]):
            argv = ssh_argv(*explicit, "nexus", "true")
            assert argv[-len(explicit) - 2:] == [*explicit, "nexus", "true"]
            assert not any("ControlMaster=auto" == arg or arg.startswith("ControlPath=/run/") or arg == "ControlPersist=10m" for arg in argv), argv
print("explicit multiplexing choices suppress defaults")
PYEXPLICIT
explicit_status=$?
check '[unit] explicit SSH multiplexing choices suppress defaults' '[ "$explicit_status" -eq 0 ] || { cat "$scratch/explicit-ssh.out"; false; }'

python3 -I - "$repo/harness/lib" <<'PYSOCKET' >"$scratch/socket-ssh.out" 2>&1
import os, sys
sys.path.insert(0, sys.argv[1])
from ssh_transport import ssh_argv
for runtime, reuse in (("/" + "a" * 74, False), ("/" + "é" * 20, False), ("/" + "a" * 35, True), ("/" + "a" * 36, False), ("/run/private dir", False), ("/run/%q", False)):
    os.environ["XDG_RUNTIME_DIR"] = runtime
    argv = ssh_argv("nexus", "true")
    assert ("ControlMaster=auto" in argv) == reuse, argv
    if not reuse:
        assert not any(arg.startswith(("ControlMaster=", "ControlPath=", "ControlPersist=", "ServerAliveInterval=", "ServerAliveCountMax=")) for arg in argv), argv
print("expanded socket paths reserve 20 bytes within the Unix socket limit")
PYSOCKET
socket_status=$?
check '[unit] SSH socket paths leave room for the OpenSSH suffix' '[ "$socket_status" -eq 0 ] || { cat "$scratch/socket-ssh.out"; false; }'

python3 - "$threads" "$scratch/routing" <<'PYROUTING' >"$scratch/routing.out" 2>&1
import hashlib, importlib.machinery, importlib.util, os, re, sqlite3, subprocess, sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
threads, scratch = sys.argv[1:]
root = Path(scratch)
root.mkdir()
db_path = root / "snapshot.db"
os.environ["THREADS_DB"] = str(db_path)
loader = importlib.machinery.SourceFileLoader("routing_threads", threads)
mod = importlib.util.module_from_spec(importlib.util.spec_from_loader("routing_threads", loader))
loader.exec_module(mod)
db = mod.connect()
operator = root / "operator"
operator.mkdir()
(operator / "costs.toml").write_text('[[service]]\nid="codex"\nkind="ai-plan"\nusd_per_week=50.0\n')
bounds = [("baseline", "2026-10-05T12:27:27Z", "2026-10-12T12:27:27Z", "2026-10-10T00:00:00Z"),
          ("codex-only", "2026-10-12T12:27:27Z", "2026-10-15T02:00Z", "2026-10-13T00:00:00Z"),
          ("split-after", "2026-10-15T02:00Z", "2026-10-15T16:00Z", "2026-10-15T02:00:00Z")]
def stamp(base, minutes):
    return (datetime.fromisoformat(base.replace("Z", "+00:00")) + timedelta(minutes=minutes)).isoformat().replace("+00:00", "Z")
def insert(agent, item, category, begin, end, landed, sha=None, outcome="done", follows=None):
    db.execute("INSERT INTO runs(agent,item,category,tier,account,staffed,started,ended,landed_at,landed,landed_sha,minutes,outcome,follows) "
               "VALUES(?,?,?,'gpt-6.1-sol medium','codex',?,?,?,?,?,?,?,?,?)",
               (agent, item, category, begin, begin, end, end if landed else None, landed, sha, 10, outcome, follows))
    db.execute("INSERT INTO run_costs VALUES('codex',?,?,?,0.5)", (begin, end, agent))
for arm_no, (arm, start, end, base) in enumerate(bounds):
    for i in range(7):
        item = f"north#{100 + arm_no * 10 + i}"
        agent = f"{arm}-{i}"
        sha = f"{arm_no * 10 + i + 1:040x}"
        category = "feature" if i < 5 else "tooling"
        insert(agent, item, category, base, stamp(base, 60 + i), 1, sha, follows=f"{arm}-retry" if i == 0 else None)
    insert(f"{arm}-retry", f"north#{100 + arm_no * 10}", "feature", base, stamp(base, 20), 0, outcome="not done")
    insert(f"{arm}-duplicate", f"north#{101 + arm_no * 10}", "feature", base, stamp(base, 62), 1, f"{arm_no * 10 + 2:040x}")
    insert(f"{arm}-future", f"north#{102 + arm_no * 10}", "feature", stamp(base, 1), stamp(base, 120), 0, outcome="not done")
    insert(f"{arm}-itemless", None, "unknown", base, stamp(base, 30), 1)
    insert(f"{arm}-unlanded", f"north#{109 + arm_no * 10}", "feature", base, stamp(base, 30), 0)
insert("outside", "north#999", "feature", "2026-10-15T16:00:00Z", "2026-10-15T16:00:00Z", 1, "f" * 40)
db.commit()
db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
db.close()
before = hashlib.sha256(db_path.read_bytes()).digest()
proc = subprocess.run([threads, "routing-compare"], capture_output=True, text=True,
                      env={**os.environ, "AGENTS_OPERATOR_DIR": str(operator), "PYTHONDONTWRITEBYTECODE": "1"})
assert proc.returncode == 0, proc.stderr
assert hashlib.sha256(db_path.read_bytes()).digest() == before, "readout wrote to snapshot"
c = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
for arm, start, end, _ in bounds:
    lines = [re.split(r"\s{2,}", line.strip()) for line in proc.stdout.splitlines() if line.startswith(arm + " ")]
    overall = next(row for row in lines if row[1] == "overall" and len(row) == 9)
    direct = c.execute("SELECT count(DISTINCT item) FROM runs WHERE role='worker' AND landed=1 AND item IS NOT NULL "
                       "AND julianday(coalesce(landed_at,ended)) >= julianday(?) "
                       "AND julianday(coalesce(landed_at,ended)) < julianday(?)", (start, end)).fetchone()[0]
    assert int(overall[2]) == direct == 7, (arm, overall, direct)
    feature = next(row for row in lines if row[1] == "feature" and len(row) == 9)
    assert feature[2] == "5" and feature[5] == "80.00% [37.55,96.38] (4/5)", feature
    assert feature[7:] == ["n/a (n=0)", "0/5"], feature
    tooling = next(row for row in lines if row[1] == "tooling" and len(row) == 9)
    assert tooling[2] == "2" and tooling[5] == "n/a (n=2)", tooling
    points = next(row for row in lines if row[1:3] == ["feature", "codex"])
    assert points[3:5] == ["5", "5"] and points[5].startswith("1.40 ["), points
    assert f"no Item runs {arm} n=1 landed=1" in proc.stdout, proc.stdout
c.close()
db = sqlite3.connect(db_path)
for arm_no, (arm, start, end, base) in enumerate(bounds):
    insert(f"{arm}-unclear-failed", f"north#{100 + arm_no * 10}", "feature", base, stamp(base, 10), 0, outcome="unclear")
    insert(f"{arm}-unclear-escalated", f"north#{105 + arm_no * 10}", "tooling", base, stamp(base, 10), 0, outcome="unclear")
    insert(f"{arm}-escalated", f"north#{105 + arm_no * 10}", "tooling", base, stamp(base, 20), 0, outcome="escalated")
    insert(f"{arm}-unclear", f"north#{106 + arm_no * 10}", "tooling", base, stamp(base, 10), 0, outcome="unclear")
db.commit()
db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
db.close()
before = hashlib.sha256(db_path.read_bytes()).digest()
quality = subprocess.run([threads, "routing-compare"], capture_output=True, text=True,
                         env={**os.environ, "AGENTS_OPERATOR_DIR": str(operator), "PYTHONDONTWRITEBYTECODE": "1"})
assert quality.returncode == 0, quality.stderr
assert hashlib.sha256(db_path.read_bytes()).digest() == before, "quality readout wrote to snapshot"
c = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
for arm, start, end, _ in bounds:
    lines = [re.split(r"\s{2,}", line.strip()) for line in quality.stdout.splitlines() if line.startswith(arm + " ")]
    overall = next(row for row in lines if row[1] == "overall" and len(row) == 9)
    direct = c.execute("SELECT count(DISTINCT item) FROM runs WHERE role='worker' AND landed=1 AND item IS NOT NULL "
                       "AND julianday(coalesce(landed_at,ended)) >= julianday(?) "
                       "AND julianday(coalesce(landed_at,ended)) < julianday(?)", (start, end)).fetchone()[0]
    assert int(overall[2]) == direct == 7, (arm, overall, direct)
    assert overall[5].endswith("(4/6)") and overall[6].endswith("(1/5)"), (arm, overall)
    assert f"quality coverage {arm} overall first_try known=6/7 unknown=1/7 adverse known=5/7 unknown=2/7" in quality.stdout, quality.stdout
    feature = next(row for row in lines if row[1] == "feature" and len(row) == 9)
    assert feature[5] == "80.00% [37.55,96.38] (4/5)", feature
    tooling = next(row for row in lines if row[1] == "tooling" and len(row) == 9)
    assert tooling[5:7] == ["n/a (n=1)", "n/a (n=0)"], tooling
c.close()
print(proc.stdout)
print(quality.stdout)
PYROUTING
routing_status=$?
out=$(cat "$scratch/routing.out")
check '[recorded] routing comparison matches direct SQL landed-item counts across three arms, Wilson rates, item review minutes and account points without writing its snapshot' \
  '[ "$routing_status" -eq 0 ]'

renamed_db="$scratch/renamed-queue.db"
sqlite3 "$renamed_db" <<'SQL'
CREATE TABLE old_queue(id INTEGER PRIMARY KEY, placement TEXT NOT NULL CHECK(length(placement) > 0), item TEXT, cmd TEXT NOT NULL CHECK(json_valid(cmd)), state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','taken','done','failed')), queued TEXT NOT NULL, taken TEXT, finished TEXT, result TEXT) STRICT;
INSERT INTO old_queue VALUES(73,'renamed',NULL,'["true"]','queued','2026-10-13T00:00:00Z',NULL,NULL,NULL);
ALTER TABLE old_queue RENAME TO job_queue;
SQL
out=$(THREADS_DB="$renamed_db" "$threads" queue take renamed &&
      THREADS_DB="$renamed_db" "$threads" queue msg 73 requeued draining &&
      THREADS_DB="$renamed_db" "$threads" queue take renamed &&
      THREADS_DB="$renamed_db" "$threads" queue msg 73 done settled)
renamed_status=$?
renamed_ids=$(grep -c '"id": 73' <<<"$out")
renamed_row=$(sqlite3 "$renamed_db" 'SELECT cmd,state,result FROM job_queue WHERE id=73')
renamed_expected='["true"]|done|settled'
check 'renamed queue preserves its job through dropped and requeued migrations' \
  '[ "$renamed_status" -eq 0 ] && [ "$renamed_ids" -eq 2 ] && [ "$renamed_row" = "$renamed_expected" ]'

PYTHONDONTWRITEBYTECODE=1 python3 - "$repo" "$scratch" <<'PYINTEGRITY'
import os, sqlite3, subprocess, sys
from pathlib import Path
repo, scratch = map(Path, sys.argv[1:])
home = scratch / 'integrity-home'
state = home / '.local/state/threads'; state.mkdir(parents=True)
conf = home / '.config/threads'; conf.mkdir(parents=True); (conf / 'remote').write_text('fixture')
transport = scratch / 'integrity-bin'; transport.mkdir()
ssh = transport / 'ssh'
ssh.write_text('''#!/usr/bin/env python3
import os, subprocess, sys
if os.environ.get('OFFLINE'): sys.exit(255)
cmd = sys.argv[-1]
if cmd == 'north threads snapshot-pages':
    with open(os.environ['REFRESH_TRACE'], 'a') as trace: trace.write('refresh\\n')
cmd = cmd.replace('north ', os.environ['NORTH_BIN'] + ' ', 1) if cmd.startswith('north ') else cmd.replace('threads ', os.environ['PUBLIC_THREADS'] + ' ', 1)
r = subprocess.run(cmd, shell=True, env=dict(os.environ, THREADS_DB=os.environ['CANONICAL']), capture_output=True)
out = r.stdout[:-1] + bytes([r.stdout[-1] ^ 1]) if os.environ.get('BAD_STREAM') and cmd.endswith('snapshot-pages') else r.stdout
sys.stdout.buffer.write(out); sys.stderr.buffer.write(r.stderr); sys.exit(r.returncode)
''')
ssh.chmod(0o755)
def canonical(name, page_size):
    path = scratch / name
    with sqlite3.connect(path) as db:
        db.execute(f'PRAGMA page_size={page_size}'); db.execute('CREATE TABLE fixture(value)')
    subprocess.run([str(repo / 'bin/threads'), 'claim', 'wisp#7', '--by', 'fixture'],
                   env=dict(os.environ, THREADS_DB=str(path)), check=True, capture_output=True)
    return path
def run(env, *args):
    return subprocess.run([str(repo / 'bin/threads'), *args], env=env, capture_output=True, text=True, timeout=10)
old = canonical('integrity-old.db', 4096); new = canonical('integrity-new.db', 512)
env = dict(os.environ, HOME=str(home), PATH=str(transport) + ':' + os.environ['PATH'],
           PUBLIC_THREADS=str(repo / 'bin/threads'), CANONICAL=str(old))
env.pop('THREADS_DB', None)
env.pop('XDG_STATE_HOME', None)
env['REFRESH_TRACE'] = str(scratch / 'integrity-refresh.trace')
snapshot = state / 'snapshot.db'
subprocess.run([os.environ['NORTH_BIN'], 'threads', 'snapshot-pull', '--force', 'fixture'], env=env, check=True); (state / 'snapshot.db.tried').unlink()
with sqlite3.connect(snapshot) as db:
    db.execute('ALTER TABLE runs DROP COLUMN brief_origin')
hold = sqlite3.connect(snapshot)
hold.execute('PRAGMA journal_mode=WAL'); hold.execute('PRAGMA wal_autocheckpoint=0')
hold.execute('BEGIN'); hold.execute('SELECT * FROM runs').fetchall()
before = snapshot.read_bytes()
wal = Path(str(snapshot) + '-wal'); before_wal = wal.read_bytes()
fallback = run(dict(env, OFFLINE='1'), 'list')
assert fallback.returncode == 0 and 'wisp#7' in fallback.stdout, fallback
assert wal.read_bytes() == before_wal and snapshot.read_bytes() == before, 'cached read mutated snapshot/WAL'
os.utime(snapshot, (0, 0))
replacement = run(dict(env, CANONICAL=str(new)), 'claim', 'wisp#8', '--by', 'fixture')
assert replacement.returncode == 0, replacement
readout = run(dict(env, OFFLINE='1'), 'list')
assert readout.returncode == 0 and 'wisp#8' in readout.stdout, readout
with sqlite3.connect(snapshot.resolve().as_uri() + '?immutable=1', uri=True) as db:
    assert db.execute('PRAGMA quick_check').fetchall() == [('ok',)]
hold.close()
print('PASS 1201 recorded: cached fallback and valid replacement preserve snapshot integrity across a held WAL reader')
before = snapshot.read_bytes(); os.utime(snapshot, (0, 0)); (state / 'snapshot.db.tried').unlink(missing_ok=True)
invalid = run(dict(env, CANONICAL=str(new), BAD_STREAM='1'), 'claim', 'wisp#9', '--by', 'fixture')
assert invalid.returncode == 0 and snapshot.read_bytes() == before, invalid
assert not list(state.glob('*.tmp')), 'invalid transport temporary file remains'
print('PASS 1202 integration: SQLite-header invalid stream preserves the previous snapshot')
requested_state = scratch / 'integrity-xdg/threads'; requested_state.mkdir(parents=True)
requested = requested_state / 'snapshot.db'; requested.write_bytes(b'corrupt requested snapshot')
with sqlite3.connect(new) as db:
    db.execute("INSERT INTO calibration(at,kind,cell,n,median,source) VALUES ('2026-10-14T00:00:00Z','box','requested',1,7,'fixture')")
default_before = snapshot.read_bytes()
trace = Path(env['REFRESH_TRACE']); trace.write_text('')
reader = subprocess.run([sys.executable, '-I', '-c',
    "import sys; sys.path.insert(0, sys.argv[1]); import calibration; print(calibration.rows())",
    str(repo / 'harness/lib')], env=dict(env, CANONICAL=str(new), XDG_STATE_HOME=str(requested_state.parent)),
    capture_output=True, text=True, timeout=10)
assert reader.returncode == 0 and "'cell': 'requested'" in reader.stdout, ('requested reader unavailable', reader)
assert trace.read_text().splitlines() == ['refresh'], 'requested recovery did not refresh exactly once'
assert snapshot.read_bytes() == default_before, 'requested recovery replaced the default snapshot'
with sqlite3.connect(requested.resolve().as_uri() + '?mode=ro&immutable=1', uri=True) as db:
    assert db.execute('PRAGMA quick_check').fetchall() == [('ok',)]
assert not list(requested_state.glob('*.tmp')), 'requested transport temporary file remains'
print('PASS 1204 integration: requested XDG reader recovers once without changing default snapshot')
PYINTEGRITY
integrity_status=$?
check '1201/1202 snapshot producer and transport integration' '[ "$integrity_status" -eq 0 ]'

printf 'NorthThreads: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
