#!/usr/bin/env bash
set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "${scratch:?}"' EXIT
export HOME="$scratch/home"
export NORTH_AGENT_ACTIVATION="$scratch/activation.json"

# shellcheck source=lib/harness-dial.sh
source "$here/lib/harness-dial.sh"

cat >"$NORTH_AGENT_ACTIVATION" <<'JSON'
{"schema":"north.agent-activation/v1","units":[
  {"id":"tripwire-guard","kind":"hook","category":"authoring","active":true},
  {"id":"agent-spawn-guard","kind":"hook","category":"dispatch","active":false}
]}
JSON

pass=0
fail=0
expect() {
  local id="$1" wanted="$2" got=off
  if north_hook_enabled "$id"; then got=on; fi
  if [[ "$got" == "$wanted" ]]; then
    pass=$((pass + 1))
  else
    printf 'FAIL  %s expected %s got %s\n' "$id" "$wanted" "$got"
    fail=$((fail + 1))
  fi
}

expect tripwire-guard on
expect agent-spawn-guard off
expect missing-hook on

AGENT_NO_AUTHORING_HOOKS=1 expect tripwire-guard off
AGENT_NO_AUTHORING_HOOKS=0 expect tripwire-guard on

printf '%s\n' '{}' >"$NORTH_AGENT_ACTIVATION"
expect tripwire-guard on

mv "$NORTH_AGENT_ACTIVATION" "$scratch/absent.json"
expect tripwire-guard on

alternate_root="$scratch/alternate-state"
mkdir -p "$alternate_root/current"
cat >"$alternate_root/current/activation.json" <<'JSON'
{"schema":"north.agent-activation/v1","units":[
  {"id":"tripwire-guard","kind":"hook","category":"authoring","active":false}
]}
JSON

# The exact-file override wins even when the configured state root has a verdict.
NORTH_AGENT_STATE_ROOT="$alternate_root" expect tripwire-guard on
unset NORTH_AGENT_ACTIVATION
NORTH_AGENT_STATE_ROOT="$alternate_root" expect tripwire-guard off

printf '%d passed, %d failed\n' "$pass" "$fail"
((fail == 0))
