#!/usr/bin/env bash
# The Python ledger libraries behind threads ingest: cost allocation, run usage, delivery metrics and the defaults-measure priors.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/ledger-context-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export HOME=$scratch/home XDG_STATE_HOME=$scratch/home/.local/state CLAUDE_CONFIG_DIR=$scratch/claude
export AGENTS_ORCHESTRATION=$repo/harness/lib/fixtures/orchestration.toml AGENTS_OPERATOR_DIR=$repo/harness/lib/fixtures
mkdir -p "$HOME"
python3 - "$repo/harness/lib" "$scratch/costs.db" "$repo/bin/threads" <<'PYMEASURE'
import importlib.machinery,json,os,sqlite3,sys
sys.path.insert(0,sys.argv[1])
from ledger_context import account_id,allocate_costs,allocation_intervals,delivery_metrics,run_usage
catalog={}
from pathlib import Path
Path(os.environ["CLAUDE_CONFIG_DIR"]).mkdir(parents=True, exist_ok=True)
Path(os.environ["CLAUDE_CONFIG_DIR"], ".claude.json").write_text(json.dumps({"cachedUsageUtilization":{"accountUuid":"fixture-secondary"}}))
assert account_id("claude")=="claude-b"
codex=[dict(type="session_meta",payload=dict(service_tier="priority")),dict(type="turn_context",payload=dict(model="gpt-6.1-sol",effort="medium")),dict(type="event_msg",timestamp="2026-10-11T00:05:00Z",payload=dict(type="token_count",info=dict(total_token_usage=dict(input_tokens=1000,cached_input_tokens=800,output_tokens=100))))]
ct=run_usage(codex+codex[2:],catalog=catalog)
assert ct["fast"]==1 and len(ct["usage_samples"])==1 and ct["usage_samples"][0]["weight"]==780,ct
rows=[dict(ts=f"2026-10-11T00:{minute:02d}:00Z",provider="claude",account="b",kind="window",name="weekly_all",used_pct=used,resets_at="2026-10-15T00:00:00Z") for minute,used in ((0,10),(10,14),(20,20))]
samples=[dict(agent=agent,account="claude-b",ts=ts,weight=weight) for agent,ts,weight in (("a","2026-10-11T00:05:00Z",730),("b","2026-10-11T00:08:00Z",270),("b","2026-10-11T00:15:00Z",100))]
costs=allocate_costs(rows,samples,{"claude-b":50})
assert abs(sum(costs.values())-5)<1e-10 and costs=={"a":1.46,"b":3.54},costs
control='[{"account": "claude-b", "started": "2026-10-11T00:00:00Z", "ended": "2026-10-11T00:10:00Z", "agent": "a", "cost_usd": 1.46}, {"account": "claude-b", "started": "2026-10-11T00:00:00Z", "ended": "2026-10-11T00:10:00Z", "agent": "b", "cost_usd": 0.54}, {"account": "claude-b", "started": "2026-10-11T00:10:00Z", "ended": "2026-10-11T00:20:00Z", "agent": "b", "cost_usd": 3.0}]'
assert json.dumps(allocation_intervals(rows,samples,{"claude-b":50})).encode()==control.encode()
print('PASS [north#51] changing Claude readouts allocate byte-identically to origin/main e46008897ca2c5e97a27f6efcc737006c5199df7')
flat=[dict(rows[0],ts="2026-10-10T23:50:00Z"),*rows]
idle=samples+[dict(agent="c",account="claude-b",ts="2026-10-10T23:55:00Z",weight=500)]
spread=allocate_costs(flat,idle,{"claude-b":50})
assert "(untracked)" not in spread and abs(sum(spread.values())-5)<1e-10,spread
jitter=[row|{"resets_at":"2026-10-15T00:00:01Z"} if row["used_pct"]==14 else row for row in rows]
assert allocate_costs(jitter,samples,{"claude-b":50})==costs
fault=allocate_costs(rows,[s|{"account":"claude-a"} for s in samples],{"claude-b":50})
assert sum(fault.values())!=5
plateau=[dict(ts=f"2026-10-12T12:{minute:02d}:00Z",provider="claude",account="a",kind="window",name="weekly_all",used_pct=used,resets_at="2026-10-15T00:00:00Z") for minute,used in ((0,91),(10,91),(20,91),(30,92),(40,92),(50,93))]
active=[dict(agent=agent,account="claude-a",ts=f"2026-10-12T12:{minute:02d}:00Z",weight=weight) for agent,minute,weight in (("plateau-a",5,1),("plateau-b",15,2),("step",25,1))]
points=allocate_costs(plateau[:4],active,{"claude-a":100})
assert sum(points.values())==1 and all(points.get(s["agent"],0)>0 for s in active),points
early=allocate_costs(plateau[:4],active[:2],{"claude-a":100})
assert sum(early.values())==1 and all(early.get(s["agent"],0)>0 for s in active[:2]),early
print('PASS [north#51] three-reading plateau gives every active run points and conserves its step')
claude_plateau=[r|dict(provider="claude",account="a",name="weekly_all",used_pct=r["used_pct"]-7) for r in plateau[:4]]
claude_active=[s|dict(account="claude-a") for s in active]
claude_points=allocate_costs(claude_plateau,claude_active,{"claude-a":100})
assert sum(claude_points.values())==1 and all(claude_points.get(s["agent"],0)>0 for s in claude_active),claude_points
print('PASS [north#51] Claude integer 84/84/84/85 plateau conserves its step and gives every plateau-only run points')
mixed=[dict(agent=m,account="claude-b",request=m,ts=t,weight=w) for m,t,w in (("claude-opus-5-5","2026-10-11T00:03:00Z",6400.0),("claude-haiku-5-5","2026-10-11T00:04:00Z",160.0),("claude-sonnet-9","2026-10-11T00:13:00Z",1600.0))]
share=allocate_costs(rows[:2],mixed[:2],{"claude-b":50})
assert abs(share["claude-opus-5-5"]/share["claude-haiku-5-5"]-40)<1e-9 and abs(sum(share.values())-2)<1e-10,share
print(f"PASS [north#35] Opus and Haiku list-price samples sharing one interval split plan $ 40:1 (${share['claude-opus-5-5']:.4f} vs ${share['claude-haiku-5-5']:.4f})")
os.environ["THREADS_DB"]=sys.argv[2]
threads=importlib.machinery.SourceFileLoader("ledger_threads_measure",sys.argv[3]).load_module()
threads.DB=sys.argv[2]
db=threads.connect()
import io
sys.stdin=io.StringIO("".join(json.dumps(row|dict(type="usage"))+"\n" for row in rows))
threads.cmd_ingest(db,None)
assert db.execute("SELECT COUNT(*) FROM usage").fetchone()[0]==3
records=[]
for agent in ("a","b"):
 records.append(dict(id=agent,account="claude-b",model="claude-opus-4-6",effort="high",fast=int(agent=="a"),usage_samples=[{k:v for k,v in sample.items() if k not in ("agent","account")} | {"request":str(n)} for n,sample in enumerate(samples) if sample["agent"]==agent],tier="high",actual_min=20,started="2026-10-11T00:00:00Z",ended="2026-10-11T00:20:00Z",outcome="done",landed=1,staffed="2026-10-11T00:00:00Z",landed_at="2026-10-11T00:20:00Z",brief="Item: north#2" if agent=="a" else "Item: wisp#3"))
