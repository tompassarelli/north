#!/usr/bin/env bash
# Deny/pass matrix for the worker tier guard.
set -uo pipefail
export PATH="/etc/codex/hooks/runtime:$PATH"
# shellcheck source-path=SCRIPTDIR source=../lib/tree-north.sh
. "$(dirname "$0")/../lib/tree-north.sh"

HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$HERE/worker-tier-guard.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/worker-tier-guard-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT
mkdir -p "$SCRATCH/home/.local/state/north"
ACTIVATION="$SCRATCH/activation.active"
printf 'hook worker-tier-guard\n' >"$ACTIVATION"
python3 - "$HOOK" "$SCRATCH" "$ACTIVATION" <<'PY'
import json
import os
from pathlib import Path
import subprocess
import sys
import time

hook, scratch, activation = sys.argv[1:]
state = Path(scratch) / 'home/.local/state/agents'
state.mkdir(parents=True)
path = state / 'pace-control.json'
grant = {'account': 'claude:a', 'session': 'parent', 'uses': 1,
         'pending': [{'id': 'denied', 'at': time.time()}]}
event = {'tool_name': 'Agent', 'session_id': 'parent', 'tool_use_id': 'denied',
         'tool_input': {'subagent_type': 'worker', 'prompt': 'Category: feature'}}
env = dict(os.environ, HOME=str(Path(scratch) / 'home'), XDG_STATE_HOME=str(state.parent),
           NORTH_AGENT_ACTIVE=activation, NORTH_AGENT_PYTHON='/etc/codex/hooks/runtime/python3',
           SPAWN_CAPACITY_HOME=str(Path(scratch) / 'home'))
for disabled in ('1', '0'):
    path.write_text(json.dumps({'grants': [grant]}))
    result = subprocess.run([hook], input=json.dumps(event), text=True, capture_output=True,
                            check=True, env=env | {'AGENT_NO_AUTHORING_HOOKS': disabled})
    saved = json.loads(path.read_text())['grants'][0]
    assert saved['uses'] == 1, saved
    if disabled == '1':
        assert not result.stdout and saved['pending'] == grant['pending'], (result, saved)
    else:
        assert json.loads(result.stdout)['hookSpecificOutput']['permissionDecision'] == 'deny', result
        assert saved['pending'] == [], saved
print('PASS tier refusal after capacity reservation releases its use immediately; disabled guard preserves it')
PY
[ "$?" -eq 0 ] || exit 1

pass=0 fail=0
# run EXPECT DESCRIPTION SUBAGENT_TYPE PROMPT [ENV...]
run() {
  local expect="$1" desc="$2" agent="$3" prompt="$4"; shift 4
  local input out decision ok=0
  input="$(python3 -c 'import json,sys
ti = {"description": "d", "prompt": sys.argv[1]}
if sys.argv[2]: ti["subagent_type"] = sys.argv[2]
if sys.argv[3] != "missing": ti["run_in_background"] = json.loads(sys.argv[3])
if sys.argv[2] == "codex":
    print(json.dumps({"tool_name": "spawn_agent", "tool_input": {"message": sys.argv[1], "task_name": "worker"}}))
else:
    print(json.dumps({"tool_name": "Agent", "tool_input": ti}))' "$prompt" "$agent" "${BACKGROUND:-true}")"
  out="$(printf '%s' "$input" | env -u AGENT_NO_AUTHORING_HOOKS \
    HOME="$SCRATCH/home" NORTH_AGENT_ACTIVE="$ACTIVATION" \
    NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 "$@" "$HOOK" 2>&1)"
  decision="$(python3 -c 'import json,sys
try:
    d = json.loads(sys.argv[1] or "null")
except Exception:
    print("malformed"); raise SystemExit
print((d or {}).get("hookSpecificOutput", {}).get("permissionDecision", "silent"))' "${out:-}")"
  case "$expect" in
    deny)  [ "$decision" = deny ] && ok=1 ;;
    allow) [ "$decision" != deny ] && [ "$decision" != malformed ] && ok=1 ;;
  esac
  if [ -n "${EXPECT_REASON:-}" ] && [[ "$out" != *"$EXPECT_REASON"* ]]; then ok=0; fi
  if [ "$ok" = 1 ]; then
    pass=$((pass + 1)); printf 'PASS  %-5s  %s\n' "$expect" "$desc"
  else
    fail=$((fail + 1))
    printf 'FAIL  %-5s  %s\n      decision=%s out=%s\n' "$expect" "$desc" "$decision" "$out"
  fi
}

FACETS=$'\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test'
MECH=$'Item: rename a field.\nCategory: mechanical\nDone when: check passes.'"$FACETS"
DOCS=$'Item: fix a skill line. ETA 5 min.\nCategory: docs-policy'"$FACETS"
FEAT=$'Item: add a parser.\nCategory: feature'"$FACETS"
ESC=$'Follows: worker-haiku run a1b2 failed on the check.\nCategory: mechanical'"$FACETS"
BACKGROUND=missing run allow 'Claude Agent implicit background' worker "$ESC"
for background in false '"true"'; do
  BACKGROUND="$background" EXPECT_REASON='run_in_background: true' run deny "Claude worker foreground $background even with Follows" worker "$ESC"
