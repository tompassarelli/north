#!/usr/bin/env bash
set -euo pipefail

lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/authoring-killswitch.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
cat >"$scratch/activation.json" <<'JSON'
{
  "schema": "north.agent-activation/v1",
  "catalogDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "generationId": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "units": [{"id": "fixture-guard", "kind": "hook", "permission": "on", "active": true}]
}
JSON

NORTH_AGENT_ACTIVATION="$scratch/activation.json" NORTH_HOOK_ID=fixture-guard \
  bash -c 'source "$1"; ! authoring_guards_off' -- "$lib/authoring-killswitch.sh"
NORTH_AGENT_ACTIVATION="$scratch/activation.json" NORTH_HOOK_ID=fixture-guard AGENT_NO_AUTHORING_HOOKS=1 \
  bash -c 'source "$1"; authoring_guards_off' -- "$lib/authoring-killswitch.sh"
printf '#!/bin/sh\necho DECIDED\n' >"$scratch/decider" && chmod +x "$scratch/decider"
# shellcheck disable=SC2016
[ "$(bash -c 'set -o pipefail; source "$1"; payload=$(head -c 300000 /dev/zero | tr "\0" x); hook_decide "$2"' -- "$lib/authoring-killswitch.sh" "$scratch/decider")" = DECIDED ]

printf 'ok: activation enables guards, the kill switch disables them, and a decider that never reads the payload still decides\n'
