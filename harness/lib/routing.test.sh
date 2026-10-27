#!/usr/bin/env bash
# Money property over the catalog's tiers and the Python rules placement.py still shares (firn#12); north pace plans the scenarios.
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

states = decisions = picks = 0
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
print(f"PASS money property: {states} account states x {len(tiers)} catalog tiers x {len(boxes)} boxes with and without an explore slot; "
      f"{decisions} decisions, {picks} picks, 0 money")
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
db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, resets_at TEXT, used_pct REAL)")
reset="2026-10-17T04:00:00Z"
rows=[("2026-10-15T06:00:00Z",40),("2026-10-15T09:00:00Z",60),("2026-10-15T12:00:00Z",63)]
db.executemany("INSERT INTO usage VALUES (?,'codex','a','window','week',?,?)",[(stamp,reset,pct) for stamp,pct in rows])
r={"ts":rows[-1][0],"provider":"codex","account":"a","name":"week","resets_at":reset,"used_pct":63,"window_min":10080}
assert plan_usage.beta(db,r,plan_usage.ts(r["ts"]))==1
r["resets_at"]="2026-10-18T04:00:00Z"
assert plan_usage.beta(db,r,plan_usage.ts(r["ts"])) is None
rows=[]
for provider,account,name,pct in (("claude","a","weekly_all",10),("claude","b","weekly_all",40),
                                  ("codex","x","week",20),("gemini","g","group1/weekly",10),
                                  ("gemini","g","group2/weekly",20)):
    rows.append({"provider":provider,"account":account,"name":name,"used_pct":pct,"kind":"window",
                 "window_min":10080,"resets_at":"2026-10-17T04:00Z","ts":now.isoformat()})
ctx=plan_usage.live_ctx({}, {}, rows, None, now, {}, {"claude-a":50,"claude-b":50,"codex":50,"gemini":1.25})
cash={n:routing.account_metrics(a,now)["available_usd"] for n,a in ctx["accounts"].items()}
assert cash=={"claude:a":45,"claude:b":30,"codex":40,"gemini":1}, cash
print("PASS pace property: Claude5/Codex15, per-account cash including binding Gemini weekly quota, equality, Friday ET across DST; 3h burn excludes older usage and other reset windows")

PY

python3 -I - "$HERE" <<'PY' || fail=1
import json, sqlite3, sys
from datetime import timedelta
sys.path.insert(0, sys.argv[1])
import routing, plan_usage
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
result = routing.ask("plan", ctx=live)
bad = routing.ask("divergence", result=result, expect=scenario["expect"])
assert bad is None, bad
assert result == routing.ask("plan", ctx=scenario)
blind = routing.ask("plan", ctx=dict(live, success={}))
assert blind["boxes"]["t1"]["tier"] == "claude:haiku", blind["boxes"]["t1"]
print("PASS live measured: live_ctx from a fixture ledger (run_ok success with escalation and landing, unbriefed unclear runs left out, MIN_RUNS, runs per tier, "
      "latest successor per model) reaches the measured scenario's plan; without its success map t1 falls back to claude:haiku")
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
         (base.replace("Surface: shell", "Surface: shell, ts"), False, "Surface shell, ts"),
         (base.replace("Scope: module", "Scope: module, cross-module"), False, "Scope module, cross-module"),
         (base.replace("Spec: exact", "Spec: exact (north#36)"), True, None),
         (base.replace("ETA: 20 min", "ETA: 15-25 min"), False, "ETA 15-25 min"),
         (base.replace("ETA: 20 min", "ETA: 10-20 min"), True, None),
         (base.replace("ETA: 20 min", "ETA: 1h"), False, "ETA 1h"),
         (base.replace("ETA: 20 min", "ETA: 1.5h"), False, "ETA 1.5h"),
         (base.replace("ETA: 20 min", "ETA: 1h30"), False, "ETA 1h30"),
         (base.replace("ETA: 20 min", "ETA: 2hrs"), False, "ETA 2hrs"),
         (base.replace("ETA: 20 min", "ETA: 20min"), True, None),
         (base.replace("ETA: 20 min", "ETA: 10m"), True, None),
         (base.replace("ETA: 20 min", "ETA: 0.3h"), False, "ETA 0.3h"),
         (base.replace("ETA: 20 min", "ETA: 15 min (uncalibrated)"), True, None),
         (base.replace("ETA: 20 min", "ETA: 25 min (uncalibrated)"), False, "ETA 25 min (uncalibrated)"),
         (base.replace("ETA: 20 min", "ETA: 2d"), False, "ETA 2d"),
         (base.replace("ETA: 20 min", "ETA: 2 days"), False, "ETA 2 days"),
         (base.replace("ETA: 20 min", "ETA: 3 weeks"), False, "ETA 3 weeks"),
         (base.replace("ETA: 20 min", "ETA: 90s"), False, "ETA 90s"),
         (base.replace("ETA: 20 min", "ETA: 20-10 min"), False, "ETA 20-10 min"),
         (base.replace("Scope: module", "Scope: module(s)"), False, "Scope module(s)"),
         (base.replace("Surface: shell", "Surface: shell (ts)"), False, "Surface shell (ts)"),
         (base.replace("Surface: shell", "Surface: shell(ts)"), False, "Surface shell(ts)"),
         (base.replace("Surface: shell", "Surface: (ts) shell"), False, "Surface (ts) shell"),
         (base.replace("Surface: shell", "Surface: shell (no ts files touched)"), False, "Surface shell (no ts files touched)"),
         (base.replace("Surface: shell", "Surface: shell (the scripts)"), True, None),
         ("", False, "Header missing")]
