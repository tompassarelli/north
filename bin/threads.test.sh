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
export AGENTS_ORCHESTRATION=$scratch/orchestration.toml
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

out=$(t release smashcraft#3 --by alice --to bob; t list)
check '[spec] release --to hands the issue over and records the handoff' \
  'grep -qE "^smashcraft#3 +bob" <<<"$out" && [ "$(sqlite3 "$THREADS_DB" "select data from events where kind = '"'handoff'"'")" = '"'"'{"from": "alice", "to": "bob"}'"'"' ]'

out=$(t claim nixos-config#9 --by alice >/dev/null; t claim tompassarelli/firn#9 --by bob); status=$?
check '[spec] a checkout alias and the GitHub name are the same issue' \
  '[ "$status" -eq 1 ]'
t release firn#9 --by alice >/dev/null

out=$(t ready)
check '[spec] ready lists open issues minus open blockers and held issues' \
  '[ "$(cut -f1 <<<"$out" | tr "\n" " ")" = "smashcraft#2 smashcraft#4 " ]'

brief=$'Goal: x\nItem: tompassarelli/smashcraft#3. Follows: a111. Category: Tooling. Arm: escalate.\nETA 45 min.'
printf '%s\n' \
  "$(jq -cn '{id:"a111",ended:"2026-10-08T01:00:00Z",tier:"high",actual_min:30,eta_min:20,tokens:1000,outcome:"handoff",note:"~/h/a111.md",brief:"Item: smashcraft#3"}')" \
  "$(jq -cn --arg b "$brief" '{id:"a222",ended:"2026-10-08T02:00:00Z",tier:"xhigh",actual_min:50,eta_min:45,tokens:2000,outcome:"done",brief:$b}')" |
  "$threads" ingest 2>/dev/null
out=$(t show smashcraft#3; t summary)
check '[spec] runs take Item, Follows, Category and Arm from the brief; a new agent is a change of hands' \
  'grep -q "a222 xhigh follows a111  50m/45m ETA  done" <<<"$out" && grep -q "handoff  a111 -> a222" <<<"$out" && grep -q "note ~/h/a111.md" <<<"$out" && grep -qE "^tooling +xhigh +escalate +1 +1 +100%" <<<"$out"'

row() { jq -cn "$@"; }
row '{id:"a333",status:"running",tier:"medium",started:"2026-10-08T03:00:00Z",eta_min:30,brief:"Item: smashcraft#4"}' |
  "$threads" ingest 2>/dev/null
out=$(t list)
check '[spec] a running worker whose brief has Item: holds the issue with no claim command' \
  'grep -qE "^smashcraft#4 +a333 .* 30m" <<<"$out"'
row --arg s "$(date -u -d '45 minutes ago' +%Y-%m-%dT%H:%M:%S.000Z)" \
  '{id:"a444",status:"running",tier:"high",started:$s,eta_min:20,brief:"Item: smashcraft#2"}' | "$threads" ingest 2>/dev/null
out=$(t list)
check '[spec] the clock runs from when the worker started, and past 2x ETA it is overdue' \
  'grep -qE "^smashcraft#2 +a444 +4[56]m +20m +OVERDUE" <<<"$out"'
row '{id:"a333",status:"finished",ended:"2026-10-08T03:20:00Z",tier:"medium",actual_min:20,tokens:1,outcome:"done",brief:"Item: smashcraft#4"}' |
  "$threads" ingest 2>/dev/null

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

row '{id:"attribution",landed:1,landed_sha:"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",ended:"2026-10-08T06:40:00Z",tier:"medium",actual_min:25,outcome:"done"}' | "$threads" ingest 2>/dev/null
row '{id:"attribution",backfill:true,landed:0,item:"wisp#71",category:"tooling",eta_min:25,ended:"2026-10-08T06:40:00Z",tier:"medium",actual_min:25,outcome:"done"}' | "$threads" ingest 2>/dev/null
out=$(sqlite3 "$THREADS_DB" "select item,category,eta_min,landed,landed_sha from runs where agent='attribution'")
check '[north#51] backfill fills adapter attribution and ETA on a previously unknown run' '[ "$out" = "wisp#71|tooling|25|1|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" ]'

# Recommend: tier from first-assignment runs that closed without escalation and landed.
runs() {  # runs TIER CATEGORY-AND-FACETS OK FAIL
  local i
  for i in $(seq 1 "$3"); do row --arg t "$1" --arg b "$2" --arg id "$1$RANDOM$i" '{id:$id,ended:"2026-10-09T00:00:00Z",tier:$t,actual_min:5,outcome:"done",landed:1,brief:$b}'; done
  for i in $(seq 1 "$4"); do row --arg t "$1" --arg b "$2" --arg id "$1$RANDOM$i" '{id:$id,ended:"2026-10-09T00:00:00Z",tier:$t,actual_min:5,outcome:"not done",landed:0,brief:$b}'; done
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
  '[ "$(grep -cx "eta 20m (p75 of n=3, cell Category: research, Spec: measured, tier medium)" <<<"$out")" -eq 2 ] && t recommend "Category: research, Spec: measured, Scope: one-file" | grep -q "^eta 5m (p75 of n=5, cell Category: research, Spec: measured, tier claude:haiku, uncalibrated: no eta_pred calibration)" && t recommend "Category: balance-tuning" | grep -q "^eta n/a (n=0)"'

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
base = Path(state) / "agents/worker-sweep"
base.mkdir(parents=True)
(base / "stalls.jsonl").write_text(json.dumps(dict(worker="b100-0", stage="nudge", outcome="closed")) + "\n")
(base / "capacity.jsonl").write_text(json.dumps(dict(run="b100-1", step="restaff")) + "\n")
items, run_n, no_item, unmatched = mod.planner_items(db, base)
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
            last["stalls"], last["recoveries"], last["compactions"]) == (60, 20, 2, 2, 1, 1, 1, 1, 4), last
    assert math.isclose(last["difficulty"], math.log(4) + math.log(3) + 4 * math.log(2) + math.log(5)), last
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
assert not case_failures, case_failures
PYBAND
out=$(cat "$scratch/planner.out")
check '[recorded] planner backfill counts every run and distinct item without ledger writes; realized components include linked recoveries' \
  'grep -qx "backfill ok" <<<"$out"'
check '[measurement] planner cut points separate known zero/one/multiple payoff signals and print their measured split beside the prior' \
  'grep -qx "cuts ok" <<<"$out"'
check '[recorded] planner fixed-seed held-out binomial check keeps withheld outcomes out of ranking and reports a failed chance-level fixture' \
  'grep -qx "holdout ok" <<<"$out"'

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
assert run(str(repo / "bin/worker-ledger"), "--recommend", "Category: balance-tuning").startswith("tier codex:sol-xhigh ")
print("codex-only median")
PYMODE
)
check '[recorded] worker-ledger recommend split stdout stays unchanged' 'grep -qx "split unchanged" <<<"$out"'
check '[recorded] worker-ledger recommend codex-only drops Claude and uses the chosen Codex cell median and n' 'grep -qx "codex-only median" <<<"$out"'

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

calls=$(wc -l <"$GH_TRACE")
a=$(t new life "p a"); b=$(t new life "p b" --due 2026-12-01)
t need "$a" "$b" >/dev/null; t need smashcraft#4 "$b" >/dev/null
blocked=$(t ready | cut -f1); shown=$(t show "$a")
t close "$b" >/dev/null
check '[spec] native items: numbered per namespace, blocked locally (also a GitHub issue), ready again on close, no gh call' \
  '[ "$a $b" = "life#1 life#2" ] && [ "$(grep -cxE "life#1|smashcraft#4" <<<"$blocked")" -eq 0 ] && grep -q "blocked by: life#2 (open)" <<<"$shown" && [ "$(t ready | cut -f1 | grep -cxE "life#[12]|smashcraft#4")" -eq 2 ] && [ "$(wc -l <"$GH_TRACE")" -eq "$calls" ]'

fake_key=AKIALALEMEL33243OLIA # gitleaks:allow
out=$(t note "$a" "fake creds $fake_key"; echo "rc=$?"; t new life "fake $fake_key"; echo "rc=$?")
check '[spec] note and new refuse text gitleaks flags and store nothing' \
  '[ "$(grep -c "^rc=1" <<<"$out")" -eq 2 ] && grep -q "gitleaks found a secret" <<<"$out" && ! sqlite3 "$THREADS_DB" "select data from events union all select title from items" | grep -q "$fake_key"'

t claim "$a" --by alice >/dev/null
out=$(THREADS_NO_NATIVE=1 t list; THREADS_NO_NATIVE=1 t ready; CLAUDE_CODE_REMOTE=true t list; CLAUDE_CODE_REMOTE=true t ready; CLAUDE_CODE_REMOTE=true t show "$a"; THREADS_NO_NATIVE=1 t snapshot)
check '[spec] under the agy (THREADS_NO_NATIVE) or cloud (CLAUDE_CODE_REMOTE) signal, list, ready, show and snapshot reveal no native item' \
  '! grep -qE "life#|p a" <<<"$out" && [ "$(grep -c "not readable here" <<<"$out")" -eq 1 ] && t list | grep -q "^life#1"'
t release "$a" --by alice >/dev/null

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

# 40 processes, each making 25 claims through threads' own entry point.
for w in $(seq 1 40); do
  python3 - "$threads" "$w" <<'PY' >/dev/null &
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
out=$(sqlite3 "$THREADS_DB" "select count(*) from claims where item like 'wisp#%'")
check '[invariant] 40 writers x 25 parallel writes keep all 1000' '[ "$out" = 1000 ]'

# The remote ledger over a fake ssh: ingest spools while it is unreachable, then flushes in order.
mkdir -p "$scratch/home/.config/threads" "$scratch/remote"
echo nexus >"$scratch/home/.config/threads/remote"
ln -s "$threads" "$scratch/remote/threads"
cat >"$scratch/bin/ssh" <<'SSH'
#!/usr/bin/env bash
[ -e "$SSH_DOWN" ] && exit 255
THREADS_DB=$REMOTE_DB PATH=$REMOTE_BIN:$PATH exec bash -c "${@: -1}"
SSH
chmod +x "$scratch/bin/ssh"
proxied() { env -u THREADS_DB HOME="$scratch/home" SSH_DOWN="$scratch/down" REMOTE_DB="$scratch/remote.db" REMOTE_BIN="$scratch/remote" "$threads" "$@" 2>&1; }
touch "$scratch/down"
for n in 1 2; do
  jq -nc --arg id "s$n" '{id:$id,ended:"2026-10-09T01:00:00Z",tier:"medium",actual_min:5,outcome:"done",brief:"Category: tooling"}' | proxied ingest >/dev/null
done
out=$(proxied claim wisp#9 --by alice); status=$?
spooled=$(ls "$scratch/home/.local/state/threads/spool" | wc -l)
rm "$scratch/down"
proxied claim wisp#7 --by bob >/dev/null
touch "$scratch/down"
out2=$(proxied list); status2=$?
check '[integration] remote down: ingest spools and exits 0, a claim fails in one line, a read serves the last snapshot marked STALE; back up: the spool flushes in order' \
  '[ "$status" -eq 255 ] && [ "$spooled" -eq 2 ] && [ -z "$(ls "$scratch/home/.local/state/threads/spool")" ] && [ "$(sqlite3 "$scratch/remote.db" "select group_concat(agent) from (select agent from runs order by rowid)")" = s1,s2 ] && [ "$status2" -eq 0 ] && grep -q "^STALE: ledger snapshot from .*; nexus unreachable" <<<"$out2" && grep -qE "^wisp#7 +bob" <<<"$out2"'

touch "$scratch/down"
jobrun() { env -u THREADS_DB HOME="$scratch/home" SSH_DOWN="$scratch/down" REMOTE_DB="$scratch/remote.db" REMOTE_BIN="$scratch/remote" "$repo/bin/job-run" "$@" 2>&1; }
jobrun nightly -- sh -c 'exit 3' >/dev/null; jobstatus=$?
jobspooled=$(ls "$scratch/home/.local/state/threads/spool" | wc -l)
rm "$scratch/down"
jobrun nightly -- true >/dev/null
check '[integration] job-run keeps its command'"'"'s exit code, spools start and finish while the remote is down and flushes them with the next run' \
  '[ "$jobstatus" -eq 3 ] && [ "$jobspooled" -eq 2 ] && [ "$(sqlite3 "$scratch/remote.db" "select group_concat(rc) from (select rc from job_runs where name = '"'nightly'"' and finished is not null order by started)")" = 3,0 ]'

qid=$("$threads" queue add laptop smashcraft#3 -- echo hi 2>&1)
"$threads" queue take laptop >"$scratch/take1" 2>&1 & "$threads" queue take laptop >"$scratch/take2" 2>&1 & wait
taken=$(cat "$scratch/take1" "$scratch/take2")
"$threads" queue msg "$qid" done "said hi" >/dev/null 2>&1; msgstatus=$?
"$threads" queue msg "$qid" done "again" >/dev/null 2>&1; againstatus=$?
check '[integration] queue: two concurrent takes claim the one job once; msg records it and comments on its issue; a second msg is refused' \
  '[ "$(grep -c "\"id\": $qid" <<<"$taken")" -eq 1 ] && [ "$msgstatus" -eq 0 ] && [ "$againstatus" -ne 0 ] && grep -q "^issue comment 3 -R tompassarelli/smashcraft" "$GH_TRACE" && [ "$(sqlite3 "$THREADS_DB" "select state from job_queue where id = $qid")" = done ]'

p1=$("$threads" queue add pool - -- echo p1 2>&1)
p2=$("$threads" queue add pool - -- echo p2 2>&1)
nexustook=$("$threads" queue take nexus,pool 2>&1)
"$threads" job-run "queue-$p1" --host nexus --started 2026-10-12T10:02:00Z >/dev/null 2>&1
laptoponly=$("$threads" queue take laptop 2>&1)
laptoptook=$("$threads" queue take laptop,pool 2>&1)
"$threads" job-run "queue-$p2" --host whiterabbit --started 2026-10-12T10:03:00Z >/dev/null 2>&1
out=$(t queue)
check '[recorded] pool: while nexus runs one pooled job the laptop takes the next; a laptop-only take leaves pooled jobs alone' \
  '[ "$(jq .id <<<"$nexustook")" = "$p1" ] && [ -z "$laptoponly" ] && [ "$(jq .id <<<"$laptoptook")" = "$p2" ] && grep -qE "^$p2 +pool +- +taken +\S+ +[0-9]+m +- +whiterabbit 2026-10-12T10:03:00Z rc=- +echo p2$" <<<"$out" && grep -qE "^$p1 +pool +- +taken .* nexus 2026" <<<"$out"'

d0=$("$threads" queue add dropq - -- echo d0 2>&1)
"$threads" queue take dropq >/dev/null 2>&1
d1=$("$threads" queue add dropq smashcraft#3 -- echo d1 2>&1)
t queue drop "$d1" superseded by hand >/dev/null; dropstatus=$?
t queue drop "$d1" again >/dev/null; againstatus=$?
t queue drop "$d0" busy >/dev/null; takenstatus=$?
dropped=$("$threads" queue take dropq 2>&1)
check '[recorded] queue drop: a queued job is dropped with its reason as result and a ledger note; dropped and taken jobs are refused' \
  '[ "$dropstatus" -eq 0 ] && [ "$againstatus" -ne 0 ] && [ "$takenstatus" -ne 0 ] && [ -z "$dropped" ] && [ "$(sqlite3 "$THREADS_DB" "select state || '"'|'"' || result from job_queue where id = $d1")" = "dropped|superseded by hand" ] && [ "$(sqlite3 "$THREADS_DB" "select item || '"'|'"' || json_extract(data, '"'$.text'"') from events where kind = '"'note'"' and json_extract(data, '"'$.job'"') = $d1")" = "smashcraft#3|job $d1 dropped: superseded by hand" ] && [ "$(sqlite3 "$THREADS_DB" "select state from job_queue where id = $d0")" = taken ]'

export GALILEO_CONTAINER=$scratch/galileo
g() { git -C "$GALILEO_CONTAINER/main" -c user.name=t -c user.email=t@t "$@"; }
mkdir -p "$GALILEO_CONTAINER/main" && g init -q -b main
for c in c1 c2 c3; do g commit -q --allow-empty -m "$c"; eval "$c=\$(g rev-parse HEAD)"; done
g checkout -q -b side "$c2" && g commit -q --allow-empty -m x && x=$(g rev-parse HEAD)
g update-ref refs/remotes/origin/main "$c3"
fs() { "$threads" queue add pool - -- galileo-full-suite "$@" 2>&1; }
s1=$(fs "$c1" --skip differential_1); s2=$(fs "$c1" --only differential_1); s3=$(fs "$x" --skip differential_1)
offmain=$(sqlite3 "$THREADS_DB" "select state from job_queue where id = $s1")
s4=$(fs "$c2" --skip differential_1)
s5=$("$threads" queue add elsewhere - -- galileo-full-suite "$c3" --only differential_1 2>&1)
"$threads" queue take elsewhere >/dev/null 2>&1; "$threads" queue msg "$s5" done passed >/dev/null 2>&1
s6=$(fs "$c2" --only differential_1)
states=$(sqlite3 "$THREADS_DB" "select group_concat(state, ' ') from (select state from job_queue where id in ($s1,$s2,$s3,$s4,$s5,$s6) order by id)")
check '[recorded] supersede: a queued galileo-full-suite SHA job drops once a newer queued or passed run with its flags covers a descendant on main; off-main or other-flag runs never do' \
  '[ "$offmain" = queued ] && [ "$states" = "dropped dropped queued queued done dropped" ] && [ "$(sqlite3 "$THREADS_DB" "select result from job_queue where id = $s1")" = "superseded by job $s4 (queued) at $c2" ] && [ "$(sqlite3 "$THREADS_DB" "select result from job_queue where id = $s6")" = "superseded by job $s5 (done) at $c3" ] && [ "$(sqlite3 "$THREADS_DB" "select count(*) from events where kind = '"'note'"' and json_extract(data, '"'$.text'"') like '"'%superseded by job%'"'")" -eq 3 ]'
unset GALILEO_CONTAINER

printf 'threads: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
