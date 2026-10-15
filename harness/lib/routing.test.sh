#!/usr/bin/env bash
# Money property over the catalog's tiers, then the four §4 scenarios and two Claude accounts (firn#12).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
fail=0

python3 -I - "$HERE" <<'PY' || fail=1
import itertools, math, sys, tomllib
from datetime import datetime
sys.path.insert(0, sys.argv[1])
import routing

agents = sys.argv[1].rsplit("/lib", 1)[0]
catalog = tomllib.load(open(f"{agents}/model-catalog.toml", "rb"))["models"]
import json, os
bands = tomllib.load(open(f"{sys.argv[1]}/fixtures/orchestration.toml", "rb"))["tiers"]
tiers = [{"key": k, "model": v["model"], "effort": v["effort"], "rank": i,
          "start": bands.get(k, {}).get("start", 100), "runs": 5 if i % 3 == 0 else 50}
         for i, (k, v) in enumerate(catalog.items()) if "model" in v]
accounts = sorted({routing.account_of(t) for t in tiers})
now = datetime.fromisoformat("2026-10-11T12:00Z")
MODES = ("standard", "fast", "ultrafast", "overflow")
VALUES = ("plan", "money", "blocked", None)
projects = {f"project-{i}-{u}": {"u": u} for i in range(3) for u in (1.0, 1.6)}
shapes = (("public", 10, "any"), ("private", 60, "band"), ("client", 95, "band"), ("public", 40, "band"))
boxes = [{"id": f"{p}-{i}", "project": p, "category": shapes[(i + j) % 4][2], "difficulty": shapes[(i + j) % 4][1],
          "sensitivity": shapes[(i + j) % 4][0]} for j, p in enumerate(projects) for i in (0, 2)]
success = {f"{t['key']}|any": 100 for t in tiers}

def money(ctx, name, mode):
    a, b = ctx["accounts"][name], ctx["billing"][name]
    if a["age_min"] > routing.STALE_MIN or b.get(mode) != "plan":
        return True
    used = max(100 - w["headroom"] for w in a["windows"].values())
    overflow_free = b.get("overflow") in ("plan", "blocked") and not (a["hasCredits"] or a["extra_usage"] or a["spend"])
    return used >= routing.MONEY_CEILING and not overflow_free

states = decisions = picks = fasts = 0
for bill in itertools.product(("plan", "money", None), ("plan", "money", None), ("plan", None), VALUES):
    for credits, extra, stale, pct in itertools.product((False, True), (False,), (False, True), (0, 94, 95, 100)):
        acct = {"windows": {"5h": {"headroom": 100 - pct, "reset_in": 2, "beta": 1}, "week": {"headroom": 100-pct,"reset_in":20,"beta":0.5,"window_h":168,"resets_at":"2026-10-12T08:00Z","as_of":now.isoformat()}},
                "grants": [], "hasCredits": credits, "extra_usage": extra, "spend": False, "age_min": 45 if stale else 5,
                "provider":"claude","target_left":5,"usd_per_week":50}
        billing = {m: v for m, v in zip(MODES, bill) if v is not None}
        ctx = {"now": now, "tiers": tiers, "success": success, "lease": True, "projects": projects,
               "accounts": {n: dict(acct,provider=n.split(":")[0],target_left=15 if n.startswith("codex") else 5) for n in accounts}, "billing": {n: billing for n in accounts}}
        states += 1
        for box in boxes:
            for slot in (False, True):
                decisions += 1
                t = routing.choose(ctx, box, slot)["tier"]
                if t is None:
                    continue
                picks += 1
                name = routing.account_of(t)
                if money(ctx, name, "standard"):
                    print(f"FAIL choose picked money: {t['key']} box {box['id']} billing {billing} credits {credits} extra {extra} stale {stale} used {pct}")
                    sys.exit(1)
                if routing.fast(ctx, name, projects[box["project"]]["u"]):
                    fasts += 1
                    if money(ctx, name, "fast"):
                        print(f"FAIL fast on money: {name} billing {billing} credits {credits} extra {extra} stale {stale} used {pct}")
                        sys.exit(1)
