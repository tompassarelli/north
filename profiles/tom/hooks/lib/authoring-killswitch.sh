# shellcheck shell=bash
# authoring-killswitch.sh — shared authoring kill-switch entry point.
#
# The implementation moved to harness-dial.sh when the authoring kill-switch
# generalized into the control-plane dial. This file stays, and stays sourced
# by name, because six guard hooks and `north config` reference it directly.
#
# Registered hook callers now receive the complete shared dial: item beats
# category, category beats all, and the authoring env override still beats
# state. Unknown external callers retain the original authoring-only behavior.
# AGENT_NO_AUTHORING_HOOKS=0|false forces authoring guards live.
# shellcheck source=harness-dial.sh
. "${BASH_SOURCE[0]%/*}/harness-dial.sh"
