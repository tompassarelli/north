#!/usr/bin/env bash
# Golden: agents usage prints one line per non-AI cost service and the weekly operating total.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/op" "$tmp/state/agents/costs"
cat >"$tmp/op/costs.toml" <<'EOF'
[[service]]
id = "claude-a"
kind = "ai-plan"
billing = "fixed"
usd_per_week = 50.0
use = "dev"
projects = {}
readout = "agents usage"

[[service]]
id = "cloud-credit"
kind = "ai-plan"
billing = "prepaid"
usd_per_week = 0.0
initial = 250.0
expires = 2099-11-05T07:59:00Z
use = "dev"
projects = {}
readout = "manual"

[[service]]
id = "digitalocean"
kind = "cloud"
billing = "metered"
usd_per_week = 0.0
use = "dev"
projects = {}
readout = "x"

[[service]]
id = "hetzner-gex45"
kind = "cloud"
billing = "fixed"
usd_per_week = 40.0
use = "dev"
projects = {}
readout = "x"

[[service]]
id = "rental"
kind = "gpu-rental"
billing = "prepaid"
usd_per_week = 0.0
use = "dev"
projects = {}
readout = "x"
EOF
cat >"$tmp/state/agents/costs/rental.json" <<'EOF'
{"service":"rental","period_start":"2026-10-01T00:00:00Z","period_end":"2026-10-15T00:00:00Z","usd_spent_period":20.0,"usd_balance":14.5,"credits_left":null,"read_at":"2026-10-12T05:00:00Z","source":"invoices"}
EOF
# Mid-month: spend to 2026-10-12T07:12Z over a whole-month period is prorated by the 11.3 days elapsed, not the 31-day month.
cat >"$tmp/state/agents/costs/digitalocean.json" <<'EOF'
{"service":"digitalocean","period_start":"2026-10-01T00:00:00+00:00","period_end":"2026-11-01T00:00:00+00:00","usd_spent_period":18.33,"usd_balance":18.33,"credits_left":0.0,"read_at":"2026-10-12T07:12:00+00:00","source":"do"}
EOF
echo '{"usd_spent_period":20.0}' >"$tmp/state/agents/costs/cloud-credit.json"
out="$(AGENTS_OPERATOR_DIR="$tmp/op" XDG_STATE_HOME="$tmp/state" THREADS_DB="$tmp/threads.db" python3 -I "$HERE/usage.py" | sed -E 's/, [0-9]+ min ago\)/)/')"
want='agents usage: no snapshot yet; run `agents usage --refresh`
cloud-credit (ai-plan, prepaid): $230.00 left of $250.00, expires 2099-11-05T07:59Z (readout: manual)
digitalocean (cloud, metered): $18.33 spent 2026-10-01..2026-11-01, balance $18.33, credits left 0 (do)
hetzner-gex45 (cloud, fixed): no readout yet
rental (gpu-rental, prepaid): $20.00 spent 2026-10-01..2026-10-15, balance $14.50 (invoices)
weekly operating $113.85 = fixed $90.00 + metered $23.85'
if [ "$out" != "$want" ]; then
  diff <(echo "$want") <(echo "$out")
  exit 1
fi
rm "$tmp/state/agents/costs/cloud-credit.json"
out="$(AGENTS_OPERATOR_DIR="$tmp/op" XDG_STATE_HOME="$tmp/state" THREADS_DB="$tmp/threads.db" python3 "$HERE/usage.py" | grep '^cloud-credit')"
want='cloud-credit (ai-plan, prepaid): no readout, unknown left of $250.00, expires 2099-11-05T07:59Z (readout: manual)'
[ "$out" = "$want" ] || { diff <(echo "$want") <(echo "$out"); exit 1; }
PYTHONDONTWRITEBYTECODE=1 python3 - "$HERE" "$tmp" <<'PYTEST' || exit 1
import json, os, subprocess, sys
from pathlib import Path
here, scratch = map(Path, sys.argv[1:])
policy = scratch / "burn.toml"
env = dict(os.environ, AGENTS_ORCHESTRATION=str(policy), THREADS_DB=str(scratch / "burn.db"))
active = {"until": "2099-01-01T00:00:00Z", "rate": "13%/h"}
cases = [("active", active, True), ("expired", active | {"until": "2000-01-01T00:00:00Z"}, False),
         ("absent", None, False), ("bad date", active | {"until": "later"}, False),
         ("naive date", active | {"until": "2099-01-01T00:00:00"}, False),
         ("fixed removed", {"until": active["until"], "fast": True, "model": "gpt-6-astra", "effort": "high"}, False),
         ("fixed mixed", active | {"fast": True}, False), ("bad rate", active | {"rate": 13}, False),
         ("bad entry", "invalid", False)]