print(f"PASS money property: {states} account states x {len(tiers)} catalog tiers x {len(boxes)} boxes with and without an explore slot; "
      f"{decisions} decisions, {picks} picks, {fasts} fast-on, 0 money")
from datetime import timedelta
from zoneinfo import ZoneInfo
import plan_usage, sqlite3
for provider,target in (("claude",5),("codex",15)):
    for reset in (datetime.fromisoformat("2026-10-17T04:00Z"),datetime.fromisoformat("2026-11-07T05:00Z")):
        start = reset-timedelta(days=7)
        cutoff = reset if provider == "claude" else reset-timedelta(minutes=1)
        for fraction in (0,0.25,0.5,0.75,1):
            instant = start+(cutoff-start)*fraction
            wanted = (100-target)*fraction
            for delta in (-1,0,1):
                if not 0 <= wanted+delta <= 100:
                    continue
                a={"provider":provider,"target_left":target,"usd_per_week":50,
                   "windows":{"week":{"headroom":100-wanted-delta,"window_h":168,
                                        "resets_at":reset.isoformat(),"as_of":instant.isoformat(),"beta":0}}}
                m=routing.account_metrics(a,instant)
                assert math.isclose(m["pace_pct"],wanted), (provider,m,wanted)
                assert math.isclose(m["available_usd"],max(100-wanted-delta,0)/2)
                assert m["state"] == ("under pace" if delta < 0 else "at or over pace")
                if provider == "codex":
                    friday=datetime.fromisoformat(m["pace_at"]).astimezone(ZoneInfo("America/New_York"))
                    assert (friday.weekday(),friday.hour,friday.minute)==(4,23,59)
db=sqlite3.connect(":memory:")
db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, name TEXT, resets_at TEXT, used_pct REAL)")
reset="2026-10-17T04:00:00Z"
rows=[("2026-10-15T06:00:00Z",40),("2026-10-15T09:00:00Z",60),("2026-10-15T12:00:00Z",63)]
db.executemany("INSERT INTO usage VALUES (?,'codex','a','week',?,?)",[(stamp,reset,pct) for stamp,pct in rows])
r={"ts":rows[-1][0],"provider":"codex","account":"a","name":"week","resets_at":reset,"used_pct":63,"window_min":10080}
assert plan_usage.beta(db,r,plan_usage.ts(r["ts"]))==1
r["resets_at"]="2026-10-18T04:00:00Z"
assert plan_usage.beta(db,r,plan_usage.ts(r["ts"])) is None
a={"provider":"claude","target_left":5,"usd_per_week":50,"age_min":5,
   "windows":{"week":{"headroom":60,"window_h":168,"reset_in":20,"resets_at":"2026-10-12T08:00Z","as_of":now.isoformat(),"beta":0}}}
b=dict(a,windows={"week":dict(a["windows"]["week"],headroom=30,reset_in=10,resets_at="2026-10-11T22:00Z")})
ctx={"now":now,"accounts":{"claude:a":a,"claude:b":b},"billing":{"claude":{"standard":"plan","overflow":"blocked"}}}
assert routing.route_account(ctx,"claude")=="claude:b"
b["windows"]["week"]["headroom"]=5
assert routing.route_account(ctx,"claude")=="claude:a"
a["windows"]["week"]["headroom"]=10
ctx.update(tiers=[{"key":"claude:opus-medium","rank":0,"start":0}], projects={"north":{"u":1}}, lease=True,
           pace={"enforce_from":{"claude:a":"2026-10-01T00:00Z","claude:b":"2026-10-01T00:00Z"}},
           boxes=[{"id":"hold","project":"north","category":"feature","difficulty":95,"sensitivity":"private"}])
