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
mkdir -p "$op/replay" "$replay" "$scratch/wisp"
printf '{"id": "force-push"}\n' >"$op/replay/force-push.json"
printf '{"id": "other"}\n' >"$op/replay/other.json"
printf '# Wisp\n' >"$scratch/wisp/AGENTS.md"
export NORTH_REPO_ROOTS="{\"wisp\": \"$scratch/wisp\"}"
printf '{"activation": {}, "registrations": {}, "baselines": [{"targets": ["shared"], "owner": {"repo": "south", "path": "agents/AGENTS.md"}}]}\n' >"$op/catalog-config.json"
printf '[projects.wisp]\n' >"$op/orchestration.toml"
printf '# Policy\n\n## Rules\n\n- Keep it short.\n' >"$op/AGENTS.md"
sha="$(sha256sum "$op/AGENTS.md" | cut -d' ' -f1)"
point() { printf 'date = "2026-10-12"\nmodel = "m"\nvariant = "full"\npolicy_sha256 = "p"\nstartup_tokens = %s\nconformance = %s\nn = %s\nevidence = "e"\n' "$1" "${2:-0.8}" "${3:-100}" >"$op/operating-point.toml"; }
full='{"force-push": [3, 3], "other": [3, 3]}'
row() { printf '{"variant": "point", "policy_sha256": "p", "model": "m", "conformance": 1.0, "n": 6, "scenarios": {"force-push": [3, 3], "other": [3, 3]}}\n{"variant": "v", "policy_sha256": "%s", "model": "%s", "startup_tokens": 1, "conformance": %s, "n": %s, "scenarios": %s}\n' "$sha" "${3:-m}" "$1" "${2:-100}" "${4:-$full}" >"$op/replay/sweep-2026-10-12.jsonl"; }
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

