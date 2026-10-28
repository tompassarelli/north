#!/usr/bin/env bash
# Claude PreToolUse(Agent) tier gate for lead sessions: a `Category: mechanical`
# brief goes to worker-haiku, a `Category: docs-policy` brief never does, and
# worker-high or worker-xhigh take only escalations. A `Follows:` line marks an escalation and
# allows any worker tier. A Spec/Scope/Surface/Verify facet outside its closed
# vocabulary (threads FACETS) is denied at any worker tier, and so is a
# tooling/infra/ci/harness brief without a `Payback:` line (planning).
# Non-worker agent types always allow.
#
# Kill-switch: `north config agents off worker-tier-guard` or env
# AGENT_NO_AUTHORING_HOOKS (shared impl: lib/authoring-killswitch.sh).
set -uo pipefail

payload="$(head -c 1048576)"
case "$payload" in
  *'"subagent_type"'*'"worker'*|*'spawn_agent"'*) ;;
  *) exit 0 ;;
esac

read -r -d '' PY <<'PYEOF' || true
import json, re, sys

try:
    data = json.loads(sys.stdin.read())
except Exception:
    sys.exit(65)
if not isinstance(data, dict):
    sys.exit(0)
ti = data.get("tool_input") or {}
if not isinstance(ti, dict):
    sys.exit(0)
native = str(data.get("tool_name", "")).endswith("spawn_agent")
if data.get("tool_name") != "Agent" and not native:
    sys.exit(0)
kind = "worker" if native else ti.get("subagent_type")
prompt = ti.get("message") if native else ti.get("prompt")
if not isinstance(kind, str) or not kind.startswith("worker") or not isinstance(prompt, str):
    sys.exit(0)
FACETS = {
    "Spec": ["exact", "measured", "judged"],
    "Scope": ["one-file", "module", "cross-module", "cross-repo"],
    "Surface": ["ts", "rust", "lua", "nix", "shell", "workflow", "docs", "assets"],
    "Verify": ["none", "local-test", "farm", "native", "visual"],
}
reason = None
if not native and ti.get("run_in_background", True) is not True:
    reason = (
        "Claude worker Agents require run_in_background: true so the background Agent result "
        "records the original prompt with the actual child identity at creation."
    )
for field in ("Category", *FACETS):
    if reason:
        break
    if not re.search(rf"(?m)^[ \t]*{field}:[ \t]*[^\s.,;`'<>|)]", prompt):
        reason = f"Missing {field}: declare Category and Spec/Scope/Surface/Verify in the worker brief before spawning."
        break
for facet, values in FACETS.items():
    for m in re.finditer(rf"(?m)^[ \t]*{facet}:[ \t]*([^\s.,;`'<>|)]*)", prompt):
        if m.group(1).lower() not in values:
            reason = (
                f"{facet}: {m.group(1) or '(empty)'} is not a facet value; use one of {' | '.join(values)}, "
                "so north worker-ledger can group runs by it."
            )
            break
    if reason:
        break
infra = re.search(r"(?mi)^[ \t]*Category:\W*(tooling|infra|ci|harness)\b", prompt)
payback = re.search(
    r"(?mi)\bPayback:[ \t]*(saves[ \t]+\d[^\n]*\bpayback[ \t]+[\d.]+[ \t]*days?\b|\S[^\n]*\bcost[ \t]+~?\d)",
    prompt,
)
if reason is None and infra and not payback:
    reason = (
        f"Category: {infra.group(1)} needs a Payback: line priced with the planning skill: "
        "`Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days` "
        "or, for enabling work, `Payback: enables <what>; cost <M> min`."
    )
docs = re.search(r"(?mi)^[ \t]*Category:\W*docs-policy\b", prompt)
if not native and reason is None and docs and kind == "worker-haiku":
    reason = (
        "Category: docs-policy goes to worker, not worker-haiku: Tom wants policy, skill and "
        "documentation prose written carefully by Opus. Haiku keeps only mechanical edits "
        "(renames, link fixes, box ticks) under Category: mechanical."
    )
follows = re.search(r"(?mi)^\W*Follows:\s*\S", prompt)
if reason is None and follows:
    sys.exit(0)
m = None if reason else re.search(r"(?mi)^[ \t]*Category:\W*mechanical\b", prompt)
if reason:
    pass
elif not native and m and kind != "worker-haiku":
    reason = (
        f"Category: mechanical goes to worker-haiku, not {kind} (ledger 28/31 mechanical). "
        "Escalate to worker only after a Haiku failure, and say so with a `Follows:` line "
        "naming the failed run."
    )
elif kind in ("worker-high", "worker-xhigh"):
    reason = (
        f"{kind} is escalation-only: assign worker first, and escalate only after its "
        "reasoning failure, saying so with a `Follows:` line naming the failed run."
    )
if reason is None:
    sys.exit(0)
if data.get("session_id") and data.get("tool_use_id"):
    try:
        sys.path.insert(0, sys.argv[1])
        import spawn_capacity
        spawn_capacity.confirm_spawn(data, launched=False)
    except Exception:
        pass
print(json.dumps({"hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": reason,
}}))
PYEOF

authoring_killswitch="$(dirname "$0")/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] \
  || authoring_killswitch="$(dirname "$0")/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || true
type hook_decide >/dev/null 2>&1 || exit 0
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0
decision="$(hook_decide "${NORTH_AGENT_PYTHON:-python3}" -c "$PY" "$(dirname "$0")/lib")"
[ -n "$decision" ] || exit 0
printf '%s\n' "$decision"
exit 0
