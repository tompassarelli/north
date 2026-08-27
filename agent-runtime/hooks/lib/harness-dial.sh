# shellcheck shell=bash
# Hook activity has one authority: North's immutable activation generation.

north_config_raw() {
  local -n output=$1
  local key=$2 line path
  output=''
  path="${NORTH_HARNESS_STATE:-$HOME/.local/state/north/harness.conf}"
  [[ -r $path ]] || return 0
  while IFS= read -r line || [[ -n $line ]]; do
    [[ $line == "$key="* ]] && output="${line#*=}"
  done <"$path"
}

north_hook_status() {
  local hook_id="$1" python_bin
  local activation="${NORTH_AGENT_ACTIVATION:-${NORTH_AGENT_STATE_ROOT:-$HOME/.local/state/north/agents}/current/activation.json}"
  python_bin="${NORTH_AGENT_PYTHON:-python3}"
  [[ -r "$activation" ]] || return 2
  "$python_bin" - "$hook_id" "$activation" <<'PY'
import json
import os
import sys

hook_id, path = sys.argv[1:]
try:
    with open(path, encoding="utf-8") as handle:
        activation = json.load(handle)
except (OSError, ValueError):
    raise SystemExit(2)

if activation.get("schema") != "north.agent-activation/v1":
    raise SystemExit(2)
units = activation.get("units")
if not isinstance(units, list):
    raise SystemExit(2)
matches = [unit for unit in units
           if isinstance(unit, dict) and unit.get("id") == hook_id]
if len(matches) != 1:
    raise SystemExit(2)
unit = matches[0]
if (unit.get("kind") != "hook"
        or unit.get("category") not in {
            "authoring", "context", "coordination", "dispatch"
        }
        or not isinstance(unit.get("active"), bool)):
    raise SystemExit(2)
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

north_hook_enabled() {
  local status
  north_hook_status "$1"
  status=$?
  case "$status" in
    0|1) return "$status" ;;
    *) return 0 ;;
  esac
}

authoring_guards_off() {
  local hook_id="${NORTH_HOOK_ID:-${0##*/}}" status
  hook_id="${hook_id%.sh}"
  hook_id="${hook_id%.js}"
  north_hook_status "$hook_id"
  status=$?
  [[ "$status" -eq 0 ]] && return 1
  return 0
}
