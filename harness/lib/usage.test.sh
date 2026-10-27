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
policy = scratch / "accounts.toml"
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
import sqlite3
retired_db = scratch / "retired.db"
week = lambda provider, account, pct: {"ts": "2026-10-27T10:00:00Z", "provider": provider, "account": account, "kind": "window",
                                       "name": "week", "used_pct": pct, "window_min": 10080, "resets_at": "2099-01-01T00:00:00Z", "source": "fixture"}
with sqlite3.connect(retired_db) as db:
    usage.record(db, [week("codex", "main", 40), week("gemini", "main", 10)])
stub = scratch / "north-stub"
stub.write_text("#!/usr/bin/env python3\nimport json, sys\nprint('null' if json.load(sys.stdin)['op'] == 'money_refusal' else '{}')\n")
stub.chmod(0o755)
gate_state = scratch / "retired-state"
(gate_state / "agents").mkdir(parents=True)
(gate_state / "agents/usage-gate.json").write_text(json.dumps({"codex": {"refusal": None}}))
retired_env = dict(os.environ, THREADS_DB=str(retired_db), XDG_STATE_HOME=str(gate_state), NORTH_PACE_BIN=str(stub),
                   AGENTS_OPERATOR_DIR=str(scratch / "op"))
shown = json.loads(subprocess.run(["python3", "-I", str(here / "usage.py"), "--json"], env=retired_env,
                                  capture_output=True, text=True, timeout=10, check=True).stdout)
assert [r["provider"] for r in shown] == ["gemini"], shown
table = subprocess.run(["python3", "-I", str(here / "usage.py")], env=retired_env, capture_output=True, text=True, timeout=10).stdout
assert "codex" not in table and "gemini main" in table, table
gated = json.loads(subprocess.run(["python3", "-I", str(here / "usage.py"), "--write-pace-gate"], env=retired_env,
                                  capture_output=True, text=True, timeout=20, check=True).stdout)
assert sorted(gated) == ["claude", "gemini"], gated
assert "codex" not in json.loads((gate_state / "agents/usage-gate.json").read_text())
print("PASS a retired provider's stored codex readings are not shown by --json or the table, nor gated by --write-pace-gate")

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
rows = [{'type':'usage', 'ts':'2026-10-13T00:00:00Z', 'provider':'gemini', 'account':'test',
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
