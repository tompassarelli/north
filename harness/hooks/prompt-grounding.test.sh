#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
grounding="$here/../grounding"
runner="${NORTH_GROUNDING_BIN:-$here/../../target/debug/north}"
[ -x "$runner" ] || runner="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target/north}/debug/north"
bun="$(command -v bun)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/prompt-grounding-test.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT

north="$scratch/north"
mkdir -p "$north/harness/skills/kayak-paddle" "$scratch/handoffs" "$scratch/sources" "$scratch/state" "$scratch/bin"
cat >"$north/harness/skills/kayak-paddle/SKILL.md" <<'EOF'
---
name: kayak-paddle
description: Plan kayak paddle trips with the river gauge and tide tables.
---
# Kayak paddle
Check the river gauge before every kayak paddle trip.
EOF
git -C "$north" init -q && git -C "$north" add -A
printf '# Gauge brief\nThe river gauge feed is cached hourly.\n' >"$scratch/handoffs/gauge-brief.md"
for i in $(seq 1 200); do
  printf '{"id":"L%s","at":"2026-10-15T06:00:00Z","kind":"decision","topic":"gauge","quote":"river gauge kayak decision number %s with padding text to fill the block","source":"tom convo:%s session:s","superseded_by":null}\n' "$i" "$i" "$i"
done >"$scratch/ledger-big.jsonl"
printf '{"id":"L1","at":"2026-10-15T06:00:00Z","kind":"decision","topic":"gauge","quote":"use the river gauge before any kayak trip","source":"tom convo:1 session:s","superseded_by":null}\n' >"$scratch/ledger.jsonl"
long="$(printf 'river gauge kayak cache detail %.0s' $(seq 1 12))"
for i in $(seq 1 60); do
  mkdir -p "$north/harness/skills/gauge-$i"
  printf -- '---\nname: gauge-%s\ndescription: %s\n---\n# Gauge %s\n%s\n' "$i" "$long" "$i" "$long" >"$north/harness/skills/gauge-$i/SKILL.md"
  printf '# %s\n%s\n' "$long" "$long" >"$scratch/handoffs/gauge-$i-brief.md"
done
git -C "$north" add -A
for source in policy ledger; do
  printf 'export * from "%s/sources/%s.ts";\n' "$grounding" "$source" >"$scratch/sources/$source.ts"
done
printf 'hook prompt-grounding\n' >"$scratch/active"
printf '{"accounts":{},"providers":{}}' >"$scratch/pace.json"
mkdir -p "$scratch/fleet/child"
printf '#!/usr/bin/env bash\nexec "$NORTH_REAL" "$@"\n' >"$scratch/bin/north"; chmod +x "$scratch/bin/north"

export NORTH_GROUNDING_CONVO_DB="$scratch/missing.db" NORTH_GROUNDING_STATE="$scratch/state" NORTH_GROUNDING_SOURCES="$scratch/sources" \
  NORTH_GROUNDING_NORTH="$north" NORTH_GROUNDING_SOUTH="$scratch/missing" NORTH_GROUNDING_HANDOFFS="$scratch/handoffs" \
  NORTH_GROUNDING_LEDGER="$scratch/ledger.jsonl" NORTH_GROUNDING_PACE="$scratch/pace.json" NORTH_FLEET_DIR="$scratch/fleet" \
  NORTH_BIN="$scratch/bin/north" NORTH_REAL="$runner" NORTH_AGENT_ACTIVE="$scratch/active" NORTH_GROUNDING_BUN="$bun" NORTH_GROUNDING_BIN="$runner" CLAUDE_CODE_ENTRYPOINT=cli PATH="/etc/codex/hooks/runtime:$PATH"
unset AGENT_NO_AUTHORING_HOOKS

hook() { printf '%s' "$1" | env "${@:2}" bash "$here/prompt-grounding.sh"; }
prompt() { printf '{"hook_event_name":"UserPromptSubmit","session_id":"s","prompt":"%s"}' "$1"; }
fail() { echo "FAIL $*" >&2; exit 1; }

out="$(hook "$(prompt 'plan a kayak paddle trip with the river gauge')")"; [ -z "$out" ] || fail "missing index must print nothing: $out"
echo "PASS missing index fails open with no output"

