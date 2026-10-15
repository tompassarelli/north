#!/usr/bin/env bash
# H3 overflow routing through `agents plan --place`: Nexus at 6/6 on a fake /proc, gex45 and the laptop state from
# fixtures, and the cloud credit's expiry crossed with PLACEMENT_NOW.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/placement-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
pass=0 fail=0
expect() {
  local name=$1 want=$2 got=$3
  if [ "$got" = "$want" ]; then pass=$((pass + 1)); printf 'PASS  %s\n' "$name"
  else fail=$((fail + 1)); printf 'FAIL  %s\n      want: %s\n      got:  %s\n' "$name" "$want" "$got"; fi
}

mkdir -p "$scratch/op" "$scratch/state/agents/costs" "$scratch/proc/1"
cat >"$scratch/op/orchestration.toml" <<'EOF'
[hosts.nexus]
resident_cap = 6

[hosts.gex45]
resident_cap = 24

[placement]
order = ["gex45", "anthropic-cloud", "nexus", "laptop", "codex-cloud"]
cloud_credit = "claude-a-cloud-credit"
codex_cloud = false
private_repos = ["galileo"]
EOF
cat >"$scratch/op/costs.toml" <<'EOF'
[[service]]
id = "claude-a-cloud-credit"
kind = "ai-plan"
billing = "prepaid"
initial = 250.0
expires = 2026-11-05T07:59:00Z
EOF
printf '1 (systemd) S 0 1 1 0 -1\n' >"$scratch/proc/1/stat"
printf '/sbin/init\0' >"$scratch/proc/1/cmdline"
for i in 1 2 3 4 5 6; do
  mkdir "$scratch/proc/30$i"
  printf '30%s (claude) S 1 30%s 30%s 0 -1\n' "$i" "$i" "$i" >"$scratch/proc/30$i/stat"
  printf '/home/tom/.local/lib/claude/current/bin/claude\0--resume\0' >"$scratch/proc/30$i/cmdline"
done
printf '{"version": 1, "nodes": []}\n' >"$scratch/org.json"
printf '{"gex45": {"online": false}}\n' >"$scratch/gex45-offline.json"
printf '{"gex45": {"online": true, "live": 3}}\n' >"$scratch/gex45-online.json"
printf '{"state": "available"}\n' >"$scratch/laptop-available.json"
printf '{"state": "unavailable"}\n' >"$scratch/laptop-unavailable.json"
printf 'Item: north#9 tidy a test\nVerify: local-test\n' >"$scratch/code-only.md"
printf 'Item: smashcraft#4 LAN check\nVerify: native\n' >"$scratch/native.md"
printf 'Item: galileo#3 private change\nVerify: local-test\n' >"$scratch/private.md"

place() {
  local hosts=$1 laptop=$2 at=$3 brief=$4
  AGENTS_ORCHESTRATION="$scratch/op/orchestration.toml" AGENTS_OPERATOR_DIR="$scratch/op" XDG_STATE_HOME="$scratch/state" \
    AGENTS_ORG_FILE="$scratch/org.json" RESIDENT_CAP_PROC="$scratch/proc" RESIDENT_CAP_HOST=nexus \
    PLACEMENT_HOSTS="$scratch/$hosts.json" LAPTOP_STATE="$scratch/laptop-$laptop.json" PLACEMENT_NOW="$at" \
    "$root/bin/agents" plan --place "$scratch/$brief.md" | sed -E 's/^[^:]*: target ([^(]*[^ (]) \(.*/\1/'
}
before=2026-10-13T13:00:00Z after=2026-11-05T08:00:00Z

expect "code-only at nexus 6/6, gex45 offline: anthropic-cloud" anthropic-cloud "$(place gex45-offline unavailable $before code-only)"
expect "code-only, gex45 online below cap: gex45" gex45 "$(place gex45-online unavailable $before code-only)"
expect "code-only after expiry, laptop available: laptop" laptop "$(place gex45-offline available $after code-only)"
expect "code-only after expiry, laptop unavailable: held" "held: laptop unavailable" "$(place gex45-offline unavailable $after code-only)"
expect "native at 6/6, laptop available: laptop" laptop "$(place gex45-online available $before native)"
expect "native at 6/6, laptop unavailable: held" "held: laptop unavailable" "$(place gex45-online unavailable $before native)"
expect "private repo skips the cloud: held" "held: laptop unavailable" "$(place gex45-offline unavailable $before private)"

line="$(AGENTS_ORCHESTRATION="$scratch/op/orchestration.toml" AGENTS_OPERATOR_DIR="$scratch/op" XDG_STATE_HOME="$scratch/state" \
  PLACEMENT_NOW=$before "$root/bin/agents" plan --credit)"
expect "credit line with no readout" "cloud credit: unread, expires in 22 days" "$line"
printf '{"usd_spent_period": 250.0, "read_at": "2026-10-12T00:00:00Z"}\n' >"$scratch/state/agents/costs/claude-a-cloud-credit.json"
expect "credit recorded as spent skips anthropic-cloud" "held: laptop unavailable" "$(place gex45-offline unavailable $before code-only)"

