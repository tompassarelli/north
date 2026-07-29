#!/usr/bin/env bash
# harness-dial.test.sh — asserts the bash resolver against the shared
# precedence contract. The Clojure report and the TS SDK assert against the
# same table; that is what stops three readers of one state file from drifting.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/harness-dial.sh
. "$HERE/lib/harness-dial.sh"

CASES="$HERE/harness-dial-cases.tsv"
pass=0 fail=0

fail_case() {
  printf 'FAIL  %-40s %s\n' "$1" "$2"
  fail=$((fail + 1))
}

# --- the precedence algebra, straight off the shared table ------------------
while IFS=$'\t' read -r id all cat item env now expect || [[ -n $id ]]; do
  [[ $id == \#* || -z $id || $id == id ]] && continue
  [[ $all  == - ]] && all=''
  [[ $cat  == - ]] && cat=''
  [[ $item == - ]] && item=''
  [[ $now  == - ]] && now=''

  unset AGENT_NO_AUTHORING_HOOKS CLAUDE_NO_AUTHORING_HOOKS
  if [[ $env != - ]]; then
    # `VAR=` must land as set-but-empty, which is not the same as unset.
    export "${env%%=*}=${env#*=}"
  fi

  north_dial_authoring_env env_decision
  north_dial_resolve got "$all" "$cat" "$item" "$env_decision" "$now"

  if [[ $got == "$expect" ]]; then
    pass=$((pass + 1))
  else
    fail_case "$id" "expected $expect, got $got"
  fi
done <"$CASES"
unset AGENT_NO_AUTHORING_HOOKS CLAUDE_NO_AUTHORING_HOOKS

# --- registry integration: the two special categories -----------------------
scratch="$(mktemp -d)"
trap 'rm -rf "${scratch:?}"' EXIT
export NORTH_HARNESS_STATE="$scratch/harness.conf"

reload() {
  __NORTH_DIAL_LOADED=0
  __NORTH_DIAL_STATE=()
}

expect_hook() {
  local id="$1" want="$2" label="$3"
  reload
  if north_hook_enabled "$id"; then got=on; else got=off; fi
  if [[ $got == "$want" ]]; then
    pass=$((pass + 1))
  else
    fail_case "$label" "hook $id expected $want, got $got"
  fi
}

printf 'hooks=off\n' >"$NORTH_HARNESS_STATE"
expect_hook tripwire-guard          off "all-sweeps-authoring"
expect_hook north-session-end       on  "all-never-sweeps-coordination"
expect_hook hook-detach             on  "all-never-sweeps-coordination-2"

printf 'guards=off\n' >"$NORTH_HARNESS_STATE"
expect_hook tripwire-guard          off "guards-is-authoring-category"
expect_hook agent-spawn-guard       on  "guards-does-not-reach-dispatch"
expect_hook north-clock-guard       on  "guards-does-not-reach-billing"

printf 'guards=off\nhooks.hook.tripwire-guard=on\n' >"$NORTH_HARNESS_STATE"
expect_hook tripwire-guard          on  "item-on-beats-guards-off"
expect_hook firn-guard              off "sibling-verdict-unchanged"

printf 'hooks.cat.coordination=off\n' >"$NORTH_HARNESS_STATE"
expect_hook north-session-end       off "coordination-off-when-named"

printf 'hooks.hook.north-clock-guard=off:until=2099-01-01T00:00:00Z\n' >"$NORTH_HARNESS_STATE"
expect_hook north-clock-guard       off "future-ttl-holds"
printf 'hooks.hook.north-clock-guard=off:until=2020-01-01T00:00:00Z\n' >"$NORTH_HARNESS_STATE"
expect_hook north-clock-guard       on  "lapsed-ttl-restores-guard"

# --- the env var must not reach across categories --------------------------
printf '' >"$NORTH_HARNESS_STATE"
export AGENT_NO_AUTHORING_HOOKS=1
expect_hook tripwire-guard          off "env-kills-authoring"
expect_hook agent-spawn-guard       on  "env-does-not-kill-dispatch"
expect_hook north-clock-guard       on  "env-does-not-kill-billing"
unset AGENT_NO_AUTHORING_HOOKS

# --- authoring_guards_off keeps its exact present meaning ------------------
printf 'guards=off\n' >"$NORTH_HARNESS_STATE"
reload
if authoring_guards_off; then pass=$((pass + 1)); else fail_case "compat-guards-off" "expected off"; fi
printf 'guards=on\n' >"$NORTH_HARNESS_STATE"
reload
if authoring_guards_off; then fail_case "compat-guards-on" "expected live"; else pass=$((pass + 1)); fi
printf '' >"$NORTH_HARNESS_STATE"
reload
if authoring_guards_off; then fail_case "compat-default" "expected live"; else pass=$((pass + 1)); fi

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[[ $fail -eq 0 ]]
