# shellcheck shell=bash
# authoring-killswitch.sh — shared authoring kill-switch entry point.
#
# North's immutable activation generation decides persistent hook activity.
# The session-only AGENT_NO_AUTHORING_HOOKS override remains: 0|false forces
# authoring guards live and every other nonempty value disables them.
# shellcheck source=harness-dial.sh
. "${BASH_SOURCE[0]%/*}/harness-dial.sh"
