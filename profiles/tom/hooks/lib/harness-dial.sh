# shellcheck shell=bash
# Hook activity has one authority: North's immutable activation generation.

north_hook_enabled() {
  local hook_id="$1"
  local activation="${NORTH_AGENT_ACTIVATION:-$HOME/.local/state/north/agents/current/activation.json}"
  [[ -r "$activation" ]] || return 0
  python3 - "$hook_id" "$activation" <<'PY'
import json
import os
import sys

hook_id, path = sys.argv[1:]
try:
    with open(path, encoding="utf-8") as handle:
        activation = json.load(handle)
except (OSError, ValueError):
    raise SystemExit(0)

if activation.get("schema") != "north.agent-activation/v1":
    raise SystemExit(0)
units = activation.get("units")
if not isinstance(units, list):
    raise SystemExit(0)
matches = [unit for unit in units
           if isinstance(unit, dict) and unit.get("id") == hook_id]
if len(matches) != 1:
    raise SystemExit(0)
unit = matches[0]
if (unit.get("kind") != "hook"
        or unit.get("category") not in {
            "authoring", "context", "coordination", "dispatch"
        }
        or not isinstance(unit.get("active"), bool)):
    raise SystemExit(0)
active = unit["active"]
if unit["category"] == "authoring":
    override = os.environ.get("AGENT_NO_AUTHORING_HOOKS", "")
    if override in ("0", "false"):
        active = True
    elif override:
        active = False
raise SystemExit(0 if active else 1)
PY
}

authoring_guards_off() {
  local hook_id="${NORTH_HOOK_ID:-${0##*/}}"
  hook_id="${hook_id%.sh}"
  hook_id="${hook_id%.js}"
  if north_hook_enabled "$hook_id"; then
    return 1
  fi
  return 0
}
