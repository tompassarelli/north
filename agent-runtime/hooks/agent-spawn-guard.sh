#!/usr/bin/env bash
set -uo pipefail

payload_limit=1048576
output_limit=65536
scratch=''

cleanup() {
  [[ -z "$scratch" ]] || rm -rf -- "${scratch:?}"
}
trap cleanup EXIT

# Drain the provider pipe before consulting optional activation state, while
# retaining at most the typed runtime's one-MiB input contract plus one byte.
umask 077
scratch="$(mktemp -d "${TMPDIR:-/tmp}/north-agent-spawn-guard.XXXXXX" 2>/dev/null)" \
  || { cat >/dev/null 2>&1 || true; exit 0; }
payload="$scratch/payload"
output="$scratch/output"
if ! head -c "$((payload_limit + 1))" >"$payload" 2>/dev/null; then
  cat >/dev/null 2>&1 || true
  exit 0
fi
cat >/dev/null 2>&1 || true
payload_bytes="$(wc -c <"$payload" 2>/dev/null)" || exit 0
[[ "$payload_bytes" =~ ^[0-9]+$ ]] || exit 0
(( payload_bytes <= payload_limit )) || exit 0

case "${AGENT_NO_AUTHORING_HOOKS:-}" in
  ''|0|false) ;;
  *) exit 0 ;;
esac

# Provider hook manifests require an executable command file. All parsing and
# admission decisions live in the typed Beagle source beside this shim.
entry_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/harness-dial.sh
# shellcheck disable=SC1091
if ! source "$entry_dir/lib/harness-dial.sh" 2>/dev/null; then
  exit 0
fi
NORTH_AGENT_PYTHON="${NORTH_AGENT_PYTHON:-$entry_dir/runtime/python3}"
north_hook_status agent-spawn-guard >/dev/null 2>&1 || exit 0

source_path="$(readlink -f "$0" 2>/dev/null || printf '%s' "$0")"
program="${source_path%.sh}.js"
bun="${NORTH_BUN:-$entry_dir/runtime/bun}"
if [ ! -x "$bun" ]; then
  bun="$(command -v bun 2>/dev/null || true)"
fi
[ -n "$bun" ] && [ -r "$program" ] || exit 0

# Release output only after the typed runtime succeeds. Optional-runtime,
# parse, and internal failures are silent fail-open outcomes.
(
  ulimit -f 64 2>/dev/null || exit 1
  "$bun" "$program" <"$payload" >"$output" 2>/dev/null
) || exit 0
output_bytes="$(wc -c <"$output" 2>/dev/null)" || exit 0
[[ "$output_bytes" =~ ^[0-9]+$ ]] || exit 0
(( output_bytes <= output_limit )) || exit 0
cat -- "$output" 2>/dev/null || exit 0