assert routing.route_account(ctx,"claude") is None
assert routing.pick_accounts(ctx)["tiers"][0]["account"]=="claude:b"
held=routing.plan(ctx)
assert held["boxes"]["hold"]["wait"]=="account pace" and held["needs"]==[], held
rows=[]
for provider,account,name,pct in (("claude","a","weekly_all",10),("claude","b","weekly_all",40),
                                  ("codex","x","week",20),("gemini","g","group1/weekly",10),
                                  ("gemini","g","group2/weekly",20)):
    rows.append({"provider":provider,"account":account,"name":name,"used_pct":pct,"kind":"window",
                 "window_min":10080,"resets_at":"2026-10-17T04:00Z","ts":now.isoformat()})
ctx=plan_usage.live_ctx({}, {}, rows, None, now, {}, {"claude-a":50,"claude-b":50,"codex":50,"gemini":1.25})
cash={n:routing.account_metrics(a,now)["available_usd"] for n,a in ctx["accounts"].items()}
assert cash=={"claude:a":45,"claude:b":30,"codex":40,"gemini":1}, cash
print("PASS pace property: Claude5/Codex15, per-account cash including binding Gemini weekly quota, equality, Friday ET across DST, nearest eligible reset; held private work preserves real account identity without questions; 3h burn excludes older usage and other reset windows")

PY

python3 -I - "$HERE" <<'PY' || fail=1
import json, sqlite3, sys
from datetime import timedelta
sys.path.insert(0, sys.argv[1])
import plan_usage, routing
scenario = json.load(open(f"{sys.argv[1]}/routing-scenarios/measured.json"))
now = plan_usage.ts(scenario["now"])
tiers = {t["key"]: {"model": t["model"], "effort": t["effort"], "start": t["start"]} for t in scenario["tiers"]}
db = sqlite3.connect(":memory:")
db.execute("CREATE TABLE runs(agent TEXT, role TEXT, tier TEXT, category TEXT, outcome TEXT, follows TEXT, expects_landing INT, landed INT,"
           " item TEXT)")
db.execute("CREATE TABLE model_events(ts TEXT, model TEXT, kind TEXT, at TEXT, p REAL, source TEXT, detail TEXT)")
cells = (("claude:haiku", "tooling", 10, 4), ("codex:sol-medium", "tooling", 20, 19), ("codex:sol-high", "docs", 10, 10), ("codex:sol-high", "tooling", 20, 19),
         ("claude:opus-medium", "docs", 10, 10), ("claude:opus-high", "docs", 10, 10), ("claude:opus-xhigh", "docs", 10, 10))
n = 0
for key, category, runs, ok in cells:
    for i in range(runs):
        n += 1
        db.execute("INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES (?, 'worker', ?, ?, ?, NULL, 0, 0)",
                   (f"a{n}", plan_usage.RUN_TIERS[key], category, "done" if i < ok else "failed"))
db.execute("INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES ('lead', 'lead', 'medium', 'tooling', 'failed', NULL, 0, 0)")
db.execute("INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES ('b1', 'worker', 'haiku', 'tooling', 'done', NULL, 1, 0)")
db.execute("INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES ('b2', 'worker', 'medium', 'tooling', 'failed', 'b1', 0, 0)")
db.execute("DELETE FROM runs WHERE agent = 'a10'")
db.executemany("INSERT INTO runs VALUES (?, 'worker', 'haiku', 'tooling', 'unclear', NULL, 0, 0, NULL)", [(f"u{i}",) for i in range(3)])
at = lambda d: (now + timedelta(days=d)).isoformat().replace("+00:00", "Z")
db.executemany("INSERT INTO model_events VALUES (?, ?, ?, ?, ?, 'fixture', '{}')",
               [("2026-10-01T00:00:00Z", "anthropic:claude-opus-6", "market", at(9), 0.4),
                ("2026-10-02T00:00:00Z", "anthropic:claude-opus-6", "market", at(5), 0.7),
                ("2026-10-02T00:00:00Z", "anthropic:claude-opus-4", "market", at(1), 0.9),
                ("2026-10-02T00:00:00Z", "openai:gpt-6.2-sol", "released", at(-1), None),
                ("2026-10-02T00:00:00Z", "google:gemini-4-argon", "market", at(2), 0.3)])
