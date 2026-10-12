#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agent-config-check.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
cat >"$scratch/activation.json" <<'JSON'
{
  "schema": "north.agent-activation/v1",
  "catalogDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "generationId": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "units": [
    {
      "id": "fixture-guard",
      "kind": "hook",
      "permission": "on",
      "active": true
    }
  ]
}
JSON

NORTH_AGENT_ACTIVATION="$scratch/activation.json" \
NORTH_HOOK_ID=fixture-guard \
  bash -c 'source "$1"; ! authoring_guards_off' -- \
    "$repo/lib/authoring-killswitch.sh"
NORTH_AGENT_ACTIVATION="$scratch/activation.json" \
NORTH_HOOK_ID=fixture-guard \
AGENT_NO_AUTHORING_HOOKS=1 \
  bash -c 'source "$1"; authoring_guards_off' -- \
    "$repo/lib/authoring-killswitch.sh"

printf 'ok: activation enables guards and the explicit kill switch disables them\n'

# shellcheck disable=SC1091
. "$repo/scripts/agent-config-check.sh"
export AGENT_CONFIG_PYTHON=python3
op="$scratch/agents"
replay="$scratch/replay"
mkdir -p "$op" "$replay"
printf '{"activation": {}, "registrations": {}, "baselines": [{"targets": ["shared"], "owner": {"repo": "south", "path": "agents/AGENTS.md"}}]}\n' >"$op/catalog-config.json"
printf '[projects]\n' >"$op/orchestration.toml"
printf '# Policy\n\n## Rules\n\n- Keep it short.\n' >"$op/AGENTS.md"
sha="$(sha256sum "$op/AGENTS.md" | cut -d' ' -f1)"
point() { printf 'date = "2026-10-12"\nmodel = "m"\nvariant = "full"\npolicy_sha256 = "x"\nstartup_tokens = %s\nconformance = %s\nn = %s\nevidence = "e"\n' "$1" "${2:-0.8}" "${3:-100}" >"$op/operating-point.toml"; }
row() { printf '{"variant": "v", "policy_sha256": "%s", "startup_tokens": 1, "conformance": %s, "n": %s}\n' "$sha" "$1" "${2:-100}" >"$replay/sweep-2026-10-12.jsonl"; }
gate() { OPERATOR="$op" AGENTS_REPLAY_DIR="$replay" policy_gate "$scratch" session >"$scratch/gate.out" 2>&1; }
expect() {
  local want="$1" pattern="$2" got=pass
  gate || got=fail
  if [ "$got" != "$want" ] || ! grep -q "$pattern" "$scratch/gate.out"; then
    printf 'policy gate: expected %s with %s, got %s:\n' "$want" "$pattern" "$got" >&2
    cat "$scratch/gate.out" >&2
    exit 1
  fi
}

expect pass 'no operating point yet'
point 0
expect fail 'startup tokens grew 0→[0-9]* with no measured conformance gain'
row 0.95
expect pass 'operating point full 0 at 2026-10-12, conformance 0.8 \[0.711, 0.867\] n=100'
point 1000000000
row 0.6
expect fail 'replay conformance dropped 0.8→0.6'
point 1000000000 1.0 42
row 0.9524 42
expect pass 'conformance 1 \[0.916, 1.000\] n=42'
printf 'ok: policy gate passes without an operating point or with a Wilson gain, fails on growth without gain and on a Wilson drop, and keeps 1.0 vs 0.95 at n=42\n'

python3 - "$repo/scripts" <<'PY'
import sys
sys.path.insert(0, sys.argv[1])
from policy_stats import operating_point
row = lambda name, tokens, c, n, force: {"variant": name, "startup_tokens": tokens, "conformance": c, "n": n,
                                         "scenarios": {"force-push": force, "other": [round(c * n) - force[0], n - force[1]]}}
point = operating_point([row("base", 5186, 1.0, 49, [10, 10]), row("hook-force-push", 5170, 0.9286, 42, [0, 3])])
assert point["variant"] == "base", point["variant"]
PY
printf 'ok: the operating point skips a variant whose interval on one scenario lies below the best variant'"'"'s\n'

cat >"$op/rules.toml" <<'TOML'
[[rule]]
id = "keep-short"
section = "Rules"
text = "Keep it short."
placement = "global"
fixes = "Long policy."
incidents = []
TOML
printf '{"id": "inc-1"}\n{"id": "inc-2"}\n' >"$op/incidents.jsonl"
git -C "$op" init -q && git -C "$op" add rules.toml && git -C "$op" -c user.name=t -c user.email=t@t commit -qm base
rule_register "$op" >/dev/null
printf '\n- Cite incidents.\n' >>"$op/AGENTS.md"
printf '\n[[rule]]\nid = "cite"\nsection = "Rules"\ntext = "Cite incidents."\nplacement = "global"\nfixes = "Unearned rules."\nincidents = ["inc-1", "inc-9"]\n' >>"$op/rules.toml"
if rule_register "$op" >"$scratch/rules.out" || ! grep -q 'cite: a new rule must cite at least 2 incidents' "$scratch/rules.out"; then
  printf 'rules check accepted an unearned rule:\n' >&2
  cat "$scratch/rules.out" >&2
  exit 1
fi
sed -i 's/"inc-9"/"inc-2"/' "$op/rules.toml"
rule_register "$op" >/dev/null
printf 'ok: a rule added since the landed rules.toml needs 2 ledger incidents\n'
