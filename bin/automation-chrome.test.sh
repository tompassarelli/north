#!/usr/bin/env bash
set -euo pipefail
unset DEBUG PWDEBUG NODE_OPTIONS
root=$(dirname "$(dirname "$(readlink -f "$0")")")
exec bun test "$root/lib/automation-chrome/actions.test.mjs"