for text, ok, why in cases:
    got = routing.codex_qualifies({"brief": text})
    assert got == (ok, why), (text, got, (ok, why))
assert routing.codex_qualifies({}) == (False, "Header missing")
feature_box = {"category": "tooling", "brief": base.replace("tooling", "feature")}
sol_medium, sol_high = {"key": "codex:sol-medium", "model": "m", "effort": "medium"}, {"key": "codex:sol-high", "model": "m", "effort": "high"}
admit_ctx = {"success": {"codex:sol-medium|tooling": 100, "codex:sol-high|tooling": 100}}
assert routing.admit(admit_ctx, sol_medium, feature_box, {}, {}) and not routing.admit(admit_ctx, sol_high, feature_box, {}, {})
bug = {"category": "bug-known-cause", "brief": base.replace("tooling", "bug-known-cause")}
under = {"codex": {"windows": {"week": {"headroom": 90, "window_h": 168, "reset_in": 80, "beta": 0, "as_of": "2026-10-15T12:00Z", "resets_at": "2026-10-17T04:00Z"}},
                   "provider": "codex", "target_left": 15, "usd_per_week": 50, "age_min": 5}}
explore_ctx = {"now": routing.datetime.fromisoformat("2026-10-15T12:00Z"), "accounts": under}
assert routing.account_state(explore_ctx, "codex") == "under pace"
assert routing.explorable(explore_ctx, sol_medium, bug, {})
assert not routing.explorable(explore_ctx, sol_high, bug, {})
assert not routing.explorable(explore_ctx, dict(sol_medium, class_runs={"bug-known-cause": 20}), bug, {})
assert not routing.explorable(explore_ctx, sol_medium, dict(bug, brief=bug["brief"].replace("shell", "ts")), {})
assert not routing.explorable(explore_ctx, sol_medium, dict(bug, brief=bug["brief"].replace("bug-known-cause", "research")), {})
for category in ("performance", "netcode-determinism", "balance-tuning"):
    tom_facing = dict(bug, brief=bug["brief"].replace("bug-known-cause", category))
    assert routing.explorable(explore_ctx, sol_high, tom_facing, {}), category
    assert not routing.explorable(explore_ctx, sol_medium, tom_facing, {}), category
    assert not routing.admit({}, sol_high, tom_facing, {}), category
    visual = dict(tom_facing, brief=tom_facing["brief"].replace("Surface: shell", "Surface: assets").replace("Verify: local-test", "Verify: visual"))
    assert routing.explorable(explore_ctx, sol_high, visual, {}), category
    assert not routing.codex_qualifies(visual, shape_only=True)[0], category