expect fail 'no operating point; run the sweep and promote one'
point 1000000000
expect fail 'the live policy has no sweep row'
point 0
row 0.8
expect fail 'startup tokens grew 0→[0-9]* with no measured conformance gain'
row 0.95 100 other-model
expect fail 'sweep row ran on other-model, the operating point on m'
row 0.95
expect pass 'operating point full 0 at 2026-10-12, conformance 0.8 \[0.711, 0.867\] n=100'
point 1000000000
row 0.6
expect fail 'replay conformance dropped 0.8→0.6'
row 0.95 100 m '{"force-push": [0, 3], "other": [3, 3]}'
expect fail 'scenario force-push flipped to 0 passes'
point 1000000000 1.0 42
row 0.9524 42
expect pass 'conformance 1 \[0.916, 1.000\] n=42'
row 0.8333 42
expect fail 'replay conformance dropped 1→0.8333'
point 1000000000 0.881 42
row 0.619 42
expect fail 'replay conformance dropped 0.881→0.619'
row 0.857 42
expect pass 'conformance 0.881'
row 0.857 42 m '{"force-push": [3, 3], "other": [2, 2]}'
expect fail "sweep row for ${sha:0:8} covers 1 of 2 scenarios"
tokens="$(OPERATOR="$op" policy_gate "$scratch" measure | sed -n 's/^session start \([0-9]*\) tokens$/\1/p')"
point "$tokens" 0.881 42
row 0.881 42
printf 'Twelve chars' >>"$scratch/wisp/AGENTS.md"
expect pass "session start $tokens tokens"
printf '{"variant": "v", "policy_sha256": "%s", "model": "m", "startup_tokens": 1, "conformance": 1.0, "n": 2, "scenarios": {"force-push": [1, 1], "other": [1, 1]}}\n' "$sha" >"$replay/sweep-2026-10-19.jsonl"
printf '{"id": "new-scenario"}\n' >"$op/replay/new-scenario.json"
expect pass "session start $tokens tokens"
printf 'ok: policy gate, reading sweep rows stored with the policy in south and this host'"'"'s own, fails without an operating point, without a live sweep row, on a row missing a scenario or trials, on another model, on growth without gain, on a one-sided Fisher drop (42/42 to 35/42, 37/42 to 26/42) and on a 3/3 to 0/3 scenario flip; passes 1.0 vs 0.95 and 37/42 vs 36/42 at n=42 and a project AGENTS.md growth, a later 1-trial row and a scenario the operating point never measured\n'

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
printf '{"id": "inc-1", "session": "s1"}\n{"id": "inc-2", "session": "s1"}\n{"id": "inc-3", "session": null, "ref": "north#5"}\n' >"$op/incidents.jsonl"
git -C "$op" init -q && git -C "$op" add rules.toml && git -C "$op" -c user.name=t -c user.email=t@t commit -qm base
rule_register "$op" >/dev/null
printf '\n- Cite incidents.\n' >>"$op/AGENTS.md"
printf '\n[[rule]]\nid = "cite"\nsection = "Rules"\ntext = "Cite incidents."\nplacement = "global"\nfixes = "Unearned rules."\nincidents = ["inc-1", "inc-9"]\n' >>"$op/rules.toml"
unearned() {
  if rule_register "$op" >"$scratch/rules.out" || ! grep -q 'cite: a new rule must cite incidents.jsonl incidents from at least 2 sessions' "$scratch/rules.out"; then
    printf 'rules check accepted an unearned rule:\n' >&2
    cat "$scratch/rules.out" >&2
    exit 1
  fi
}
unearned
sed -i 's/"inc-9"/"inc-2"/' "$op/rules.toml"
unearned
work="$scratch/work"
git init -q "$work"
for n in 1 2; do git -C "$work" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "fix $n (work#7)"; done
first="$(git -C "$work" rev-parse HEAD~1)" second="$(git -C "$work" rev-parse HEAD)"
printf '{"id": "inc-4", "source": "git", "session": null, "ref": "work@%s"}\n{"id": "inc-5", "source": "github", "session": null, "ref": "t/work#7"}\n{"id": "inc-6", "source": "git", "session": null, "ref": "work@%s"}\n' "$first" "$second" >>"$op/incidents.jsonl"
sed -i 's/"inc-1", "inc-2"\]/"inc-4", "inc-6", "inc-5"]/' "$op/rules.toml"
NORTH_REPO_ROOTS="{\"work\": \"$work\"}" unearned
sed -i 's/"inc-4", "inc-6", "inc-5"\]/"inc-1", "inc-3"]/' "$op/rules.toml"
rule_register "$op" >/dev/null
printf 'ok: a rule added since the landed rules.toml needs ledger incidents from 2 sessions, counting one issue once\n'

vocab="$scratch/vocab"
git init -q "$vocab"
printf '# Repo\n\nThe harness is under harness/hooks/.\n' >"$vocab/README.md"
printf '{"ref": "north:socrates/hooks/x.sh"}\n' >"$vocab/history.jsonl"
git -C "$vocab" add README.md history.jsonl
harness_vocabulary "$vocab" >/dev/null
refused() {
  if harness_vocabulary "$vocab" >"$scratch/vocab.out" || ! grep -q "$1" "$scratch/vocab.out"; then
    printf 'vocabulary check accepted %s:\n' "$2" >&2
    cat "$scratch/vocab.out" >&2
    exit 1
  fi
  git -C "$vocab" rm -q --cached -r . && git -C "$vocab" add README.md history.jsonl
}
mkdir -p "$vocab/socrates"; printf 'x\n' >"$vocab/socrates/hook.sh"; git -C "$vocab" add socrates/hook.sh
refused 'lives under harness/, not socrates/: socrates/hook.sh' 'a socrates/ path'
printf 'Run socrates/scripts/check.sh.\n' >"$vocab/notes.md"; git -C "$vocab" add notes.md
refused 'notes.md' 'a socrates/ caller'
printf 'Socrates is the agent machinery beside North.\n' >"$vocab/notes.md"; git -C "$vocab" add notes.md
refused 'notes.md' 'Socrates called the agent machinery'
harness_vocabulary "$vocab" >/dev/null
printf 'ok: the vocabulary check refuses a socrates/ path, a socrates/ caller and "Socrates is the agent machinery", and passes harness/ paths and history lines\n'
