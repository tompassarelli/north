#!/usr/bin/env bash
set -uo pipefail

# Provider hook manifests require an executable command file. All parsing and
# admission decisions live in the typed Beagle source beside this shim.
entry_dir="$(cd "$(dirname "$0")" && pwd)"
source_path="$(readlink -f "$0" 2>/dev/null || printf '%s' "$0")"
program="${source_path%.sh}.js"
bun="${NORTH_BUN:-$entry_dir/runtime/bun}"
if [ ! -x "$bun" ]; then
  bun="$(command -v bun 2>/dev/null || true)"
fi
[ -n "$bun" ] && [ -r "$program" ] || exit 0
exec "$bun" "$program"