live = plan_usage.live_ctx({}, {}, [], db, now, tiers)
assert live["success"] == scenario["success"], live["success"]
assert [(t["key"], t["runs"], t.get("successor")) for t in live["tiers"]] == [
    (t["key"], t["runs"], t.get("successor")) for t in scenario["tiers"]], live["tiers"]
live.update({k: scenario[k] for k in ("billing", "lease", "accounts", "projects", "boxes")})
result = routing.plan(live)
bad = routing.divergence(result, scenario["expect"])
assert bad is None, bad
assert result == routing.plan(scenario)
blind = routing.plan(dict(live, success={}))
assert blind["boxes"]["t1"]["tier"] == "claude:haiku", blind["boxes"]["t1"]
print("PASS live measured: live_ctx from a fixture ledger (run_ok success with escalation and landing, unbriefed unclear runs left out, MIN_RUNS, runs per tier, "
      "latest successor per model) reaches the measured scenario's plan; without its success map t1 falls back to claude:haiku")
PY

python3 -I - "$HERE" <<'PY' || fail=1
import copy, json, sys
sys.path.insert(0, sys.argv[1])
import routing
base = json.load(open(f"{sys.argv[1]}/routing-scenarios/capacity-astra.json"))
ctx = copy.deepcopy(base)
ctx["now"] = routing.datetime.fromisoformat(ctx["now"])
ctx["accounts"] = {"claude:a": ctx["accounts"]["claude"], "claude:b": copy.deepcopy(ctx["accounts"]["claude"])}
ctx["accounts"]["claude:a"]["windows"]["week"]["headroom"] = 33
ctx["accounts"]["claude:b"]["windows"]["week"].update(headroom=90, reset_in=10)
assert routing.route_account(ctx, "claude") == "claude:b"
a, b = routing.pace_reading(ctx, "claude", "claude:a"), routing.pace_reading(ctx, "claude", "claude:b")
assert (a["account"], a["state"], a["used_pct"]) == ("claude:a", "at or over pace", 67), a
assert (b["account"], b["state"]) == ("claude:b", "under pace"), b
assert routing.pace_reading(ctx, "claude")["account"] == "claude:b"

ctx = copy.deepcopy(base)
ctx["accounts"] = {"codex:a": dict(copy.deepcopy(ctx["accounts"]["codex"]), age_min=4), "codex:b": ctx["accounts"]["codex"],
                   "claude": ctx["accounts"]["claude"]}
ctx["accounts"]["codex:a"]["windows"]["week"]["headroom"] = 3
ctx["billing"]["codex"]["standard"] = "plan"
cap = routing.capacity(dict(ctx, now=routing.datetime.fromisoformat(ctx["now"])))
assert cap["codex"]["fallback"] == "codex:astra" and cap["codex"]["left"] > routing.CODEX_FLOOR, cap

ctx = copy.deepcopy(base)
ctx["tiers"].append({"key": "codex:astra-xhigh", "model": "gpt-6-astra", "effort": "xhigh", "rank": 5, "start": 90})
ctx["boxes"].append({"id": "x1", "project": "galileo", "category": "feature", "difficulty": 95, "sensitivity": "private", "local": False})
result = routing.plan(ctx)
assert result["boxes"]["x1"]["tier"] != "codex:astra-xhigh" and not result["boxes"]["x1"]["fallback"], result["boxes"]["x1"]
assert result["boxes"]["f1"]["fallback"], result["boxes"]["f1"]
print("PASS capacity and pace per account: a named account reads its own pace, not the routed one; Codex headroom comes from "
      "the routed account, not the alphabetically first; the Astra fallback rewrites only tiers on the model at capacity; no Codex class lists an Astra tier")