sys.stdin=io.StringIO("".join(json.dumps(r)+"\n" for r in records))
running=records[1]|dict(status="running")
sys.stdin=io.StringIO(json.dumps(records[0])+"\n"+json.dumps(running)+"\n")
threads.cmd_ingest(db,None)
assert db.execute("SELECT COUNT(*) FROM runs").fetchone()[0]==1
assert abs(db.execute("SELECT SUM(cost_usd) FROM run_costs").fetchone()[0]-5)<1e-10
sys.stdin=io.StringIO(json.dumps(records[1])+"\n")
threads.cmd_ingest(db,None)
assert abs(db.execute("SELECT SUM(cost_usd) FROM runs").fetchone()[0]-5)<1e-10
assert db.execute("SELECT fast,COUNT(*) FROM runs GROUP BY fast").fetchall()[0][0]==0
metrics=delivery_metrics(db.execute("SELECT * FROM runs"),"north")
assert metrics==dict(done_boxes=1,concurrency=1,median_brief_to_landed_min=20,median_cost_per_done_usd=1.46),metrics
retried=[dict(dict(r),agent="a0",outcome="not done",landed=0,cost_usd=0.54,staffed="2026-10-10T23:40:00Z",started="2026-10-10T23:40:00Z",ended="2026-10-10T23:55:00Z",landed_at=None) for r in db.execute("SELECT * FROM runs WHERE agent='a'")]
assert delivery_metrics([*map(dict,db.execute("SELECT * FROM runs")),*retried],"north")["median_cost_per_done_usd"]==2.0
sys.stdin=io.StringIO("".join(json.dumps(r)+"\n" for r in records))
threads.cmd_ingest(db,None)
assert db.execute("SELECT COUNT(*) FROM run_usage").fetchone()[0]==3
prior=dict(db.execute("SELECT * FROM runs WHERE agent='a'").fetchone())
sys.stdin=io.StringIO(json.dumps(records[0]|dict(telemetry_only=True, role=None, actual_min=999, outcome="unclear", fast=0))+"\n")
threads.cmd_ingest(db,None)
after=dict(db.execute("SELECT * FROM runs WHERE agent='a'").fetchone())
assert after["fast"]==0 and all(after[k]==prior[k] for k in ("role","outcome","minutes","landed_at"))
print("PASS measured interval cost $5.00: all allocated to tracked runs, a zero-cost interval does not shrink later shares; a box's $ includes its failed attempt; fixture-secondary account claude-b; fast 0/1; replay idempotent; project delivery 20 min/$1.46")
PYMEASURE
python3 - "$repo" "$scratch/costs.db" <<'PYPRIORS'
import argparse, importlib.util, math, sqlite3, sys, tomllib
from pathlib import Path
root=Path(sys.argv[1])
spec=importlib.util.spec_from_file_location("defaults_measure",root/"harness/scripts/defaults-measure.py")
measure=importlib.util.module_from_spec(spec)
spec.loader.exec_module(measure)
with sqlite3.connect(sys.argv[2]) as rw:
    rw.execute("create table if not exists usage(ts, provider, kind, name, used_pct, resets_at)")
    rw.execute("create table if not exists model_events(model, kind, at)")
db=measure.open_db(sys.argv[2])
a=argparse.Namespace(state=None)
registry=tomllib.loads((root/"harness/defaults.toml").read_text())
expected={name:entry for name,entry in registry.items() if str(entry.get("source","")).startswith("prior: ")}
for name,entry in expected.items():
 n,value=getattr(measure,entry["settles_when"]["count"])(db,a)
 assert type(n) is int and n>=0,(name,n)
 assert value is None or isinstance(value,(int,float)) and math.isfinite(value),(name,value)
rows=list(measure.priors(db,a))
assert {row.split()[1] for row in rows}==set(expected),rows
print(f"PASS [north#50] every advertised prior evaluates with a nonnegative integer count and optional finite measurement ({len(rows)} priors)")
PYPRIORS
