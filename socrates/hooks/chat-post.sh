#!/usr/bin/env bash
set -uo pipefail
here="${BASH_SOURCE[0]%/*}"
authoring_killswitch="$here/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] || authoring_killswitch="$here/../lib/authoring-killswitch.sh"
. "$authoring_killswitch" 2>/dev/null || true
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0
timeout 0.18s "${NORTH_AGENT_PYTHON:-python3}" "$here/lib/chat_post.py" >/dev/null 2>&1 || true
exit 0
