#!/usr/bin/env bash
# Owner behavior plus North's policy-adapter seam.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OWNER_TEST="$HERE/../../../../../beagle/main/integrations/north/hooks/beagle-session-start.test.sh"

bash "$OWNER_TEST" || exit $?

scratch="$(mktemp -d "${TMPDIR:-/tmp}/north-beagle-session-adapter.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/home" "$scratch/project" "$scratch/session-state"
touch "$scratch/project/main.bnix"
printf '%s\n' 'hooks.hook.beagle-session-start=off' >"$scratch/harness.conf"

payload="$(
  printf '{"hook_event_name":"SessionStart","session_id":"north-adapter","source":"startup","cwd":"%s"}' \
    "$scratch/project"
)"
out="$(
  printf '%s' "$payload" |
    env HOME="$scratch/home" NORTH_HARNESS_STATE="$scratch/harness.conf" \
      AGENT_NO_AUTHORING_HOOKS=0 \
      BEAGLE_SESSION_STATE_DIR="$scratch/session-state" \
      "$HERE/beagle-session-start.sh"
)"

if [ -z "$out" ]; then
  printf '%s\n' 'PASS  North item dial silences the context hook'
else
  printf 'FAIL  North item dial emitted: %s\n' "$out" >&2
  exit 1
fi
