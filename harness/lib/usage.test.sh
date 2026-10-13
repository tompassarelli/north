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
id = "claude-b-cloud-credit"
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
id = "vast"
kind = "gpu-rental"
billing = "prepaid"
usd_per_week = 0.0
use = "dev"
projects = {}
readout = "x"
EOF
cat >"$tmp/state/agents/costs/vast.json" <<'EOF'
{"service":"vast","period_start":"2026-10-01T00:00:00Z","period_end":"2026-10-15T00:00:00Z","usd_spent_period":20.0,"usd_balance":14.5,"credits_left":null,"read_at":"2026-10-12T05:00:00Z","source":"vastai show invoices"}
EOF
# Mid-month: spend to 2026-10-12T07:12Z over a whole-month period is prorated by the 11.3 days elapsed, not the 31-day month.
cat >"$tmp/state/agents/costs/digitalocean.json" <<'EOF'
{"service":"digitalocean","period_start":"2026-10-01T00:00:00+00:00","period_end":"2026-11-01T00:00:00+00:00","usd_spent_period":18.33,"usd_balance":18.33,"credits_left":0.0,"read_at":"2026-10-12T07:12:00+00:00","source":"do"}
EOF
echo '{"usd_spent_period":20.0}' >"$tmp/state/agents/costs/claude-b-cloud-credit.json"
out="$(AGENTS_OPERATOR_DIR="$tmp/op" XDG_STATE_HOME="$tmp/state" THREADS_DB="$tmp/threads.db" python3 -I "$HERE/usage.py" | sed -E 's/, [0-9]+ min ago\)/)/')"
want='agents usage: no snapshot yet; run `agents usage --refresh`
claude-b-cloud-credit (ai-plan, prepaid): $230.00 left of $250.00, expires 2099-11-05T07:59Z (readout: manual)
digitalocean (cloud, metered): $18.33 spent 2026-10-01..2026-11-01, balance $18.33, credits left 0 (do)
hetzner-gex45 (cloud, fixed): no readout yet
vast (gpu-rental, prepaid): $20.00 spent 2026-10-01..2026-10-15, balance $14.50 (vastai show invoices)
weekly operating $113.85 = fixed $90.00 + metered $23.85'
if [ "$out" != "$want" ]; then
  diff <(echo "$want") <(echo "$out")
  exit 1
fi
rm "$tmp/state/agents/costs/claude-b-cloud-credit.json"
out="$(AGENTS_OPERATOR_DIR="$tmp/op" XDG_STATE_HOME="$tmp/state" THREADS_DB="$tmp/threads.db" python3 "$HERE/usage.py" | grep '^claude-b-cloud-credit')"
want='claude-b-cloud-credit (ai-plan, prepaid): no readout, unknown left of $250.00, expires 2099-11-05T07:59Z (readout: manual)'
[ "$out" = "$want" ] || { diff <(echo "$want") <(echo "$out"); exit 1; }
PYTHONDONTWRITEBYTECODE=1 python3 - "$HERE" "$tmp" <<'PYTEST'
import json, os, subprocess, sys
from pathlib import Path
here, scratch = map(Path, sys.argv[1:])
policy = scratch / "burn.toml"
env = dict(os.environ, AGENTS_ORCHESTRATION=str(policy), THREADS_DB=str(scratch / "burn.db"))
active = {"until": "2099-01-01T00:00:00Z", "fast": True, "model": "gpt-6-astra", "effort": "high"}
cases = [("active", active, True), ("expired", active | {"until": "2000-01-01T00:00:00Z"}, False),
         ("absent", None, False), ("bad date", active | {"until": "later"}, False),
         ("naive date", active | {"until": "2099-01-01T00:00:00"}, False),
         ("bad model", active | {"model": 42}, False), ("missing effort", {k:v for k,v in active.items() if k != "effort"}, False),
         ("bad fast", active | {"fast": "true"}, False), ("bad entry", "invalid", False)]
for label, entry, on in cases:
    value = "{ " + ", ".join(f"{k} = {json.dumps(v)}" for k,v in entry.items()) + " }" if isinstance(entry, dict) else json.dumps(entry)
    policy.write_text("[burn]\ncodex = " + value + "\n" if entry is not None else "")
    run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--burn", "codex"], env=env, capture_output=True, text=True, timeout=10)
    assert (run.returncode, run.stdout, run.stderr) == (0 if on else 1, "gpt-6-astra high\n" if on else "", ""), (label, run)
    run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--fast", "codex", "--cached"], env=env, capture_output=True, text=True, timeout=10)
    assert (run.returncode, run.stdout, run.stderr) == (0 if on else 1, "on\n" if on else "off\n", ""), (label, run)
    if on:
        run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--fast", "codex"], env=env, capture_output=True, text=True, timeout=10)
        assert (run.returncode, run.stdout, run.stderr) == (0, "on\n", ""), run
for content in ("burn = 3\n", "[burn\n", None):
    if content is None:
        policy.unlink()
    else:
        policy.write_text(content)
    run = subprocess.run(["python3", "-I", str(here / "usage.py"), "--burn", "codex"], env=env, capture_output=True, text=True, timeout=10)
    assert (run.returncode, run.stdout, run.stderr) == (1, "", ""), run
print(f"PASS burn policy public boundary: {len(cases) + 3} cases, cached and uncached fast")
PYTEST
echo "usage.test.sh: ok"
