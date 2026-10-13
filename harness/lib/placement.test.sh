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

printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
