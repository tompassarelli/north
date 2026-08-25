#!/usr/bin/env bash
# North-owned policy adapter around Beagle's owner-local hook.
set -uo pipefail

SCRIPT_DIR="$(
  CDPATH='' builtin cd -- "${BASH_SOURCE[0]%/*}" 2>/dev/null &&
    builtin pwd -P
)" || exit 0

export NORTH_HOOK_ID=beagle-session-start
export BEAGLE_AUTHORING_KILLSWITCH_LIB="$SCRIPT_DIR/lib/authoring-killswitch.sh"
BEAGLE_HOME="${BEAGLE_HOME:-$HOME/code/beagle/main}"
exec "$BEAGLE_HOME/integrations/north/hooks/beagle-session-start.sh" "$@"
