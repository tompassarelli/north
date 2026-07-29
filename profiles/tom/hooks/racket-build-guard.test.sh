#!/usr/bin/env bash
# Owner behavior plus North's policy-adapter seam.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OWNER_TEST="$HERE/../../../../../beagle/main/integrations/north/hooks/racket-build-guard.test.sh"

bash "$OWNER_TEST" || exit $?

scratch="$(mktemp -d "${TMPDIR:-/tmp}/north-racket-build-adapter.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/home" "$scratch/project"
touch "$scratch/project/main.rkt"
printf '%s\n' 'hooks.hook.racket-build-guard=off' >"$scratch/harness.conf"

payload="$(
  printf '{"hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{"file_path":"%s"}}' \
    "$scratch/project/main.rkt"
)"
out="$(
  printf '%s' "$payload" |
    env HOME="$scratch/home" NORTH_HARNESS_STATE="$scratch/harness.conf" \
      AGENT_NO_AUTHORING_HOOKS=0 \
      "$HERE/racket-build-guard.sh"
)"

if [ -z "$out" ]; then
  printf '%s\n' 'PASS  North item dial silences the build hook'
else
  printf 'FAIL  North item dial emitted: %s\n' "$out" >&2
  exit 1
fi