rm -f "$scratch/state/agents/costs/claude-a-cloud-credit.json"
tooling='Item: north#51 overflow\nCategory: tooling\nSpec: exact\nScope: module\nSurface: shell\nVerify: local-test\nETA: 20 min\nFiles: harness/lib/routing.py\nDone when: `bash harness/lib/routing.test.sh` passes\n'
printf "$tooling" >"$scratch/tooling.md"
printf "${tooling/Surface: shell/Surface: ts}" >"$scratch/tooling-ts.md"
printf "${tooling/Category: tooling/Category: bug-known-cause}" >"$scratch/tooling-bug.md"
usage_db() {
  local name=$1; shift
  python3 -I - "$scratch/$name.db" "$@" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, resets_at TEXT, source TEXT)")
db.execute("INSERT INTO usage VALUES ('2026-10-15T04:50:00Z', 'claude', 'a', 'window', 'session', 97, '2026-10-15T05:00:00Z', 'usage_claude')")
db.execute("INSERT INTO usage VALUES ('2026-10-15T07:30:00Z', 'claude', 'b', 'window', 'session', 99, '2026-10-15T10:00:00Z', 'usage_claude')")
for spec in sys.argv[2:]:
    ts, pct, source = spec.split(",")
    db.execute("INSERT INTO usage VALUES (?, 'claude', 'a', 'window', 'session', ?, '2026-10-15T10:00:00Z', ?)", (ts, float(pct), source))
db.commit()
PY
}
usage_db over 2026-10-15T07:00:00Z,0,usage_claude 2026-10-15T07:30:00Z,28,usage_claude 2026-10-15T08:00:00Z,43,usage_claude
usage_db under 2026-10-15T07:30:00Z,25,usage_claude 2026-10-15T08:00:00Z,34,usage_claude
usage_db full 2026-10-15T07:30:00Z,91,usage_claude 2026-10-15T08:00:00Z,91,usage_claude
usage_db throttled 2026-10-15T07:25:00Z,25,usage_claude 2026-10-15T07:55:00Z,34,"usage_claude refresh failed code=1 throttled=True"
place_line() {
  local db=$1 brief=$2
  THREADS_DB="$scratch/$db.db" AGENTS_ORCHESTRATION="$scratch/op/orchestration.toml" AGENTS_OPERATOR_DIR="$scratch/op" \
    XDG_STATE_HOME="$scratch/state" AGENTS_ORG_FILE="$scratch/org.json" RESIDENT_CAP_PROC="$scratch/proc" RESIDENT_CAP_HOST=nexus \
    PLACEMENT_HOSTS="$scratch/gex45-offline.json" LAPTOP_STATE="$scratch/laptop-unavailable.json" PLACEMENT_NOW=2026-10-15T08:00:00Z \
    "$root/bin/agents" plan --place "$scratch/$brief.md" | sed -E 's/^[^:]*: //'
}
today="$(place_line none tooling)"
expect "claude:a 5h projected 103%: tooling box overflows to codex" "target codex (claude:a 5h projected 103% > 90%)" "$(place_line over tooling)"
expect "claude:a 5h projected 70%: today's target" "$today" "$(place_line under tooling)"
expect "claude:a 5h projected 70%: today's target is anthropic-cloud" "target anthropic-cloud" "${today%% (*}"
expect "claude:a 5h 91% used: codex" "target codex (claude:a 5h 91% >= 90%)" "$(place_line full tooling)"
expect "claude:a 429 5 min ago: codex" "target codex (claude:a 5h 429 5 min ago)" "$(place_line throttled tooling)"
expect "claude:a 5h projected 103%, Surface ts: codex sol-high tight leash" "target codex (claude:a 5h projected 103% > 90%; codex:sol-high on the tight leash)" "$(place_line over tooling-ts)"
expect "claude:a 5h projected 103%, Claude-routed bug-known-cause: codex sol-high tight leash" "target codex (claude:a 5h projected 103% > 90%; codex:sol-high on the tight leash)" "$(place_line over tooling-bug)"
printf "${tooling/Scope: module/Scope: cross-module}" >"$scratch/tooling-cross.md"
printf '{"claude": {"paces": {"claude:a": {"state": "at or over pace"}, "claude:b": {"state": "at or over pace"}}}}\n' >"$scratch/state/agents/usage-gate.json"
expect "every Claude account held, cross-module box: codex sol-high tight leash" "target codex (no Claude account admits: claude:a at or over pace, claude:b at or over pace; codex:sol-high on the tight leash)" "$(place_line under tooling-cross)"
printf '{"claude": {"paces": {"claude:a": {"state": "at or over pace"}, "claude:b": {"state": "under pace"}}}}\n' >"$scratch/state/agents/usage-gate.json"
expect "claude:b under pace, cross-module box: today's target" "$today" "$(place_line under tooling-cross)"
rm -f "$scratch/state/agents/usage-gate.json"

printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
