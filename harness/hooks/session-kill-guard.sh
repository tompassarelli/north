#!/usr/bin/env bash
# PreToolUse guard: refuses session-killing signal shapes, user-manager teardown
# (locally or inside an ssh remote command) and unmanaged background-child launches.
# The decider is `north hook session-kill` (src/guard/session_kill.rs).
# Kill-switch: `north config agents off session-kill-guard` or AGENT_NO_AUTHORING_HOOKS.
here="${0%/*}"
if [ -r "$here/lib/authoring-killswitch.sh" ]; then
  . "$here/lib/authoring-killswitch.sh"
else
  . "$here/../lib/authoring-killswitch.sh"
fi
if authoring_guards_off; then
  cat >/dev/null
  exit 0
fi
exec north hook session-kill
