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
chmod +x "$scratch/bin/claude" "$scratch/bin/codex"
export AGENTS_CLAUDE_BIN="$scratch/bin/claude" AGENTS_CODEX_BIN="$scratch/bin/codex"
export THREADS_DB="$scratch/threads.db" CODEX_CONFIG_DIR="$scratch/codex"
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
out="$(plan)"
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
[[ "$out" == *"box t1 (north tooling d30): codex:sol-medium on gpt-6-astra medium (capacity fallback)"* ]] || fail "capacity Astra: $out"
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
  'threads snapshot') HOME="$REMOTE_HOME" THREADS_DB="$REMOTE_DB" "$THREADS_BIN" snapshot ;;
  *) exit 97 ;;
esac
SH
chmod +x "$scratch/bin/ssh"
env -u THREADS_DB HOME="$scratch/usage-home" PATH="$scratch/bin:$PATH" AGENTS_OPERATOR_DIR="$scratch" REMOTE_HOME="$scratch/remote-home" REMOTE_DB="$scratch/remote.db" THREADS_BIN="$repo/bin/threads" python3 -I - "$repo/harness/lib" <<'DB'
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
printf 'ok: agents plan configuration, authentication, cash/pace and recorded routing decisions\n'