PY

SCRATCH="$(mktemp -d)"
trap 'rm -rf -- "$SCRATCH"' EXIT
AGENTS_ORCHESTRATION="$HERE/fixtures/orchestration.toml" CLAUDE_CONFIG_DIR="$SCRATCH" SPAWN_CAPACITY_HOME="$SCRATCH" \
python3 -I - "$HERE" <<'PY' || fail=1
import sys, tomllib
from datetime import datetime, timedelta, timezone
sys.path[:0] = [sys.argv[1], f"{sys.argv[1]}/../hooks/lib"]
import routing, spawn_capacity
cfg = tomllib.load(open(f"{sys.argv[1]}/fixtures/orchestration.toml", "rb"))
now = datetime.now(timezone.utc)
verdicts = {}
QUALIFYING_BRIEF = ("Category: tooling\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\n"
                    "Done when: `bash t.test.sh`\nETA: 15 min\nFiles: harness/t.sh")
for name in ("codex", "claude:a", "claude:b"):
    p = name.split(":", 1)[0]
    for used in (1, 80, 99):
        week = {"headroom": 100 - used, "window_h": 168, "reset_in": 80, "beta": 0, "as_of": now.isoformat(),
                "resets_at": (now + timedelta(hours=80)).isoformat()}
        acct = {"provider": p, "target_left": 15 if p == "codex" else 5, "usd_per_week": 50, "age_min": 5,
                "windows": {"week": week}}
        ctx = {"now": now, "accounts": {name: acct}, "billing": {p: {"standard": "plan", "overflow": "blocked"}},
               "tiers": [{"key": "codex:sol-high" if p == "codex" else f"{p}:t", "account": name, "rank": 0, "start": 0}], "lease": True, "services": [],
               "projects": {k: dict(v, u=1.0) for k, v in cfg["projects"].items()}, "pace": cfg["pace"]}
        m = routing.account_metrics(acct, now)
        reading = routing.pace_reading(ctx, p, name)
        for project in cfg["projects"]:
            box = {"id": "b", "project": project, "category": "tooling", "difficulty": 10, "sensitivity": "private", "brief": QUALIFYING_BRIEF}
            routed = routing.choose(ctx, box)["tier"] is not None
            hook = spawn_capacity.pace_hold([f"Item: {project}#1"], {"pace": reading}, p, {"agent_id": "w"})[0] is None
            assert routed == hook, (name, used, m["state"], project, routed, hook)
            verdicts[(name, used, m["state"], project)] = routed
over = {(n, pr): v for (n, u, s, pr), v in verdicts.items() if u == 80 and s == "at or over pace"}
floor = {(n, pr): v for (n, u, s, pr), v in verdicts.items() if u == 99}
assert floor == {(n, pr): n == "codex" and pr == "galileo" for n in ("codex", "claude:a", "claude:b")
                 for pr in ("galileo", "north", "smashcraft", "wisp")}, floor
# Codex paces to Friday 23:59 ET from the wall-clock window start, so its 80% cell may read under pace; Claude's never does.
expected = {("codex", "galileo"): True, ("codex", "north"): False, ("codex", "smashcraft"): False, ("codex", "wisp"): False,
                ("claude:a", "galileo"): True, ("claude:a", "north"): True, ("claude:a", "smashcraft"): True, ("claude:a", "wisp"): True,
                ("claude:b", "galileo"): False, ("claude:b", "north"): False, ("claude:b", "smashcraft"): False,
                ("claude:b", "wisp"): False}
