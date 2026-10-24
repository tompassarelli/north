#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
agents="$repo/bin/agents"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agents-plan-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

mkdir -p "$scratch/bin"
cat >"$scratch/bin/claude" <<'SH'
#!/usr/bin/env bash
[[ "$*" == "auth status" ]] || exit 97
printf '{"loggedIn": %s}\n' "${FAKE_CLAUDE_IN:-true}"
SH
cat >"$scratch/bin/codex" <<'SH'
#!/usr/bin/env bash
[[ "$*" == "login status" ]] || exit 97
if [[ "${FAKE_CODEX_IN:-true}" == true ]]; then
  printf 'Logged in using ChatGPT\n' >&2
else
  printf 'Not logged in\n' >&2
  exit 1
fi
SH
cat >"$scratch/bin/update-status" <<'SH'
#!/usr/bin/env bash
printf 'STALE: machine-update fixture snapshot is stale\n'
SH
chmod +x "$scratch/bin/claude" "$scratch/bin/codex" "$scratch/bin/update-status"
export PATH="$scratch/bin:$PATH"
export AGENTS_CLAUDE_BIN="$scratch/bin/claude" AGENTS_CODEX_BIN="$scratch/bin/codex"
export THREADS_DB="$scratch/threads.db" CODEX_CONFIG_DIR="$scratch/codex"
export XDG_STATE_HOME="$scratch/state"
python3 -I - "$THREADS_DB" <<'DB'
import sqlite3,sys
sqlite3.connect(sys.argv[1]).close()
DB
cat >"$scratch/costs.toml" <<'TOML'
[[service]]
id = "claude-a"
kind = "ai-plan"
usd_per_week = 50
[[service]]
id = "codex"
kind = "ai-plan"
usd_per_week = 50
TOML

# Fixture with Tom's 9 Oct table, so editing the live file never breaks this.
fixture() {
  cat <<TOML
mode = "split"
${1:-}
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
agent = "worker-haiku"
role = "mechanical"
start = 0
[tiers."codex:luna-low"]
model = "gpt-6-luna"
effort = "low"
role = "mechanical, Codex side"
start = 20
[tiers."codex:sol-medium"]
model = "gpt-6.1-sol"
effort = "medium"
role = "middle"
start = 20
[tiers."codex:sol-high"]
model = "gpt-6.1-sol"
effort = "high"
role = "middle, harder half"
start = 50
[tiers."claude:opus-medium"]
model = "claude-opus-5-5"
effort = "medium"
agent = "worker"
role = "top"
start = 75
[tiers."claude:opus-high"]
model = "claude-opus-5-5"
effort = "high"
agent = "worker-high"
role = "planning and architecture"
start = 95
[on_request."codex:astra"]
model = "gpt-6-astra"
effort = "xhigh"
TOML
}
export AGENTS_ORCHESTRATION="$scratch/orchestration.toml"
fixture >"$AGENTS_ORCHESTRATION"
plan() { "$agents" plan --json "$@"; }
check() { jq -e "$1" <<<"$out" >/dev/null || fail "plan decision: $out"; }
cat >>"$AGENTS_ORCHESTRATION" <<'BURN'
[burn]
codex = { until = "2099-01-01T00:00:00Z", rate = "13%/h" }
BURN
for level in 2 4; do
  python3 - "$level" <<'PYBURN'
import json, os, sys
from pathlib import Path
path = Path(os.environ["XDG_STATE_HOME"]) / "agents/burn.json"
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps({"codex": {"policy": {"until": "2099-01-01T00:00:00Z", "rate": "13%/h", "target": 13.0},
                                     "parallel": int(sys.argv[1])}}))
PYBURN
  out="$(plan)"
  if [ "$level" -eq 2 ]; then
    allowance=2
    check '.burn.codex | .parallel == 2 and .model == "gpt-6.1-sol" and .effort == "high" and .fast == false and .calibrated == false and .advisory == true and .worker_ceiling == "pace-enforcement per-account allowed_workers"'
  else
    allowance=4
    check '.burn.codex | .parallel == 4 and .model == "gpt-6.1-sol" and .effort == "high" and .fast == false and .calibrated == false and .advisory == true and .worker_ceiling == "pace-enforcement per-account allowed_workers"'
  fi
  out="$("$agents" plan)"
  [[ "$out" == *"advisory parallel workers ${allowance} per sub-lead (uncalibrated recommendation); sole worker ceiling: pace-enforcement per-account allowed_workers"* ]] || fail "staffing allowance missing: $out"