for label, entry, on in cases:
    value = "{ " + ", ".join(f"{k} = {json.dumps(v)}" for k,v in entry.items()) + " }" if isinstance(entry, dict) else json.dumps(entry)
    policy.write_text("[burn]\ncodex = " + value + "\n" if entry is not None else "")
    run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--burn", "codex"], env=env, capture_output=True, text=True, timeout=10)
    assert (run.returncode, run.stdout, run.stderr) == (0 if on else 1, "gpt-6.1-sol high\n" if on else "", ""), (label, run)
    run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--fast", "codex", "--cached"], env=env, capture_output=True, text=True, timeout=10)
    assert (run.returncode, run.stdout, run.stderr) == (1, "off\n", ""), (label, run)
for content in ("burn = 3\n", "[burn\n", None):
    if content is None:
        policy.unlink()
    else:
        policy.write_text(content)
    run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--burn", "codex"], env=env, capture_output=True, text=True, timeout=10)
    assert (run.returncode, run.stdout, run.stderr) == (1, "", ""), run
print(f"PASS burn policy public boundary: {len(cases) + 3} cases, cached and uncached fast")
sys.path.insert(0, str(here))
import usage, usage_claude
from ledger_context import account_id
from unittest.mock import patch
policy.write_text('[accounts.claude]\nlabels = { "9b3a7fb5" = "a", "b04cb253" = "b" }\n')
os.environ.update(AGENTS_ORCHESTRATION=str(policy), CLAUDE_CONFIG_DIR=str(scratch))
cache_path = scratch / ".claude.json"
caches = {label: {"accountUuid": uuid, "utilization": {"limits": [
    {"kind": "weekly_all", "group": "weekly", "percent": pct, "resets_at": reset}]}}
    for label, uuid, pct, reset in (("a", "fixture-primary", 97, "2026-10-15T02:00:00Z"),
                                   ("b", "fixture-secondary", 98, "2026-10-17T12:00:00Z"))}
for host in ("whiterabbit", "nexus"):
    with patch("socket.gethostname", return_value=host):
        for login, cache in caches.items():
            cache_path.write_text(json.dumps({"cachedUsageUtilization": cache}))
            assert usage.local_account("claude") == login
            assert account_id("claude") == "claude-" + login
            for label, other_cache in caches.items():
                weekly = next(r for r in usage_claude.rows(other_cache, "2026-10-13T00:00:00Z") if r["kind"] == "window")
                assert weekly["account"] == label and weekly["used_pct"] == (97 if label == "a" else 98), weekly
print("PASS account labels follow the login across whiterabbit and nexus, including other-account readings")
from datetime import datetime, timedelta, timezone
import random, sqlite3
os.environ.update(AGENTS_ORCHESTRATION=str(policy), XDG_STATE_HOME=str(scratch / "controller-state"))
now = datetime(2026, 10, 13, 1, 0, tzinfo=timezone.utc)
rate_entry = {"until": "2099-01-01T00:00:00Z", "rate": "13%/h"}
def configure(entry):
    policy.write_text("[burn]\ncodex = { " + ", ".join(f"{k} = {json.dumps(v)}" for k,v in entry.items()) + " }\n")
for rate in ("13%/h", "1.33%/h", "0%/h", "-1%/h", "nan%/h", "13", 13, None):
    configure(rate_entry | {"rate": rate})
    parsed = usage.burn_policy("codex", now)
    assert bool(parsed) == (rate in ("13%/h", "1.33%/h")), (rate, parsed)
rng = random.Random(61)
transitions = 0
for _ in range(20):
    target = rng.uniform(0.01, 100)
    for measured, direction in ((target * .99, 1), (target, 0), (target * 1.19, 0),
                                (target * 1.21, -1), (None, 0)):
        for allowance in (2, 3, 7, 100):
            expected = max(2, allowance + direction)
            assert usage.burn_decide(allowance, measured, target) == expected, (allowance, measured, target)
            transitions += 1
print(f"PASS burn decision property: {transitions} seeded transitions")
configure(rate_entry)
db = sqlite3.connect(scratch / "controller.db")
reset = "2026-10-19T15:29:10Z"
def sample(at, pct, db=db, reset=reset):
    usage.record(db, [{"ts": at.isoformat().replace("+00:00", "Z"), "provider": "codex", "account": "main",
                      "kind": "window", "name": "week", "used_pct": pct, "window_min": 10080, "resets_at": reset}])
