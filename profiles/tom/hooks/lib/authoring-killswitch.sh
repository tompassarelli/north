# shellcheck shell=bash
# authoring-killswitch.sh — compatibility shim.
#
# The implementation moved to harness-dial.sh when the authoring kill-switch
# generalized into the control-plane dial. This file stays, and stays sourced
# by name, because six guard hooks and `north config` reference it directly;
# routing them through one shim is cheaper and safer than editing six live
# enforcement scripts to gain behavior they already have.
#
# `authoring_guards_off` keeps its exact prior meaning: env beats state,
# AGENT_NO_AUTHORING_HOOKS=0|false forces guards live, CLAUDE_NO_AUTHORING_HOOKS
# remains an alias, and `guards=off` disables the authoring category.
# Per-hook and per-category control arrives through north_hook_enabled, which
# hooks adopt one at a time.
# shellcheck source=harness-dial.sh
. "${BASH_SOURCE[0]%/*}/harness-dial.sh"
