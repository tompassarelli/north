#!/usr/bin/env bash
# routing.ask against the north pace router: per-account cash from live readings, the 3h burn, and the measured scenario rebuilt from a fixture ledger.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
fail=0

python3 -I - "$HERE" <<'PY' || fail=1
import sys
from datetime import datetime
sys.path.insert(0, sys.argv[1])
import plan_usage, routing, sqlite3
now = datetime.fromisoformat("2026-10-11T12:00Z")
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
plan = routing.ask("plan", ctx=ctx)["accounts"]
cash = {n: plan[n]["available_usd"] for n in ctx["accounts"]}
assert cash=={"claude:a":45,"claude:b":30,"codex":40,"gemini":1}, cash
print("PASS plan_usage cash and burn: per-account cash including binding Gemini weekly quota; 3h burn excludes older usage and other reset windows")

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

exit "$fail"
