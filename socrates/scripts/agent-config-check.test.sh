#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
checker="$repo/scripts/agent-config-check.sh"
firn="$(jq -er '."nixos-config" // empty' <<<"${NORTH_REPO_ROOTS:-null}" 2>/dev/null || printf '%s' "$HOME/code/nixos-config/main")"
requirements="$firn/modules/codex/requirements.toml"
module="$firn/modules/codex/default.bnix"
catalog="${AGENTS_OPERATOR_DIR:-$HOME/code/south/main/agents}/catalog-config.json"
config="$firn/dotfiles/codex/config.toml"

bash -n "$checker"
# shellcheck source=agent-config-check.sh
source "$checker"

if grep -Fq '[mcp_servers.north]' "$config"; then
  printf 'retired North MCP declaration remains\n' >&2
  exit 1
fi

"$AGENT_CONFIG_PYTHON" - "$catalog" <<'PY'
import json
import pathlib
import sys

catalog = json.loads(pathlib.Path(sys.argv[1]).read_text())


def owners(value):
    if isinstance(value, dict):
        if isinstance(value.get("repo"), str) and isinstance(value.get("path"), str):
            yield value
        for nested in value.values():
            yield from owners(nested)
    elif isinstance(value, list):
        for nested in value:
            yield from owners(nested)


retired = {
    "coordination",
    "logcompress-hook",
    "north-mark-delegated",
    "north-on-spawn",
    "north-on-stop",
    "north-on-terminal",
    "north-on-tooluse",
}
assert retired.isdisjoint(catalog["rootOrder"])
assert retired.isdisjoint(catalog["activation"])
assert all(
    value["repo"] == "nixos-config"
    or (value["repo"] == "north" and value["path"].startswith(("agent-machinery/", "socrates/")))
    or (value["repo"] == "south" and value["path"].startswith("agents/"))
    for value in owners(catalog)
)
assert catalog["baselines"][1]["owner"] == {
    "repo": "north",
    "path": "socrates/code/AGENTS.md",
}
PY

if rg -n \
  'north-mcp|north-coordinator|north-on-|north-mark-delegated|harness-dial|/var/lib/north-enforcement' \
  "$module" "$requirements" "$config" "$catalog"; then
  printf 'retired North-v1 runtime wiring remains\n' >&2
  exit 1
fi

scratch="$(mktemp -d "${TMPDIR:-/tmp}/agent-config-check.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
cat >"$scratch/activation.json" <<'JSON'
{
  "schema": "north.agent-activation/v1",
  "catalogDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "generationId": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "units": [
    {
      "id": "fixture-guard",
      "kind": "hook",
      "permission": "on",
      "active": true
    }
  ]
}
JSON

NORTH_AGENT_ACTIVATION="$scratch/activation.json" \
NORTH_HOOK_ID=fixture-guard \
  bash -c 'source "$1"; ! authoring_guards_off' -- \
    "$repo/lib/authoring-killswitch.sh"
NORTH_AGENT_ACTIVATION="$scratch/activation.json" \
NORTH_HOOK_ID=fixture-guard \
AGENT_NO_AUTHORING_HOOKS=1 \
  bash -c 'source "$1"; authoring_guards_off' -- \
    "$repo/lib/authoring-killswitch.sh"

bash -n "$repo/hooks/firn-system-policy"
printf 'ok: North activation and Codex guard wiring contain no North-v1 runtime\n'
