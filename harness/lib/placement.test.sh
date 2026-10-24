#!/usr/bin/env bash
# Claude-only placement through `agents plan --place`: the cloud credit for code-only public boxes
# until it expires or a readout records it spent, then the Claude account with the most headroom,
# skipping accounts at the 5h stop or over the 5h burn cap. Pace lines and weekly projections never hold.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
# shellcheck source-path=SCRIPTDIR source=tree-north.sh
. "$here/tree-north.sh"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/placement-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
pass=0 fail=0
expect() {
  local name=$1 want=$2 got=$3
  if [ "$got" = "$want" ]; then pass=$((pass + 1)); printf 'PASS  %s\n' "$name"
  else fail=$((fail + 1)); printf 'FAIL  %s\n      want: %s\n      got:  %s\n' "$name" "$want" "$got"; fi
}

mkdir -p "$scratch/op" "$scratch/state/agents/costs"
for label in a c; do
  profile="$scratch/home/.local/state/north/profiles/claude-$label"
  mkdir -p "$profile" && printf '{}\n' >"$profile/.credentials.json"
  printf '{"oauthAccount": {"accountUuid": "fixture-%s"}}\n' "$label" >"$profile/.claude.json"
done
cat >"$scratch/op/orchestration.toml" <<'EOF'
[placement]
cloud_credit = "cloud-credit"
private_repos = ["galileo"]
[hosts.here]
serves = ["native"]
EOF
cat >"$scratch/op/costs.toml" <<'EOF'
[[service]]
id = "cloud-credit"
kind = "ai-plan"
billing = "prepaid"
initial = 250.0
expires = 2026-11-05T07:59:00Z
EOF
printf 'Item: north#9 tidy a test\nSurface: shell\nVerify: local-test\n' >"$scratch/code-only.md"
printf 'Item: smashcraft#4 LAN check\nSurface: lua\nVerify: native\n' >"$scratch/native.md"
printf 'Item: galileo#3 private change\nSurface: rust\n' >"$scratch/private.md"
printf 'Item: wisp#2 no facets\n' >"$scratch/undeclared.md"
gate() {
  printf '{"claude": {"paces": {"claude:a": {"state": "at or over pace"}}, "controls": {
    "claude:a": {"windows": [{"window": "5h", "used_pct": %s, "burn_pct_h": %s, "resets_at": "2026-10-13T17:00:00Z"}]},
    "claude:c": {"windows": [{"window": "5h", "used_pct": %s, "burn_pct_h": %s, "resets_at": "2026-10-13T17:00:00Z"},
                             {"window": "week", "used_pct": 90, "burn_pct_h": 2, "resets_at": "2026-10-19T00:00:00Z"}]}}}}\n' \
    "$@" >"$scratch/state/agents/usage-gate.json"
}
plan() {
  HOME="$scratch/home" AGENTS_ORCHESTRATION="$scratch/op/orchestration.toml" AGENTS_OPERATOR_DIR="$scratch/op" XDG_STATE_HOME="$scratch/state" \
    RESIDENT_CAP_HOST=here PLACEMENT_NOW="$1" "$root/bin/agents" plan --place "$scratch/$2.md"
}
place() { plan "$1" "$2" | sed -n -E 's/^[^ ][^:]*: target ([^(]*[^ (]).*/\1/p'; }
verdict() { plan "$1" native | sed -n -E "s/^  ($2 [a-z]+): .*/\1/p"; }
before=2026-10-13T13:00:00Z after=2026-11-05T08:00:00Z

gate 96 5 10 5
expect "code-only public box before expiry: anthropic-cloud" anthropic-cloud "$(place $before code-only)"
expect "code-only after expiry: account with headroom" "here claude:c" "$(place $after code-only)"
expect "native box stays here" "here claude:c" "$(place $before native)"
expect "private repository stays here" "here claude:c" "$(place $before private)"
expect "undeclared surface stays here" "here claude:c" "$(place $before undeclared)"
expect "claude:a at 96% 5h is unavailable" "claude:a unavailable" "$(verdict $before claude:a)"
gate 35 10 42 86
expect "claude:c burning 86%/h is over the 50%/h cap" "claude:c unavailable" "$(verdict $before claude:c)"
expect "claude:c over the burn cap places on claude:a, whose pace line never holds" "here claude:a" "$(place $before native)"
gate 99 1 96 1
expect "every account exhausted: held" "held: every Claude account exhausted or over its burn cap" "$(place $before native)"

line="$(HOME="$scratch/home" AGENTS_ORCHESTRATION="$scratch/op/orchestration.toml" AGENTS_OPERATOR_DIR="$scratch/op" XDG_STATE_HOME="$scratch/state" \
  PLACEMENT_NOW=$before "$root/bin/agents" plan --credit)"
expect "credit line with no readout" "cloud credit: unread, expires in 22 days" "$line"
printf '{"usd_spent_period": 250.0, "read_at": "2026-10-12T00:00:00Z"}\n' >"$scratch/state/agents/costs/cloud-credit.json"
gate 35 10 10 5
expect "credit recorded as spent skips anthropic-cloud for the most headroom" "here claude:a" "$(place $before code-only)"

printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