sample(now - timedelta(minutes=10), 10)
sample(now, 10)
lines = usage.burn_step(db, "codex", now)
assert len(lines) == 1 and "parallel 2 -> 3; measured 0.00%/h target 13%/h; window week 10.00min" in lines[0], lines
assert usage.burn_step(db, "codex", now + timedelta(minutes=2)) == [], "same snapshot moved twice"
sample(now + timedelta(minutes=2), 10)
assert usage.burn_step(db, "codex", now + timedelta(minutes=2)) == [], "raised before full window"
sample(now + timedelta(minutes=10), 10)
assert "parallel 3 -> 4;" in usage.burn_step(db, "codex", now + timedelta(minutes=10))[0]
sample(now + timedelta(minutes=12), 20)
assert "parallel 4 -> 3;" in usage.burn_step(db, "codex", now + timedelta(minutes=12))[0], "excess waited for cadence"
assert usage.burn_step(db, "codex", now + timedelta(hours=2)) == [], "stale reading raised"
configure(rate_entry | {"rate": "1.33%/h"})
assert usage.burn_override("codex")["parallel"] == 2, "new target reused previous allowance"
assert usage.burn_step(db, "codex", now + timedelta(hours=2)) == [], "new target raised from stale data"
configure(rate_entry | {"until": "2000-01-01T00:00:00Z"})
assert usage.burn_override("codex") is None
assert usage.burn_step(db, "codex", now) and "codex" not in usage.burn_state()
configure(rate_entry)
empty = sqlite3.connect(":memory:")
assert usage.burn_step(empty, "codex", now) == []
assert usage.burn_override("codex")["fast"] is False
print("PASS burn snapshot journey: +1 workers, full window between rises, immediate excess, stale/missing, target change, expiry")
recorded = [("01:21:12",34), ("01:22:51",34), ("01:30:01",38), ("01:30:05",38),
            ("01:31:43",40), ("01:35:32",51), ("01:38:12",64), ("01:38:27",65),
            ("01:41:14",79), ("01:44:32",95), ("01:48:20",100)]
replay = sqlite3.connect(":memory:")
start = datetime(2026,10,13,1,35,32,tzinfo=timezone.utc)
configure(rate_entry | {"rate": "15%/h"})
entry = usage.burn_policy("codex", start)
usage.burn_state_path().write_text(json.dumps({"codex": {"policy": entry, "parallel": 4, "at": start.timestamp()}}))
remaining = [(datetime.fromisoformat("2026-10-13T" + t + "+00:00"), pct) for t,pct in recorded]
first_down = None
for minutes in range(0,15,2):
    at = start + timedelta(minutes=minutes)
    while remaining and remaining[0][0] <= at:
        t,pct = remaining.pop(0)
        sample(t,pct,replay)
    lines = usage.burn_step(replay,"codex",at)
    if lines and "parallel 4 -> 3" in lines[0]:
        first_down = minutes
        break
assert first_down is not None and first_down <= 4, ("recorded51->100 stepdown exceeds4min",first_down)
print(f"PASS recorded2026-10-13 usage DB51->100 replay: stepdown after {first_down}min")
measure = sqlite3.connect(":memory:")
sample(now - timedelta(hours=3),0,measure)
sample(now - timedelta(minutes=10),10,measure, "2026-10-19T15:29:11Z")
sample(now,12,measure)
reading = usage.burn_measure(measure,"codex",now)
assert reading and reading["measured"] == 12 and reading["seconds"] == 600, ("long average or jitter broke measurement",reading)
print("PASS short window: snapshot delta ignores3h average and handles observed1s reset jitter")
configure(rate_entry)
report_path = scratch / "report.db"
report_db = sqlite3.connect(report_path)
real_now = datetime.now(timezone.utc)
sample(real_now - timedelta(minutes=10),10,report_db,f"{real_now + timedelta(days=3):%Y-%m-%dT%H:%M:%SZ}")
sample(real_now,12,report_db,f"{real_now + timedelta(days=3):%Y-%m-%dT%H:%M:%SZ}")
report_db.close()
import hashlib
state_before = usage.burn_state_path().read_bytes()
db_before = hashlib.sha256(report_path.read_bytes()).hexdigest()
run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--burn-report", "codex"],
                     env=dict(os.environ,THREADS_DB=str(report_path)), capture_output=True,text=True,timeout=10)