assert not routing.codex_qualifies(dict(bug, brief=bug["brief"].replace("shell", "lua")), explore=True)[0]
import json, plan_usage, routing
explore_scenario = json.load(open(f"{sys.argv[1]}/routing-scenarios/codex-explore.json"))
explore_result = routing.ask("plan", ctx=explore_scenario)
shown = plan_usage.lines(dict(explore_scenario, now=routing.datetime.fromisoformat(explore_scenario["now"])), explore_result)
assert any(line.startswith("box b0 ") and "(explore: brief header Explore: codex)" in line for line in shown), shown
assert not any("Explore: codex" in line for line in shown if line.startswith("box b10 ")), shown
assert routing.codex_class("tooling") == {"route": "codex", "tiers": ["codex:sol-high"]}
assert routing.codex_class("bug-known-cause")["route"] == "claude" and routing.codex_class("new-thing") == {"route": "claude", "tiers": []}
print(f"PASS codex_qualifies: {len(cases)} header cases cover C1-C10 with the failed criterion named, body lines ignored, multi-facet and range values refused, tiers looked up by header Category; Codex exploration only in a Claude class's listed tier, under the run cap, never on ts or lua; Tom-facing performance, netcode and balance explore sol-high on assets or visual boxes but are never admitted; agents plan tells the lead to write Explore: codex; per-class route and tiers read from defaults.toml")
PY

python3 -I - "$HERE" <<'PY' || fail=1
import sys
sys.path.insert(0, sys.argv[1])
import routing

now = "2026-10-15T08:00:00Z"
def rows(*pairs, resets="2026-10-15T10:00:00Z"):
    return [{"ts": ts, "used_pct": pct, "resets_at": resets} for ts, pct in pairs]
stale = rows(("2026-10-15T04:50:00Z", 97), ("2026-10-15T04:59:00Z", 99), resets="2026-10-15T05:00:00Z")
over = rows(("2026-10-15T07:00:00Z", 0), ("2026-10-15T07:30:00Z", 28), ("2026-10-15T08:00:00Z", 43))
under = rows(("2026-10-15T07:30:00Z", 25), ("2026-10-15T08:00:00Z", 34))
cases = [
    ([], {}, (False, "claude:a 5h unread")),
    (stale, {}, (False, "claude:a 5h unread")),
    (over, {}, (True, "claude:a 5h projected 103% > 90%")),
    (stale + over, {}, (True, "claude:a 5h projected 103% > 90%")),
    (under, {}, (False, "claude:a 5h 34% projected 70%")),
    (stale + under, {}, (False, "claude:a 5h 34% projected 70%")),
    (rows(("2026-10-15T07:40:00Z", 21)), {}, (False, "claude:a 5h 21% projected 21%")),
    (rows(("2026-10-15T06:00:00Z", 0), ("2026-10-15T08:00:00Z", 21)), {}, (False, "claude:a 5h 21% projected 21%")),
    (under, {"target": 60}, (True, "claude:a 5h projected 70% > 60%")),
    (rows(("2026-10-15T07:30:00Z", 90), ("2026-10-15T08:00:00Z", 90)), {}, (True, "claude:a 5h 90% >= 90%")),
    (rows(("2026-10-15T08:00:00Z", 96)), {}, (True, "claude:a 5h 96% >= 95% ceiling")),
    (under, {"throttled_at": "2026-10-15T07:55:00Z"}, (True, "claude:a 5h 429 5 min ago")),
    ([], {"throttled_at": "2026-10-15T07:45:00Z"}, (True, "claude:a 5h 429 15 min ago")),
    (under, {"throttled_at": "2026-10-15T07:44:00Z"}, (False, "claude:a 5h 34% projected 70%")),
    (rows(("2026-10-15T07:30:00Z", 40), ("2026-10-15T08:00:00Z", 39)), {}, (False, "claude:a 5h 39% projected 39%")),
    (rows(("2026-10-15T07:30:00Z", 25), ("2026-10-15T08:00:00Z", 34), ("2026-10-15T08:30:00Z", 99)), {}, (False, "claude:a 5h 34% projected 70%")),
    (rows(("2026-10-15T07:00:00Z", 0), ("2026-10-15T07:30:00Z", 28), resets="2026-10-15T09:59:59Z") + rows(("2026-10-15T08:00:00Z", 43)), {}, (True, "claude:a 5h projected 103% > 90%")),
    (rows(("2026-10-15T06:50:00Z", 20), ("2026-10-15T07:20:00Z", 35)), {}, (True, "claude:a 5h projected 115% > 90%")),
]
for got_rows, kwargs, want in cases:
    got = routing.claude_session_overflow(got_rows, now, **kwargs)
    assert got == want, (got_rows, kwargs, got, want)
