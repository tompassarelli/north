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

printf 'ok: activation enables guards and the explicit kill switch disables them\n'
