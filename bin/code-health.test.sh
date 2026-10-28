#!/usr/bin/env bash
set -euo pipefail
root=$(dirname "$(dirname "$(readlink -f "$0")")")
exec bun test "$root/harness/code-health"
