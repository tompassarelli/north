#!/usr/bin/env bash
set -uo pipefail
[ -n "${AGENT_ORG_NAME:-}" ] || exit 0
here="${BASH_SOURCE[0]%/*}"
authoring_killswitch="$here/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] || authoring_killswitch="$here/../lib/authoring-killswitch.sh"
. "$authoring_killswitch" 2>/dev/null || exit 0
authoring_guards_off && exit 0
payload="$(head -c 1048577)"
[ "${#payload}" -le 1048576 ] || exit 0
hook_decide "${NORTH_AGENT_PYTHON:-python3}" "$here/lib/inbox_surface.py"
exit 0
