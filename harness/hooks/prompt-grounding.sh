#!/usr/bin/env bash
# Claude Code UserPromptSubmit hook: before Tom's proxy answers, injects a Grounding block naming the
# existing skills, policy, hooks, decisions, live usage/org state and recent history that match his
# message (north grounding hook, tier A over a precomputed index), or the tier-B command to run.
# north grounding hook decides who is grounded: a role-proxy fleet session for a Tom message, else
# only Tom's top-level interactive session.
#
# Kill-switch: `north config agents off prompt-grounding` or env
# AGENT_NO_AUTHORING_HOOKS (shared impl: lib/authoring-killswitch.sh).
set -uo pipefail

payload="$(head -c 1048577)"
[ "${#payload}" -le 1048576 ] || exit 0
case "$payload" in
  *'"agent_id"'*|*'"agent_type"'*) exit 0 ;;
esac

here="${BASH_SOURCE[0]%/*}"
authoring_killswitch="$here/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] || authoring_killswitch="$here/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || exit 0
authoring_guards_off && exit 0

north="${NORTH_BIN:-${NORTH_GROUNDING_BIN:-$(command -v north || true)}}"
[ -x "$north" ] || exit 0

hook_decide "$north" grounding hook
exit 0
