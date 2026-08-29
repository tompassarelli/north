#!/usr/bin/env bash
set -uo pipefail

payload_limit=1048576
output_limit=65536
scratch=''

cleanup() {
  [[ -z "$scratch" ]] || rm -rf -- "${scratch:?}"
}
trap cleanup EXIT

# Keep the provider pipe flowing even when the envelope is too large, but never
# retain more than the Firn core's one-MiB input contract plus the overflow byte.
umask 077
scratch="$(mktemp -d "${TMPDIR:-/tmp}/north-firn-system-policy.XXXXXX" 2>/dev/null)" \
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

# shellcheck source=lib/harness-dial.sh
if source "${BASH_SOURCE[0]%/*}/lib/harness-dial.sh" 2>/dev/null; then
  north_hook_enabled firn-system-policy || exit 0
fi

policy="${FIRN_SYSTEM_POLICY:-/home/tom/.local/lib/firn/policy/current/bin/firn-system-policy}"
[[ -x "$policy" ]] || exit 0

# A failed core must not leave a partial deny on stdout. Its successful output
# is bounded independently before the adapter releases any bytes to the caller.
(
  ulimit -f 64 2>/dev/null || exit 1
  "$policy" "$@" <"$payload" >"$output" 2>/dev/null
) || exit 0
output_bytes="$(wc -c <"$output" 2>/dev/null)" || exit 0
[[ "$output_bytes" =~ ^[0-9]+$ ]] || exit 0
(( output_bytes <= output_limit )) || exit 0
cat -- "$output" 2>/dev/null || exit 0
