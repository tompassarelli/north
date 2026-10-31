#!/usr/bin/env bash
# The prompt-grounding hook checks the kill-switch before any north call and decides fleet membership from this host's session records, never from `north fleet`.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/prompt-grounding-membership.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT
fail() { echo "FAIL $*" >&2; exit 1; }

mkdir -p "$scratch/fleet/child" "$scratch/fleet/other"
printf '{\n  "name": "child",\n  "conversation": "s"\n}\n' >"$scratch/fleet/child/session.json"
printf '{\n  "name": "other",\n  "conversation": "elsewhere"\n}\n' >"$scratch/fleet/other/session.json"
printf '#!/usr/bin/env bash\necho "$*" >>"$CALLS"\necho GROUNDED\n' >"$scratch/north"; chmod +x "$scratch/north"
printf 'hook prompt-grounding\n' >"$scratch/active"
export CALLS="$scratch/calls" NORTH_BIN="$scratch/north" NORTH_FLEET_DIR="$scratch/fleet" NORTH_AGENT_ACTIVE="$scratch/active" CLAUDE_CODE_ENTRYPOINT=cli
unset AGENT_NO_AUTHORING_HOOKS

hook() { printf '{"hook_event_name":"UserPromptSubmit","session_id":"%s","prompt":"plan a trip"}' "$1" | env "${@:2}" bash "$here/prompt-grounding.sh"; }

: >"$CALLS"
[ -z "$(hook s)" ] || fail "a session with a fleet record was grounded"
[ ! -s "$CALLS" ] || fail "fleet membership ran north: $(cat "$CALLS")"
for id in unknown '' '\n' 'x\ny'; do
  [ "$(hook "$id")" = GROUNDED ] || fail "session id '$id' was not grounded beside the decoy fleet records"
done
grep -q '^fleet' "$CALLS" && fail "grounding ran north fleet: $(cat "$CALLS")"
: >"$CALLS"
[ -z "$(hook unknown AGENT_NO_AUTHORING_HOOKS=1)" ] || fail "kill-switch env still grounded"
[ ! -s "$CALLS" ] || fail "kill-switch still ran north: $(cat "$CALLS")"
echo "PASS fleet membership is a local read after the kill-switch, and no fleet record matches an unrelated or malformed session id"
