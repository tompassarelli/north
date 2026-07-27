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

source_text="$(<"$root/bin/north-on-spawn")"
[[ "$source_text" == *'(str "@role:" role-alias)'* ]]
[[ "$source_text" == *'["target" (subs subject (count "@agent:"))]'* ]]

echo "session role alias: 7 / 7 PASS"
