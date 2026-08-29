#!/usr/bin/env bash
set -euo pipefail

here="${BASH_SOURCE[0]%/*}"
adapter="$here/firn-system-policy.sh"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/north-firn-system-policy-test.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT

policy="$scratch/policy"
activation_probe="$scratch/activation-probe"
printf '#!/usr/bin/env bash\nexit 0\n' >"$activation_probe"
chmod 700 "$activation_probe"
cat >"$policy" <<'POLICY'
#!/usr/bin/env bash
set -euo pipefail
cat >"$POLICY_INPUT"
case "${POLICY_MODE:-pass}" in
  pass) printf '%s' "$POLICY_OUTPUT" ;;
  fail) printf '%s' "$POLICY_OUTPUT"; exit 9 ;;
esac
POLICY
chmod 700 "$policy"

receipt='{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"{\"schema\":\"InvocationObservation/v1\",\"hook\":\"firn-system-policy\",\"operation\":\"functions.get_goal\",\"classification\":\"empty-object\",\"decision\":\"pass\"}"}}'
payload='{"tool_name":"functions.get_goal","tool_input":{},"session_id":"fixture"}'
POLICY_INPUT="$scratch/input" POLICY_OUTPUT="$receipt" FIRN_SYSTEM_POLICY="$policy" \
  NORTH_AGENT_PYTHON="$activation_probe" \
  "$adapter" <<<"$payload" >"$scratch/output"
[[ "$(cat "$scratch/output")" == "$receipt" ]]
[[ "$(cat "$scratch/input")" == "$payload" ]]

# The adapter drains the producer after retaining the overflow byte, invokes no
# policy, and emits nothing for an oversized envelope.
rm -f "$scratch/input" "$scratch/drained"
(
  set -e
  head -c 1048577 /dev/zero
  : >"$scratch/drained"
) | POLICY_INPUT="$scratch/input" POLICY_OUTPUT="$receipt" FIRN_SYSTEM_POLICY="$policy" \
  NORTH_AGENT_PYTHON="$activation_probe" \
  "$adapter" >"$scratch/output"
[[ -f "$scratch/drained" ]]
[[ ! -e "$scratch/input" ]]
[[ ! -s "$scratch/output" ]]

# A core failure may have produced a denial prefix; none of it crosses the
# adapter boundary.
POLICY_INPUT="$scratch/input" POLICY_OUTPUT="$receipt" POLICY_MODE=fail \
  FIRN_SYSTEM_POLICY="$policy" NORTH_AGENT_PYTHON="$activation_probe" \
  "$adapter" <<<"$payload" >"$scratch/output"
[[ ! -s "$scratch/output" ]]

echo "firn-system-policy adapter: PASS"