assert {n for n, pr in over} >= {"claude:a", "claude:b"} and over == {k: v for k, v in expected.items() if k in over}, over
under = [v for (n, u, s, pr), v in verdicts.items() if s == "under pace"]
assert under and all(under), verdicts
print(f"PASS pace agreement: routing.choose and spawn_capacity.pace_hold agree on {len(verdicts)} cells of the fixture "
      "orchestration.toml (enforced or not, under or over pace, at the drain floor, drain-granted live target, ungranted target, no target, passed target)")
PY

python3 -I - "$HERE" <<'PY' || fail=1
import sys
sys.path.insert(0, sys.argv[1])
import routing
base = ("Item: north#51\nCategory: tooling\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\n"
        "Done when: `bash t.test.sh`\nETA: 20 min\nFiles: a.sh, b.sh\n\nbody Surface: ts\n")
cases = [(base, True, None),
         (base.replace("tooling", "bug-known-cause"), False, "Category bug-known-cause"),
         (base.replace("tooling", "new-thing"), False, "Category new-thing"),
         (base.replace("exact", "measured"), True, None),
         (base.replace("tooling", "feature").replace("exact", "measured"), False, "Spec measured"),
         (base.replace("exact", "judged"), False, "Spec judged"),
         (base.replace("module", "cross-module"), False, "Scope cross-module"),
         (base.replace("shell", "ts"), False, "Surface ts"),
         (base.replace("shell", "docs"), True, None),
         (base.replace("local-test", "visual"), False, "Verify visual"),
         (base.replace("`bash t.test.sh`", "tests pass"), False, "Done when has no command"),
         (base.replace("ETA: 20 min", "ETA: 25 min"), False, "ETA 25 min"),
         (base.replace("ETA: 20 min", "ETA: 1 h"), False, "ETA 1 h"),
         (base.replace("ETA: 20 min\n", ""), False, "ETA missing"),
         (base.replace("a.sh, b.sh", "a.sh, b.sh, c.sh, d.sh, e.sh, f.sh, g.sh"), False, "Files"),
         (base.replace("Files: a.sh, b.sh\n", "Files:\n - a.sh\n - b.sh\n"), True, None),
         (base.replace("Files: a.sh, b.sh\n", "Files: the harness\n"), False, "Files"),
         (base.replace("Item:", "Follows: b1 (gpt-6.1-sol high)\nItem:"), False, "Follows a Codex run"),
         (base.replace("Item:", "Follows: b1 (claude-haiku-5-5)\nItem:"), True, None),
         (base.replace("\n\nbody", "\nOpen questions: which file?\n\nbody"), False, "Open questions"),
         (base.replace("Surface: shell\n", "").replace("\n\nbody", "\n\nSurface: shell\nbody"), False, "Surface missing"),
         ("", False, "Header missing")]
for text, ok, why in cases:
    got = routing.codex_qualifies({"brief": text})
    assert got == (ok, why), (text, got, (ok, why))
assert routing.codex_qualifies({}) == (False, "Header missing")
assert routing.codex_class("tooling") == {"route": "codex", "tiers": ["codex:sol-high"], "leash": "normal"}
assert routing.codex_class("bug-known-cause")["route"] == "claude" and routing.codex_class("new-thing") == {"route": "claude", "tiers": [], "leash": "tight"}
assert routing.COACH == {"takeover_fixes": 3, "first_steps": 10, "first_minutes": 8, "ladder_steps": [20, 30, 50], "ladder_minutes": [15, 20, 30]}
print(f"PASS codex_qualifies: {len(cases)} header cases cover C1-C10 with the failed criterion named, body lines ignored; per-class route, tier and leash keys and the coach ladder read from defaults.toml")
PY

for s in burn conserve new-model deadline two-claude reserve thursday-behind capacity-astra capacity-floor measured pace-report-only deadline-drain report-only-floor codex-gate; do
  if out="$(python3 -I "$HERE/routing.py" scenario "$HERE/routing-scenarios/$s.json" 2>&1)"; then
    echo "PASS scenario $s"
  else
    echo "FAIL scenario $s"; echo "$out"; fail=1
  fi
done
exit "$fail"
