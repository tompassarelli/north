#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d -t north-role-alias.XXXXXX)"
cleanup() {
  status=$?
  trap - EXIT
  rm -rf "${scratch:?}"
  exit "$status"
}
trap cleanup EXIT

mkdir -p "$scratch/bin" "$scratch/runtime"
fake_bb="$scratch/bin/bb"
role_log="$scratch/role.log"

apply_fake_bb() {
  cp "$root/bin/tests/fixtures/fake-bb-role-alias.sh" "$fake_bb"
  chmod +x "$fake_bb"
}

apply_fake_bb

payload="$(printf '{"cwd":"%s","session_id":"role-alias-session","hook_event_name":"SessionStart","model":"test-model"}' "$root")"
output="$(
  printf '%s' "$payload" |
    PATH="$scratch/bin:$PATH" \
    ROLE_ALIAS_TEST_LOG="$role_log" \
    XDG_RUNTIME_DIR="$scratch/runtime" \
    NORTH_ORCHESTRATION_ROLE="integrator" \
    NORTH_PORT="59999" \
    "$root/bin/north-on-spawn"
)"

for _ in $(seq 1 100); do
  [[ -s "$role_log" ]] && break
  sleep 0.02
done

if [[ ! -s "$role_log" ]]; then
  echo "FAIL session-start projection did not receive role alias inputs" >&2
  exit 1
fi

IFS=$'\t' read -r repo role alias subject <"$role_log"
[[ "$repo" == "north" ]]
[[ "$role" == "integrator" ]]
[[ "$alias" == "north-integrator" ]]
[[ "$subject" == @agent:native-* ]]
[[ "$output" == *"north coordination active"* ]]
[[ "$output" == *'durable role alias is \"north-integrator\"'* ]]
[[ "$output" == *"Address peers ALIAS-FIRST"* ]]
[[ "$output" == *"FAILS LOUDLY"* ]]
[[ "$output" == *"\`--dead-drop\` is only"* ]]

source_text="$(<"$root/bin/north-on-spawn")"
[[ "$source_text" == *'(str "@role:" role-alias)'* ]]
[[ "$source_text" == *'["target" (subs subject (count "@agent:"))]'* ]]

route_cache="$(
  find "$scratch/runtime/north-agent-routes" -maxdepth 1 -type f \
    ! -name '*.seed' -print -quit
)"
[[ -n "$route_cache" ]]
rm -f -- "${route_cache:?}"
: >"$role_log"

foreign_repo="$scratch/foreign-repo"
git init -q "$foreign_repo"
git -C "$foreign_repo" remote add origin /tmp/other-repository
tool_payload="$(printf '{"cwd":"%s","session_id":"role-alias-session","hook_event_name":"PostToolUse"}' "$foreign_repo")"
printf '%s' "$tool_payload" |
  PATH="$scratch/bin:$PATH" \
  ROLE_ALIAS_TEST_LOG="$role_log" \
  XDG_RUNTIME_DIR="$scratch/runtime" \
  NORTH_ORCHESTRATION_ROLE="integrator" \
  NORTH_PORT="59999" \
  "$root/bin/north-on-tooluse" >/dev/null

for _ in $(seq 1 100); do
  [[ -s "$role_log" && -s "$route_cache" ]] && break
  sleep 0.02
done

IFS=$'\t' read -r repair_repo repair_role repair_alias repair_subject <"$role_log"
[[ "$repair_repo" == "north" ]]
[[ "$repair_role" == "integrator" ]]
[[ "$repair_alias" == "north-integrator" ]]
[[ "$repair_subject" == "$subject" ]]
[[ "$(sed -n '4p' "$route_cache")" == "integrator" ]]
[[ "$(sed -n '5p' "$route_cache")" == "north-integrator" ]]

echo "session role alias, repair, and doctrine: 18 / 18 PASS"
