# shellcheck shell=bash
# harness-dial.sh — the ONE resolver for every control-plane dial, and the
# generalization of authoring-killswitch.sh. Sourced by every hook AND by
# north config, so enforcement and the report can never disagree.
#
# Precedence, identical on every axis: item > category > all > default(on).
# Explicit session env beats state entirely. An `off:until=<iso>` whose
# deadline has passed reads as `on` AT ITS OWN LEVEL and stops the search —
# a lapsed TTL restores the guard rather than falling through to a broader
# `off`. Every rule here is asserted by ../harness-dial-cases.tsv, which the
# Clojure report and the TS SDK assert against the same way.
#
# Two categories are special:
#   authoring    — stored under the pre-existing `guards` key, not a second
#                  key, so `north config guards off` keeps meaning exactly
#                  what it means today and no migration is needed.
#   coordination — excluded from the `all` sweep. Sweeping it off destroys
#                  agent identity, presence, and mail while looking like
#                  "I turned off guards"; it must be named explicitly.
#
# Canonical state is ~/.local/state/north/harness.conf. Tests override with
# NORTH_HARNESS_STATE; NORTH_DIAL_NOW injects the instant for until= tests.

north_harness_state_path() {
  if [[ -n "${NORTH_HARNESS_STATE:-}" ]]; then
    builtin printf '%s\n' "$NORTH_HARNESS_STATE"
  elif [[ -n "${AUTHORING_KILLSWITCH_STATE:-}" ]]; then
    builtin printf '%s\n' "$AUTHORING_KILLSWITCH_STATE"
  elif [[ -f "$HOME/.local/state/north/harness.conf" ]]; then
    builtin printf '%s\n' "$HOME/.local/state/north/harness.conf"
  else
    builtin printf '%s\n' "$HOME/.local/state/north/harness.conf"
  fi
}

# Current instant as YYYY-MM-DDTHH:MM:SSZ, without forking. Both this and the
# stored until= use the same canonical shape, so lexicographic comparison is
# chronological comparison.
north_dial_now() {
  local -n __now_out=$1
  if [[ -n "${NORTH_DIAL_NOW:-}" ]]; then
    __now_out="$NORTH_DIAL_NOW"
    return 0
  fi
  local __tz_saved="${TZ-}" __tz_was_set=0
  [[ ${TZ+x} ]] && __tz_was_set=1
  TZ=UTC
  builtin printf -v __now_out '%(%Y-%m-%dT%H:%M:%SZ)T' -1
  if ((__tz_was_set)); then TZ="$__tz_saved"; else unset TZ; fi
}

# Decide at ONE level. Emits on|off for a level that decides, or empty for a
# level that is unset and therefore defers to the next one out.
north_dial_decide_level() {
  local -n __lvl_out=$1
  local __raw="$2" __now="$3" __until
  __lvl_out=''
  case "$__raw" in
    '')  return 0 ;;
    on)  __lvl_out=on ;;
    off) __lvl_out=off ;;
    off:until=*)
      __until="${__raw#off:until=}"
      # Anything but the canonical instant is unreadable, and an unreadable
      # deadline must not silently hold a guard down.
      if [[ $__until =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] \
         && [[ $__now < $__until ]]; then
        __lvl_out=off
      else
        __lvl_out=on
      fi
      ;;
    *) __lvl_out=on ;;
  esac
  return 0
}

# The algebra, isolated so the case table can assert it directly.
# __env is a prior decision (on|off) or empty for "no env opinion".
north_dial_resolve() {
  local -n __res_out=$1
  local __all="$2" __cat="$3" __item="$4" __env="$5" __now="${6:-}"
  local __level=''
  if [[ -n $__env ]]; then
    __res_out="$__env"
    return 0
  fi
  [[ -n $__now ]] || north_dial_now __now
  north_dial_decide_level __level "$__item" "$__now"
  if [[ -n $__level ]]; then __res_out="$__level"; return 0; fi
  north_dial_decide_level __level "$__cat" "$__now"
  if [[ -n $__level ]]; then __res_out="$__level"; return 0; fi
  north_dial_decide_level __level "$__all" "$__now"
  if [[ -n $__level ]]; then __res_out="$__level"; return 0; fi
  __res_out=on
  return 0
}

# The authoring kill-switch env vars, as a resolve() env decision.
north_dial_authoring_env() {
  local -n __env_out=$1
  __env_out=''
  case "${AGENT_NO_AUTHORING_HOOKS:-${CLAUDE_NO_AUTHORING_HOOKS:-}}" in
    0|false) __env_out=on ;;
    ?*)      __env_out=off ;;
  esac
  return 0
}

