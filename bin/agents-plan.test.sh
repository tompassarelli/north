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
check '.mode == "split" and .usable == ["claude","codex"] and .bands == [[0,20,"claude:haiku"],[20,50,"codex:sol-medium"],[50,75,"codex:sol-high"],[75,95,"claude:opus-medium"],[95,100,"claude:opus-high"]] and .escalation == ["claude:haiku","codex:sol-medium","codex:sol-high","claude:opus-medium","claude:opus-high"]'
fixture >"$AGENTS_ORCHESTRATION"
out="$(plan --mode codex-only)"
check '.usable == ["codex"] and .bands == [[0,50,"codex:sol-medium"],[50,100,"codex:sol-high"]] and .escalation == ["codex:sol-medium","codex:sol-high"]'
out="$(plan --mode claude-only)"
check '.usable == ["claude"] and .bands == [[0,20,"claude:haiku"],[20,95,"claude:opus-medium"],[95,100,"claude:opus-high"]] and .escalation == ["claude:haiku","claude:opus-medium","claude:opus-high"]'
signed_out="$(FAKE_CODEX_IN=false plan)"
[ "$(jq -c '.bands' <<<"$signed_out")" = "$(jq -c '.bands' <<<"$out")" ] || fail "signed-out provider retained bands"
[ "$(jq -c '.usable' <<<"$signed_out")" = '["claude"]' ] || fail "signed-out provider usable"
if out="$(FAKE_CODEX_IN=false FAKE_CLAUDE_IN=false plan)"; then fail "plan succeeded without a provider"; fi
check '.usable == [] and .bands == [] and .escalation == []'
scenarios="$repo/socrates/lib/routing-scenarios"
for scenario in burn conserve new-model deadline; do
  out="$(plan --at "$scenarios/$scenario.json")" || fail "replay $scenario: $out"
  jq -e --slurpfile fixture "$scenarios/$scenario.json" '
    . as $actual | $fixture[0].expect as $expected |
    all(($expected.states // {}) | to_entries[]; $actual.accounts[.key].state == .value) and
    all(($expected.tiers // {}) | to_entries[]; $actual.boxes[.key].tier == .value) and
    all(($expected.fast // {}) | to_entries[]; $actual.boxes[.key].fast == .value)
  ' <<<"$out" >/dev/null || fail "replay decisions $scenario"
done
out="$(plan --at "$scenarios/burn.json" --state codex=conserve)"
check '.accounts.codex.state == "conserve"'

# Live: an expired override prints as expired and is ignored; --state overrides one account for one run.
export THREADS_DB="$scratch/threads.db"
python3 -I - "$THREADS_DB" <<'PY'
import sqlite3, sys
from datetime import datetime, timedelta, timezone
now = datetime.now(timezone.utc)
iso = lambda d: d.strftime("%Y-%m-%dT%H:%M:%SZ")
db = sqlite3.connect(sys.argv[1])
db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT)")
rows = [("claude", "c1", "window", "session", 0.0, 300, iso(now + timedelta(hours=4)), None),
        ("claude", "c1", "window", "weekly_all", 0.0, 10080, iso(now + timedelta(days=4)), None),
        ("codex", "x1", "window", "week", 30.0, 10080, iso(now + timedelta(days=4)), None),
        ("codex", "x1", "billing", "hasCredits", None, None, None, 0.0)]
db.executemany("INSERT INTO usage VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'fixture')", [(iso(now), *r) for r in rows])
db.commit()
PY
cat >"$AGENTS_ORCHESTRATION" <<'TOML'
mode = "split"
usage = { burn_slack = 15, conserve_slack = -5, horizon_h = 36, money_ceiling = 95, fast_floor = 10, stale_min = 30, urgent = 1.2, shift = 15, late = 1.5 }
[projects.galileo]
priority = 1
[accounts.codex]
state = "burn"
until = "2026-01-01T00:00Z"
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
check '(.accounts.codex.state == "even" or .accounts.codex.state == "conserve") and .projects.galileo.priority == 1 and (.projects.galileo | has("target") | not)'
base_claude="$(jq -c '.accounts.claude' <<<"$out")"
out="$(plan --state codex=burn)"
check '.accounts.codex.state == "burn"'
[ "$(jq -c '.accounts.claude' <<<"$out")" = "$base_claude" ] || fail "one-account override changed another"
printf 'ok: agents plan configuration, authentication and recorded routing decisions\n'
