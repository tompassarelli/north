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
cat >"$scratch/bin/update-status" <<'SH'
#!/usr/bin/env bash
printf 'STALE: machine-update fixture snapshot is stale\n'
SH
chmod +x "$scratch/bin/claude" "$scratch/bin/update-status"
export PATH="$scratch/bin:$PATH"
export AGENTS_CLAUDE_BIN="$scratch/bin/claude"
export THREADS_DB="$scratch/threads.db"
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
[tiers."gemini:3.8-flash-high"]
model = "gemini-3.8-flash"
effort = "high"
role = "middle, Gemini side"
start = 20
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
TOML
}
export AGENTS_ORCHESTRATION="$scratch/orchestration.toml"
fixture >"$AGENTS_ORCHESTRATION"
plan() { "$agents" plan --json "$@"; }
check() { jq -e "$1" <<<"$out" >/dev/null || fail "plan decision: $out"; }
fixture >"$AGENTS_ORCHESTRATION"
out="$(plan 2>"$scratch/plan.stderr")"
[[ "$(cat "$scratch/plan.stderr")" == "STALE: machine-update fixture snapshot is stale" ]] || fail "update-status diagnostic missing from stderr: $(cat "$scratch/plan.stderr")"
check '.mode == "split" and .usable == ["claude"] and .bands == [[0,20,"claude:haiku"],[20,95,"claude:opus-medium"],[95,100,"claude:opus-high"]] and .escalation == ["claude:haiku","claude:opus-medium","claude:opus-high"]'
split_bands="$(jq -c '.bands' <<<"$out")"
out="$("$agents" plan)"
[[ "$out" == *"gemini: dropped, not signed in (no sign-in check for this provider)"* ]] || fail "unchecked provider usable: $out"
out="$(plan --mode claude-only)"
check '.usable == ["claude"] and .bands == [[0,20,"claude:haiku"],[20,95,"claude:opus-medium"],[95,100,"claude:opus-high"]] and .escalation == ["claude:haiku","claude:opus-medium","claude:opus-high"]'
[ "$(jq -c '.bands' <<<"$out")" = "$split_bands" ] || fail "signed-out provider retained bands"
if out="$(plan --mode gemini-only)"; then fail "plan succeeded with only an unchecked provider"; fi
check '.usable == [] and .bands == [] and .escalation == []'
if out="$(FAKE_CLAUDE_IN=false plan)"; then fail "plan succeeded without a provider"; fi
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
        ("claude", "a", "window", "weekly_all", 30.0, 10080, iso(now + timedelta(days=4)), None),
        ("claude", "a", "billing", "hasCredits", None, None, None, 0.0)]
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
[accounts.claude]
target_left = 15
band = [10, 20]
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
start = 0
[tiers."claude:opus-medium"]
model = "claude-opus-5-5"
effort = "medium"
start = 20
TOML
out="$(plan)"
check '.accounts.claude.used_pct == 30 and .accounts.claude.available_usd == 35 and .accounts.claude.state == (if .accounts.claude.used_pct < .accounts.claude.pace_pct then "under pace" else "at or over pace" end) and .accounts.claude.resets_at != null and .projects.galileo.priority == 1 and .projects.galileo.share == 25 and (.projects.galileo | has("target") | not)'
readout="$("$agents" plan)"
[[ "$readout" == *"used 30.0%"* && "$readout" == *"pace "* && "$readout" == *"available \$35.00"* && "$readout" == *"resets_at "* && "$readout" == *"route: claude:"* ]] || fail "live cash/pace lines: $readout"
python3 -I - "$THREADS_DB" <<'DB'
import sqlite3,sys
from datetime import datetime,timedelta,timezone
db=sqlite3.connect(sys.argv[1])
db.execute("UPDATE usage SET ts=? WHERE provider='claude'",((datetime.now(timezone.utc)-timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),))
db.commit()
DB
if out="$(plan 2>&1)"; then fail "stale readout accepted"; fi
[[ "$out" == *"account claude STALE"* && "$out" != *"price money"* ]] || fail "stale readout did not fail loudly: $out"
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
rows=[dict(zip(usage.COLUMNS,["2026-10-15T12:00:00Z","gemini","x1","window","week",40,10080,"2026-10-17T04:00:00Z",None,"fixture"]))]
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
usage.record(db,[row("a",20),row("b",30),row("c",92),dict(row("x",5),provider="gemini")]*2)
assert sorted((r["provider"],r["account"]) for r in usage.latest(db))==[("claude","a"),("claude","b"),("gemini","x")] and db.execute("SELECT count(*) FROM usage").fetchone()[0]==4, usage.latest(db)
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
