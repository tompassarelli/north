#!/usr/bin/env bash
set -euo pipefail
unset DEBUG PWDEBUG NODE_OPTIONS
if [[ -n "${AUTOMATION_BROWSER_SKIP:-}" ]]; then
  echo "SKIPPED CDP boundary test: $AUTOMATION_BROWSER_SKIP"
  exit 0
fi
root=$(dirname "$(dirname "$(readlink -f "$0")")")
exec bun test "$root/lib/automation-chrome/actions.test.mjs"
