#!/usr/bin/env bash
set -uo pipefail
payload=''
oversized=0
while :; do
  chunk=''
  IFS= read -r -N 65536 chunk
  status=$?
  keep=$((1048576 - ${#payload}))
  [ "$keep" -le 0 ] || payload+="${chunk:0:$keep}"
  [ "${#chunk}" -le "$keep" ] || oversized=1
  [ "$status" -eq 0 ] || break
done
[ "$oversized" -eq 0 ] || exit 0
activity="$(dirname "$0")/lib/authoring-killswitch.sh"
[ -r "$activity" ] || activity="$(dirname "$0")/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$activity" 2>/dev/null || exit 0
authoring_guards_off && exit 0
north_agent_unit_active_json hook desktop-action-guard || exit 0
hook_decide "${NORTH_AGENT_PYTHON:-python3}" "$(dirname "$0")/lib/desktop_action.py"