counts="$("$runner" grounding refresh)"
case "$counts" in *'"ledger":'*1*'"policy":'*[1-9]*) ;; *) fail "first refresh on a fresh state dir indexed nothing: $counts" ;; esac
echo "PASS first refresh on a fresh state dir builds the index: $counts"

out="$(hook "$(prompt 'plan a kayak paddle trip with the river gauge')")"
case "$out" in *'"hookEventName":"UserPromptSubmit"'*) ;; *) fail "proxy prompt got no UserPromptSubmit context: $out" ;; esac
case "$out" in *'harness/skills/kayak-paddle/SKILL.md'*) ;; *) fail "block lacks the matching skill path: $out" ;; esac
case "$out" in *'ledger:L1'*) ;; *) fail "block lacks the matching decision: $out" ;; esac
echo "PASS proxy prompt gets a Grounding block naming the matching skill path and decision"

printf '{\n  "name": "child",\n  "conversation": "s"\n}\n' >"$NORTH_FLEET_DIR/child/session.json"
[ -z "$(hook "$(prompt 'plan a kayak paddle trip with the river gauge')")" ] || fail "fleet session"
rm "$NORTH_FLEET_DIR/child/session.json"
[ -z "$(hook "$(prompt 'plan a kayak paddle trip with the river gauge')" CLAUDE_CODE_ENTRYPOINT=sdk-cli)" ] || fail "non-cli session"
[ -z "$(hook '{"hook_event_name":"UserPromptSubmit","agent_id":"w","prompt":"kayak paddle river gauge"}')" ] || fail "subagent payload"
echo "PASS only Tom's top-level interactive session is grounded (fleet, sdk and subagent print nothing)"

[ -z "$(hook "$(prompt 'ok')")" ] && [ -z "$(hook "$(prompt 'yes')")" ] || fail "chit-chat produced a block"
echo "PASS chit-chat prints nothing"

[ -z "$(hook "$(prompt 'plan a kayak paddle trip with the river gauge')" AGENT_NO_AUTHORING_HOOKS=1)" ] || fail "kill-switch env"
printf 'hook other\n' >"$scratch/inactive"
[ -z "$(hook "$(prompt 'plan a kayak paddle trip with the river gauge')" NORTH_AGENT_ACTIVE="$scratch/inactive")" ] || fail "inactive unit"
echo "PASS kill-switch and inactive activation print nothing"

NORTH_GROUNDING_LEDGER="$scratch/ledger-big.jsonl" "$runner" grounding refresh --force >/dev/null
"$bun" -e 'const a = {}; for (let i = 0; i < 60; i++) a[`claude:${i}`] = { windows: [{ window: "5h", used_pct: i, resets_at: "2026-10-15T09:00:00Z", as_of: "2026-10-15T07:00:00Z" }, { window: "week", used_pct: i, resets_at: "2026-10-20T09:00:00Z", as_of: "2026-10-15T07:00:00Z" }], allowed_workers: 1 }; console.log(JSON.stringify({ accounts: a }))' >"$scratch/pace-big.json"
out="$(hook "$(prompt 'river gauge kayak cache detail decision usage')" NORTH_GROUNDING_PACE="$scratch/pace-big.json")"
size="$(printf '%s' "$out" | "$bun" -e 'const d = JSON.parse(await Bun.stdin.text()); console.log(d.hookSpecificOutput.additionalContext.length)')"
[ "$size" -gt 0 ] && [ "$size" -le 6000 ] || fail "block size $size outside 1..6000"
[ "$size" -gt 5000 ] || fail "cap test did not fill the block ($size chars)"
echo "PASS block stays within the 6000-character cap with 120 matching docs, 200 decisions and 60 accounts ($size chars)"

out="$(PATH=/etc/codex/hooks/runtime "$runner" grounding --tier B --query "didn't we already build the zebra quokka harmonica thing")"
case "$out" in *'searched '*" for '"*'zebra'*'this one?'*) ;; *) fail "tier B without hits lacks the ask format: $out" ;; esac
echo "PASS tier B with nothing above threshold prints the ask format naming what was searched"

out="$("$runner" grounding --query "didn't we already set up the river gauge cache?")"
case "$out" in *'--tier B'*) ;; *'handoffs/gauge'*) ;; *) fail "past-referring query gives neither hits nor tier-B command: $out" ;; esac
echo "PASS past-referring query is grounded or names the tier-B command"
