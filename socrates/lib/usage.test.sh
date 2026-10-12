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
echo '{"usd_spent_period":20.0}' >"$tmp/state/agents/costs/claude-b-cloud-credit.json"
out="$(AGENTS_OPERATOR_DIR="$tmp/op" XDG_STATE_HOME="$tmp/state" THREADS_DB="$tmp/threads.db" python3 -I "$HERE/usage.py" | sed -E 's/, [0-9]+ min ago\)/)/')"
want='agents usage: no snapshot yet; run `agents usage --refresh`
claude-b-cloud-credit (ai-plan, prepaid): $230.00 left of $250.00, expires 2099-11-05T07:59Z (readout: manual)
hetzner-gex45 (cloud, fixed): no readout yet
vast (gpu-rental, prepaid): $20.00 spent 2026-10-01..2026-10-15, balance $14.50 (vastai show invoices)
weekly operating $100.00 = fixed $90.00 + metered $10.00'
if [ "$out" != "$want" ]; then
  diff <(echo "$want") <(echo "$out")
  exit 1
fi
echo "usage.test.sh: ok"