done
sed -i 's/2099-01-01/2000-01-01/' "$AGENTS_ORCHESTRATION"
out="$(plan)"
check 'has("burn") | not'
printf 'PASS staffing burn boundary: current levers and uncalibrated allowance agree in text/JSON; expiry restores ordinary routing\n'
fixture >"$AGENTS_ORCHESTRATION"
out="$(plan 2>"$scratch/plan.stderr")"
[[ "$(cat "$scratch/plan.stderr")" == "STALE: machine-update fixture snapshot is stale" ]] || fail "update-status diagnostic missing from stderr: $(cat "$scratch/plan.stderr")"
check '.mode == "split" and .usable == ["claude","codex"] and .bands == [[0,20,"claude:haiku"],[20,50,"codex:sol-medium"],[50,75,"codex:sol-high"],[75,95,"claude:opus-medium"],[95,100,"claude:opus-high"]] and .escalation == ["claude:haiku","codex:luna-low","codex:sol-medium","codex:sol-high","claude:opus-medium","claude:opus-high"]'
fixture >"$AGENTS_ORCHESTRATION"
out="$(plan --mode codex-only)"
check '.usable == ["codex"] and .bands == [[0,20,"codex:luna-low"],[20,50,"codex:sol-medium"],[50,100,"codex:sol-high"]] and .escalation == ["codex:luna-low","codex:sol-medium","codex:sol-high"]'
out="$(plan --mode claude-only)"
check '.usable == ["claude"] and .bands == [[0,20,"claude:haiku"],[20,95,"claude:opus-medium"],[95,100,"claude:opus-high"]] and .escalation == ["claude:haiku","claude:opus-medium","claude:opus-high"]'
signed_out="$(FAKE_CODEX_IN=false plan)"
[ "$(jq -c '.bands' <<<"$signed_out")" = "$(jq -c '.bands' <<<"$out")" ] || fail "signed-out provider retained bands"
[ "$(jq -c '.usable' <<<"$signed_out")" = '["claude"]' ] || fail "signed-out provider usable"
if out="$(FAKE_CODEX_IN=false FAKE_CLAUDE_IN=false plan)"; then fail "plan succeeded without a provider"; fi
check '.usable == [] and .bands == [] and .escalation == []'
scenarios="$repo/harness/lib/routing-scenarios"
for scenario in burn conserve new-model deadline two-claude reserve thursday-behind capacity-astra capacity-floor; do
  out="$(plan --at "$scenarios/$scenario.json")" || fail "replay $scenario: $out"
  jq -e --slurpfile fixture "$scenarios/$scenario.json" '
    . as $actual | $fixture[0].expect as $expected |
    all(($expected.states // {}) | to_entries[]; $actual.accounts[.key].state == .value) and
    all(($expected.tiers // {}) | to_entries[]; $actual.boxes[.key].tier == .value) and
    all(($expected.fast // {}) | to_entries[]; $actual.boxes[.key].fast == .value)
  ' <<<"$out" >/dev/null || fail "replay decisions $scenario"
done
out="$("$agents" plan --at "$scenarios/thursday-behind.json")"
[[ "$out" == *"FLAG codex drain behind"* && "$out" == *"projected 45.0% left at Fri 23:59 ET (band 10-20)"* ]] || fail "Thursday45% projection: $out"
out="$("$agents" plan --at "$scenarios/reserve.json")"
[[ "$out" != *"FLAG codex drain behind"* && "$out" == *"projected 18.0% left at Fri 23:59 ET (band 10-20)"* ]] || fail "Thursday18% projection: $out"
out="$("$agents" plan --at "$scenarios/capacity-floor.json")"
[[ "$out" =~ "account codex: unavailable (provider capacity, since "[0-9]{2}:[0-9]{2}")" && "$out" == *"box t1 (north tooling d30): claude:haiku,"* && "$out" == *"account codex: state at or over pace, used 89"* ]] || fail "capacity floor: $out"
out="$("$agents" plan --at "$scenarios/capacity-astra.json")"
[[ "$out" == *"box t1 (north tooling d30): codex:sol-high on gpt-6-astra medium (capacity fallback)"* ]] || fail "capacity Astra: $out"
python3 -I - "$THREADS_DB" <<'DB'
import sqlite3, sys
from datetime import datetime, timedelta, timezone
now = datetime.now(timezone.utc)
iso = lambda d: d.strftime("%Y-%m-%dT%H:%M:%SZ")
db = sqlite3.connect(sys.argv[1])
db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT)")
rows = [("claude", "a", "window", "session", 0.0, 300, iso(now + timedelta(hours=4)), None),
        ("claude", "a", "window", "weekly_all", 0.0, 10080, iso(now + timedelta(days=4)), None),
        ("codex", "x1", "window", "week", 30.0, 10080, iso(now + timedelta(days=4)), None),
        ("codex", "x1", "billing", "hasCredits", None, None, None, 0.0)]
db.executemany("INSERT INTO usage VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'fixture')", [(iso(now), *r) for r in rows])
db.executemany("INSERT INTO usage VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
               [(iso(now - timedelta(hours=2)), "claude", "a", "grant", "cedar_ember/" + kind,
                 None, None, None, 1, "expired manual grant") for kind in ("full", "five_hour")])
db.commit()
DB
cat >"$AGENTS_ORCHESTRATION" <<'TOML'
mode = "split"
[projects.galileo]
priority = 1
share = 25
[accounts.codex]
target_left = 15
band = [10, 20]
thursday = true
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
start = 0
[tiers."codex:sol-medium"]
model = "gpt-6.1-sol"
effort = "medium"
start = 20
TOML
out="$(plan)"
check '.accounts.codex.used_pct == 30 and .accounts.codex.available_usd == 35 and .accounts.claude.available_usd == 50 and .accounts.codex.state == (if .accounts.codex.used_pct < .accounts.codex.pace_pct then "under pace" else "at or over pace" end) and .accounts.codex.resets_at != null and .projects.galileo.priority == 1 and .projects.galileo.share == 25 and (.projects.galileo | has("target") | not)'
readout="$("$agents" plan)"
[[ "$readout" == *"used 30.0%"* && "$readout" == *"pace "* && "$readout" == *"available \$35.00"* && "$readout" == *"resets_at "* && "$readout" == *"route: codex:sol-medium:"* ]] || fail "live cash/pace lines: $readout"
capacity_since="$(python3 -I - "$THREADS_DB" "$CODEX_CONFIG_DIR/sessions/rollout-capacity.jsonl" <<'DB'
import json, sqlite3, sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
now = datetime.now(timezone.utc)
since = (now - timedelta(minutes=5)).isoformat()
with sqlite3.connect(sys.argv[1]) as db:
    db.execute("UPDATE usage SET used_pct=93 WHERE provider='codex' AND kind='window'")
rollout = Path(sys.argv[2])
rollout.parent.mkdir(parents=True, exist_ok=True)
with rollout.open("w") as f:
    for kind, payload in [("turn_context", {"model": "gpt-6.1-sol", "effort": "medium"}),
                          ("event_msg", {"type": "task_complete", "error": {"codex_error_info": "server_overloaded", "message": "Selected model is at capacity"}})]:
        f.write(json.dumps({"timestamp": since, "type": kind, "payload": payload}) + "\n")
print(since)
DB
)"
readout="$("$agents" plan --mode codex-only)" || fail "empty available tiers text: $readout"
[[ "$readout" == *$'bands (difficulty percentile: start tier):\n'"  none: no provider available (claude dropped, mode is codex-only; codex capacity error since $capacity_since)"$'\nescalation:'* ]] || fail "empty available tiers reason: $readout"
out="$(plan --mode codex-only)" || fail "empty available tiers JSON: $out"
check '.bands == [] and .escalation == [] and .capacity.codex.fallback == null and .accounts.codex.used_pct == 93'
rm "$CODEX_CONFIG_DIR/sessions/rollout-capacity.jsonl"
python3 -I - "$THREADS_DB" <<'DB'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    db.execute("UPDATE usage SET used_pct=30 WHERE provider='codex' AND kind='window'")
DB
printf 'PASS empty available tiers: codex-only fresh capacity, no Astra headroom; text reason and JSON bands []\n'
python3 -I - "$THREADS_DB" <<'DB'
import sqlite3,sys
from datetime import datetime,timedelta,timezone
db=sqlite3.connect(sys.argv[1])
db.execute("UPDATE usage SET ts=? WHERE provider='codex'",((datetime.now(timezone.utc)-timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),))
db.commit()
DB
if out="$(plan 2>&1)"; then fail "stale readout accepted"; fi
[[ "$out" == *"account codex STALE"* && "$out" != *"price money"* ]] || fail "stale readout did not fail loudly: $out"
mkdir -p "$scratch/usage-home/.config/threads" "$scratch/usage-home/.local/state/threads" "$scratch/remote-home"
printf 'fixture-host\n' >"$scratch/usage-home/.config/threads/remote"
cat >"$scratch/bin/ssh" <<'SH'
#!/usr/bin/env bash
case "${!#}" in
  'threads ingest') HOME="$REMOTE_HOME" THREADS_DB="$REMOTE_DB" "$THREADS_BIN" ingest ;;
  'north threads snapshot-pages') HOME="$REMOTE_HOME" THREADS_DB="$REMOTE_DB" "$NORTH_BIN" threads snapshot-pages ;;
  *) exit 97 ;;
esac
SH
chmod +x "$scratch/bin/ssh"
env -u THREADS_DB HOME="$scratch/usage-home" PATH="$scratch/bin:$PATH" NORTH_BIN="${NORTH_PACE_BIN:-$repo/target/debug/north}" AGENTS_OPERATOR_DIR="$scratch" REMOTE_HOME="$scratch/remote-home" REMOTE_DB="$scratch/remote.db" THREADS_BIN="$repo/bin/threads" python3 -I - "$repo/harness/lib" <<'DB'
import sys,sqlite3
from pathlib import Path
sys.path.insert(0,sys.argv[1])
import usage
rows=[dict(zip(usage.COLUMNS,["2026-10-15T12:00:00Z","codex","x1","window","week",40,10080,"2026-10-17T04:00:00Z",None,"fixture"]))]
mirror=Path(usage.DB)
with sqlite3.connect(mirror) as db:
    usage.record(db,rows)
frozen=mirror.with_name("threads.db")
with sqlite3.connect(frozen) as db:
    usage.record(db,[dict(rows[0],used_pct=90)])
frozen_bytes=frozen.read_bytes()
with usage.open_readout_db() as db:
    assert usage.latest(db)[0]["used_pct"]==40
    usage.publish(db,[dict(rows[0],ts="2026-10-15T12:05:00Z",used_pct=45)])
    assert usage.latest(db)[0]["used_pct"]==45
with sqlite3.connect(mirror.as_uri()+"?mode=ro",uri=True) as db:
    assert usage.latest(db)[0]["used_pct"]==45
assert frozen.read_bytes()==frozen_bytes
print("PASS usage boundary: snapshot40% -> fresh45% -> authoritative ingest/mirror45%; frozen90% untouched")
DB
printf '[accounts.claude.profiles]\na = { "%s" = {} }\nb = { elsewhere = {} }\n' "$(hostname)" >"$scratch/hosted-orchestration.toml"
AGENTS_ORCHESTRATION="$scratch/hosted-orchestration.toml" python3 -I - "$repo/harness/lib" <<'DB'
import sys,sqlite3
sys.path.insert(0,sys.argv[1])
import usage
row=lambda account,pct: dict(zip(usage.COLUMNS,["2026-10-15T12:00:00Z","claude",account,"window","session",pct,300,"2026-10-15T15:00:00Z",None,"fixture"]))
db=sqlite3.connect(":memory:")
usage.record(db,[row("a",20),row("b",30),row("c",92),dict(row("x",5),provider="codex")])
assert sorted((r["provider"],r["account"]) for r in usage.latest(db))==[("claude","a"),("claude","b"),("codex","x")], usage.latest(db)
print("PASS hosted accounts: a Claude account with no profile on any host (retired claude:c) is left out of usage and plan, and one whose profile is on another host stays")
DB
mkdir -p "$scratch/goal/harness/lib" "$scratch/goal/bin"
cat >"$scratch/goal/bin/threads" <<'SH'
#!/usr/bin/env bash
[[ "$1 $2" == "calibrate --write" ]] || exit 97
exec sleep 6
SH
chmod +x "$scratch/goal/bin/threads"
PYTHONDONTWRITEBYTECODE=1 python3 -I - "$repo/harness/lib" "$scratch/goal/harness/lib" >"$scratch/goal.stdout" 2>"$scratch/goal.stderr" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import goal_eta
goal_eta.LIB = Path(sys.argv[2])
assert goal_eta.record({}) is None
PY
[[ ! -s "$scratch/goal.stdout" ]] || fail "calibration timeout changed stdout"
[[ "$(cat "$scratch/goal.stderr")" == "warning: threads calibrate --write timed out after 5s; goal ETA row not recorded" ]] || fail "calibration timeout warning: $(cat "$scratch/goal.stderr")"
printf 'PASS goal ETA calibration timeout: exit 0, one stderr warning, stdout unchanged\n'
printf 'ok: agents plan configuration, authentication, cash/pace and recorded routing decisions\n'