held = {"claude:a": {"state": "at or over pace"}, "claude:b": {"state": "at or over pace"}}
assert routing.claude_unavailable(held, [], now) == (True, "no Claude account admits: claude:a at or over pace, claude:b at or over pace")
assert routing.claude_unavailable(dict(held, **{"claude:b": {"state": "under pace"}}), [], now)[0] is False
assert routing.claude_unavailable(dict(held, **{"claude:b": {"state": "under pace", "stale": "old"}}), [], now)[0] is True
assert routing.claude_unavailable(None, [], now)[0] is False
assert routing.claude_unavailable({}, [], now)[0] is False
assert routing.claude_unavailable({"claude:a": {"state": "under pace"}}, [], now, throttled_at="2026-10-15T07:45:00Z") == (True, "claude:a 5h 429 15 min ago")
print(f"PASS claude_session_overflow: {len(cases)} cases cover burn over the 30 min before the newest reading of the newest window (resets within 5 min count as one window), projected from that reading, stale windows and older readings ignored, no readings, target, ceiling and a 429 within 15 min; claude_unavailable when every Claude account is at or over pace or stale, never on a missing reading")
PY

python3 -I - "$HERE" <<'PY' || fail=1
import copy, json, sys
sys.path.insert(0, sys.argv[1])
import routing
base = json.load(open(f"{sys.argv[1]}/routing-scenarios/two-claude.json"))
base["accounts"] = {"claude:a": base["accounts"]["claude:a"]}
base["tiers"] = [t for t in base["tiers"] if t["key"].startswith("claude:")]
base["boxes"] = [b for b in base["boxes"] if b["id"] == "t1"]
base["pace"] = {"enforce_from": {"claude:a": "2026-10-01T00:00Z"}}
base["billing"]["claude"]["overflow"] = "blocked"

def plan(week=None, five_hour=None):
    ctx = copy.deepcopy(base)
    windows = ctx["accounts"]["claude:a"]["windows"]
    windows["week"]["headroom"] = week if week is not None else windows["week"]["headroom"]
    windows["5h"]["headroom"] = five_hour if five_hour is not None else windows["5h"]["headroom"]
    box = routing.ask("plan", ctx=ctx)["boxes"]["t1"]
    return box.get("tier"), box.get("wait")

state = routing.ask("plan", ctx=base)["accounts"]["claude:a"]["state"]
assert state == "at or over pace", state
tier, wait = plan()
assert tier and tier.startswith("claude:") and wait is None, (tier, wait)
assert plan(week=5) == (tier, None), plan(week=5)
assert plan(five_hour=6)[0] == tier
assert plan(week=0) == (None, "account exhausted"), plan(week=0)
assert plan(five_hour=5) == (None, "account exhausted"), plan(five_hour=5)
print("PASS launch rule: an account over its pace line, enforced and without a drain grant, still gets a tier; a 5h window at 95% or a provider week at 100% is the only refusal")
PY

exit "$fail"
