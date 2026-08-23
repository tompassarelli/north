#!/usr/bin/env bash
set -uo pipefail

provider_hooks="${NORTH_AGENT_PROVIDER_HOOKS:-${NORTH_AGENT_STATE_ROOT:-$HOME/.local/state/north/agents}/current/provider-hooks}"
# shellcheck source=lib/harness-dial.sh
if source "$provider_hooks/lib/harness-dial.sh" 2>/dev/null; then
  north_hook_enabled code-upstream-guard || exit 0
fi

BEAGLE_HOME="${BEAGLE_HOME:-$HOME/code/beagle/main}"
exec "$BEAGLE_HOME/store/integrations/north/hooks/code-upstream-guard.sh" "$@"