done
BACKGROUND=false run allow 'native spawn ignores Claude background field' codex "$ESC"
BACKGROUND=false run allow 'nonworker remains foreground' Explore "$ESC"
run deny  'mechanical to worker'        worker        "$MECH"
run allow 'docs-policy to worker'       worker        "$DOCS"
run deny  'mechanical to worker-high'   worker-high   "$MECH"
run allow 'mechanical to worker-haiku'  worker-haiku  "$MECH"
run deny  'docs-policy to worker-haiku' worker-haiku  "$DOCS"
run deny  'docs-policy to worker-high'  worker-high   "$DOCS"
run allow 'mechanical escalated'        worker        "$ESC"
run deny  'worker-high first'           worker-high   "$FEAT"
run deny  'worker-xhigh first'          worker-xhigh  "$FEAT"
run allow 'worker-high escalated'       worker-high   $'Follows: worker run c3 wrong cause twice.\n'"$FEAT"
run allow 'worker-xhigh escalated'      worker-xhigh  $'Follows: worker-high run d4 failed.\n'"$FEAT"
run allow 'complete native spawn' codex "$FEAT"
run allow 'native mechanical retains Codex tier rules' codex "$MECH"
run allow 'feature to worker'           worker        "$FEAT"
for field in Category Spec Scope Surface Verify; do
  incomplete=$(printf '%s\n' "$FEAT" | sed "/^$field:/d")
  run deny "missing $field" worker "$incomplete"
  run deny "native missing $field" codex "$incomplete"
  run deny "missing $field escalated" worker-high $'Follows: worker failed.\n'"$incomplete"
done
run allow 'known facets'                worker        $'Category: tooling\nSpec: measured\nScope: one-file\nSurface: rust\nVerify: local-test\nPayback: enables x; cost 5 min'
PAYBACK='Payback: saves 4 min/change (Tom 0.3, agent 1), 20 changes/day, cost 45 min, payback 0.5 days'
for cat in tooling infra ci harness; do
  EXPECT_REASON='planning skill' run deny "$cat without Payback" worker "Category: $cat$FACETS"
  run deny "native $cat without Payback" codex "Category: $cat$FACETS"
  run allow "$cat with saving Payback" worker "Category: $cat"$'\n'"$PAYBACK$FACETS"
done
run allow 'tooling with enabling Payback' worker $'Category: tooling\nPayback: enables iteration ledger; cost 30 min'"$FACETS"
run allow 'tooling with other priced reason' worker $'Category: tooling\nPayback: stops unpriced boxes; cost ~45 agent-min'"$FACETS"
run deny  'tooling with unpriced Payback' worker $'Category: tooling\nPayback: worth it'"$FACETS"
run deny  'tooling escalation without Payback' worker-high $'Follows: worker run c3 failed.\nCategory: tooling'"$FACETS"
run allow 'prose mentioning Missing Scope: declare' worker $'Item: tidy the guard message Missing Scope: declare.\nCategory: feature'"$FACETS"
run allow 'prose mentioning the Scope: section' worker $'Item: check the Scope: section of the brief.\nCategory: feature'"$FACETS"
run deny  'header line Scope: section still refused' worker $'Category: feature\nSpec: measured\nScope: section\nSurface: shell\nVerify: local-test'
run deny  'facet declared only mid-line is missing' worker $'Category: feature\nSpec: measured\nItem: tidy. Scope: module.\nSurface: shell\nVerify: local-test'
run allow 'tooling with Payback ignores mid-sentence Category: mechanical' worker $'Item: reword the note that says Category: mechanical goes to Haiku.\nCategory: tooling\nPayback: enables x; cost 5 min'"$FACETS"
run allow 'feature needs no Payback'    worker        "$FEAT"
run deny  'unknown facet value'         worker        $'Category: tooling\nSpec: measured\nScope: whole-repo\nSurface: shell\nVerify: local-test'
run deny  'unknown facet escalated'     worker-high   $'Follows: worker run c3 failed.\nCategory: tooling\nSpec: measured\nScope: module\nSurface: shell\nVerify: eyeball'
run allow 'mechanical to Explore'       Explore       "$MECH"
run allow 'mechanical to general'       ''            "$MECH"
run allow 'mechanical to fork'          fork          "$MECH"
BACKGROUND=false run allow 'killswitch'                  worker-high   "$FEAT" AGENT_NO_AUTHORING_HOOKS=1
printf 'hook adjacent-hook\n' >"$ACTIVATION"
BACKGROUND=false run allow 'inactive identity' worker "$FEAT"
printf 'hook worker-tier-guard\n' >"$ACTIVATION"
BACKGROUND=false run allow 'unavailable interpreter' worker "$FEAT" NORTH_AGENT_PYTHON=/unavailable/python
for input in '{"tool_input": {"subagent_type": "worker"}' '{"tool_name":"Agent","tool_input":false}'; do
  out=$(printf '%s' "$input" | env HOME="$SCRATCH/home" NORTH_AGENT_ACTIVE="$ACTIVATION" "$HOOK")
  if [ -z "$out" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); fi
done
printf 'worker-tier-guard: %s passed, %s failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