# Load harness.conf once per process into __NORTH_DIAL_STATE.
declare -gA __NORTH_DIAL_STATE=()
__NORTH_DIAL_LOADED=0
north_dial_load() {
  ((__NORTH_DIAL_LOADED)) && return 0
  __NORTH_DIAL_LOADED=1
  local path line key value
  path="$(north_harness_state_path)" || return 0
  [[ -r $path ]] || return 0
  while IFS= builtin read -r line || [[ -n $line ]]; do
    [[ $line == \#* ]] && continue
    [[ $line == *=* ]] || continue
    key="${line%%=*}"
    value="${line#*=}"
    [[ $key =~ ^[A-Za-z0-9_.-]+$ ]] || continue
    __NORTH_DIAL_STATE["$key"]="$value"
  done <"$path"
  return 0
}

north_dial_raw() {
  local -n __raw_out=$1
  north_dial_load
  __raw_out="${__NORTH_DIAL_STATE[$2]-}"
  return 0
}

# Codex requirements are machine-wide and therefore static, while the agents
# switchboard decides which installed hooks are active. Claude composes only
# active hooks into settings, so it never needs this extra gate. In the managed
# Codex closure the sibling helper is installed by Firn; outside that closure a
# missing helper preserves the pre-switchboard behavior.
north_switchboard_hook_active() {
  local __id="$1"
  local __activity_lib="${AGENTS_SWITCHBOARD_ACTIVITY_LIB:-${BASH_SOURCE[0]%/*}/switchboard-activity.sh}"
  [[ -r $__activity_lib ]] || return 0
  if ! type agents_switchboard_active >/dev/null 2>&1; then
    # shellcheck disable=SC1090
    builtin source "$__activity_lib" 2>/dev/null || return 0
  fi
  agents_switchboard_active hook "$__id"
}

# Category of a hook id, from the registry beside this lib. Unknown ids get
# no category, which means they answer to the `all` sweep only.
north_dial_hook_category() {
  local -n __cat_out=$1
  local __id="$2" __registry __rid __rcat __rest
  __cat_out=''
  __registry="${NORTH_HOOK_REGISTRY:-$(builtin printf '%s' "${BASH_SOURCE[0]%/*}")/../registry.tsv}"
  [[ -r $__registry ]] || return 0
  while IFS=$'\t' builtin read -r __rid __rcat __rest || [[ -n $__rid ]]; do
    [[ $__rid == \#* || -z $__rid || $__rid == id ]] && continue
    if [[ $__rid == "$__id" ]]; then __cat_out="$__rcat"; return 0; fi
  done <"$__registry"
  return 0
}

# Is this hook live right now? The one question every hook asks at line ~1.
north_hook_enabled() {
  local __id="$1" __cat __all __catraw __itemraw __env __verdict
  north_switchboard_hook_active "$__id" || return 1
  north_dial_hook_category __cat "$__id"
  north_dial_authoring_env __env
  # The env kill-switch speaks only for authoring guards; it must not reach
  # across and silence dispatch or coordination.
  [[ $__cat == authoring ]] || __env=''
  north_dial_raw __all hooks
  # coordination is never swept by `all` — it must be named.
  [[ $__cat == coordination ]] && __all=''
  if [[ $__cat == authoring ]]; then
    north_dial_raw __catraw guards
  elif [[ -n $__cat ]]; then
    north_dial_raw __catraw "hooks.cat.$__cat"
  else
    __catraw=''
  fi
  north_dial_raw __itemraw "hooks.hook.$__id"
  north_dial_resolve __verdict "$__all" "$__catraw" "$__itemraw" "$__env"
  [[ $__verdict == on ]]
}

# Backward-compatible entrypoint used by the existing hooks. A registered
# caller gets the complete item > category > all resolution; an unknown caller
# retains the original authoring-only behavior so external consumers of this
# compatibility function do not acquire a new global switch by surprise.
authoring_guards_off() {
  local id="${NORTH_HOOK_ID:-${0##*/}}" category
  id="${id%.sh}"
  id="${id%.js}"
  north_dial_hook_category category "$id"
  if [[ -n $category ]]; then
    if north_hook_enabled "$id"; then
      return 1
    fi
    return 0
  fi

  local env verdict all cat
  north_dial_authoring_env env
  north_dial_raw all hooks
  north_dial_raw cat guards
  north_dial_resolve verdict "$all" "$cat" '' "$env"
  [[ $verdict == off ]]
}