assert run.returncode == 0 and "parallel 2 -> 2; measured 12.00%/h target 13%/h;" in run.stdout and "read-only" in run.stdout and not run.stderr, run
assert usage.burn_state_path().read_bytes() == state_before, "read-only report changed controller state"
assert hashlib.sha256(report_path.read_bytes()).hexdigest() == db_before, "read-only report changed snapshot database"
print("PASS read-only report public boundary: real-format snapshot delta with unchanged database/controller bytes")
def control():
    run = subprocess.run(["python3","-I",str(here / "usage.py"),"--burn-step","codex"],
                         env=dict(os.environ, THREADS_DB=str(report_path)),capture_output=True,text=True,timeout=10)
    assert run.returncode == 0 and not run.stderr, run
control()
configure(rate_entry | {"until":"2000-01-01T00:00:00Z"})
control()
assert "codex" not in usage.burn_state(), "expiry kept advisory"
configure(rate_entry)
control()
policy.write_text("[burn\n")
control()
assert "codex" not in usage.burn_state(), "malformed burn kept advisory"
configure(rate_entry)
control()
policy.unlink()
control()
assert "codex" not in usage.burn_state(), "missing burn kept advisory"
print("PASS sweep advisory step public boundary: expiry/malformed/missing release without a timer")

PYTEST
PYTHONDONTWRITEBYTECODE=1 python3 - "$HERE" "$tmp" <<'PYRECOVERY' || exit 1
import json, os, sqlite3, subprocess, sys
from pathlib import Path
here, scratch = map(Path, sys.argv[1:]); repo = here.parents[1]
home = scratch / 'recovery-home'; state = home / '.local/state/threads'; state.mkdir(parents=True)
conf = home / '.config/threads'; conf.mkdir(parents=True); (conf / 'remote').write_text('fixture')
transport = scratch / 'recovery-bin'; transport.mkdir()
canonical = scratch / 'recovery-canonical.db'
(scratch / 'costs.toml').write_text('')
rows = [{'type':'usage', 'ts':'2026-10-13T00:00:00Z', 'provider':'fixture', 'account':'test',
         'kind':'window', 'name':'week', 'used_pct':12, 'window_min':10080, 'source':'recorded fixture'}]
subprocess.run([str(repo / 'bin/threads'), 'ingest'], env=dict(os.environ, THREADS_DB=str(canonical), AGENTS_OPERATOR_DIR=str(scratch)),
               input=''.join(json.dumps(r)+'\n' for r in rows), text=True, check=True, capture_output=True)
trace = scratch / 'recovery-trace'
ssh = transport / 'ssh'; ssh.write_text('''#!/usr/bin/env python3
import os, subprocess, sys
from pathlib import Path
assert sys.argv[-1] == 'north threads snapshot-pages'
with open(os.environ['RECOVERY_TRACE'], 'a') as log: log.write('refresh\\n')
if os.environ.get('BAD_STREAM'):
    sys.stdout.buffer.write(b'SQLite format 3\\0' + b'x' * 600); sys.exit(0)
r = subprocess.run([os.environ['NORTH_BIN'], 'threads', 'snapshot-pages'], env=dict(os.environ, THREADS_DB=os.environ['CANONICAL']), capture_output=True)
sys.stdout.buffer.write(r.stdout); sys.exit(r.returncode)
'''); ssh.chmod(0o755)
env = dict(os.environ, HOME=str(home), PATH=str(transport)+':'+os.environ['PATH'],
           PUBLIC_THREADS=str(repo / 'bin/threads'), CANONICAL=str(canonical), RECOVERY_TRACE=str(trace), NORTH_BIN=os.environ.get('NORTH_PACE_BIN') or str(repo / 'target/debug/north'))
env.pop('THREADS_DB', None)
snapshot = state / 'snapshot.db'
def corrupt(): snapshot.write_bytes(b'SQLite format 3\0' + b'x' * 600)
def readout(env):
    return subprocess.run([str(repo / 'bin/agents'), 'usage', '--json'], env=env, capture_output=True, text=True, timeout=10)
corrupt(); result = readout(env)
assert result.returncode == 0 and json.loads(result.stdout)[0]['used_pct'] == 12, result
assert trace.read_text().splitlines() == ['refresh'], trace.read_text()
with sqlite3.connect(snapshot) as db: assert db.execute('PRAGMA quick_check').fetchall() == [('ok',)]
trace.write_text(''); corrupt(); before = snapshot.read_bytes()
result = readout(dict(env, BAD_STREAM='1'))
assert result.returncode != 0 and 'snapshot still unreadable after one refresh' in result.stderr, result
assert 'Traceback' not in result.stderr, result.stderr
assert trace.read_text().splitlines() == ['refresh'], trace.read_text()
assert snapshot.read_bytes() == before
print('PASS 1203 recorded: agents usage repairs a corrupt snapshot once and reports persistent corruption after one retry')
PYRECOVERY
echo "usage.test.sh: ok"
