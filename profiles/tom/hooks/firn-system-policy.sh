#!/usr/bin/env bash
set -uo pipefail

payload="$(cat 2>/dev/null || true)"

# shellcheck source=lib/harness-dial.sh
if source "${BASH_SOURCE[0]%/*}/lib/harness-dial.sh" 2>/dev/null; then
  north_hook_enabled firn-system-policy || exit 0
fi

policy="${FIRN_SYSTEM_POLICY:-/run/current-system/sw/bin/firn-system-policy}"
[[ -x "$policy" ]] || exit 0
printf '%s' "$payload" | exec "$policy" "$@"
