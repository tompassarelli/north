#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d -t north-agent-write-controls.XXXXXX)"
trap 'rm -rf "${scratch:?}"' EXIT
fake_fram="$scratch/fram"
calls="$scratch/calls"
bb_calls="$scratch/bb-calls"
runtime="$scratch/runtime"
mkdir -p "$fake_fram" "$runtime"

# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'printf "%s\n" "fram $*" >>"$TEST_CALLS"' \
  'if [[ "${4:-}" == fail ]]; then exit 9; fi' \
  'printf "%s\n" "committed via coordinator: ${2:-} ${3:-} = ${4:-}"' \
  >"$fake_fram/fram"
chmod +x "$fake_fram/fram"

# Exact UUID writes avoid the resolver. Handle tests resolve to thread_a.
# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'printf "%s\n" "bb $*" >>"$TEST_BB_CALLS"' \
  'printf "%s\n" "@019fa4d4-93aa-7447-aae5-0a5bcfca6849"' \
  >"$scratch/bb"
chmod +x "$scratch/bb"

thread_a=019fa4d4-93aa-7447-aae5-0a5bcfca6849
thread_b=019fa4d4-93aa-7447-aae5-0a5bcfca6850
common_env=(
  HOME="$scratch/home"
  XDG_RUNTIME_DIR="$runtime"
  FRAM_BIN="$fake_fram"
  FRAM_LOG="$scratch/facts.log"
  NORTH_BB="$scratch/bb"
  NORTH_TELEMETRY_PARTITION=0
  NORTH_VERB_SLOTS=0
  TEST_CALLS="$calls"
  TEST_BB_CALLS="$bb_calls"
)

expect_denied() {
  local label="$1"
  shift
  if "$@" >"$scratch/denied.out" 2>&1; then
    printf 'agent write controls: expected denial: %s\n' "$label" >&2
    exit 1
  fi
  grep -Fq 'REFUSED' "$scratch/denied.out"
}

for topology in worker orchestrator; do
  agent_env=("${common_env[@]}" AGENT_TOPOLOGY="$topology" AGENT_ID="$topology-agent")
  for predicate in started checkpoint blocked landed handoff; do
    env "${agent_env[@]}" "$root/bin/north" tell \
      "$thread_a" "$predicate" "$topology $predicate" >/dev/null
    if [ "$predicate" = checkpoint ]; then
      find "$runtime/north-agent-checkpoints" -type f ! -name '*.lock' \
        -exec touch -d '11 minutes ago' {} +
    fi
  done
  expect_denied "$topology arbitrary tell" \
    env "${agent_env[@]}" "$root/bin/north" tell "$thread_a" progress report
  expect_denied "$topology retract" \
    env "${agent_env[@]}" "$root/bin/north" retract "$thread_a" checkpoint report
  expect_denied "$topology capture" \
    env "${agent_env[@]}" "$root/bin/north" capture "agent-created thread"
done

env -u AGENT_TOPOLOGY -u AGENT_ID "${common_env[@]}" \
  "$root/bin/north" tell "$thread_a" progress "interactive report" >/dev/null
env -u AGENT_TOPOLOGY -u AGENT_ID "${common_env[@]}" \
  "$root/bin/north" capture "interactive thread" >/dev/null

rm -rf "${runtime:?}/north-agent-checkpoints"
: >"$calls"
rate_env=("${common_env[@]}" AGENT_TOPOLOGY=worker AGENT_ID=rate-agent)
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint first >/dev/null
expect_denied "second checkpoint inside 600 seconds" \
  env "${rate_env[@]}" "$root/bin/north" tell "$thread_a" checkpoint second
grep -Fq 'rate-limited to one per thread every 600s' "$scratch/denied.out"
[[ "$(wc -l <"$calls")" -eq 1 ]]

env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_b" checkpoint "different thread" >/dev/null
env "${common_env[@]}" AGENT_TOPOLOGY=worker AGENT_ID=other-agent \
  "$root/bin/north" tell "$thread_a" checkpoint "different agent" >/dev/null

find "$runtime/north-agent-checkpoints" -type f ! -name '*.lock' \
  -exec touch -d '11 minutes ago' {} +
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint "after interval" >/dev/null

rm -rf "${runtime:?}/north-agent-checkpoints"
if env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint fail >"$scratch/fail.out" 2>&1; then
  echo "agent write controls: fake failed checkpoint unexpectedly succeeded" >&2
  exit 1
fi
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint "retry after failure" >/dev/null

rm -rf "${runtime:?}/north-agent-checkpoints"
set +e
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint concurrent-a >"$scratch/concurrent-a.out" 2>&1 &
pid_a=$!
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint concurrent-b >"$scratch/concurrent-b.out" 2>&1 &
pid_b=$!
wait "$pid_a"
status_a=$?
wait "$pid_b"
status_b=$?
set -e
if ! { { [ "$status_a" -eq 0 ] && [ "$status_b" -ne 0 ]; } ||
       { [ "$status_b" -eq 0 ] && [ "$status_a" -ne 0 ]; }; }; then
  printf 'agent write controls: concurrent checkpoints were not serialized: %s %s\n' \
    "$status_a" "$status_b" >&2
  exit 1
fi
grep -Fq 'rate-limited to one per thread every 600s' \
  "$scratch/concurrent-a.out" "$scratch/concurrent-b.out"

rm -rf "${runtime:?}/north-agent-checkpoints"
env "${rate_env[@]}" "$root/bin/north" tell \
  @alias checkpoint "resolved handle" >/dev/null
expect_denied "handle and UUID share a canonical checkpoint limit" \
  env "${rate_env[@]}" "$root/bin/north" tell "$thread_a" checkpoint duplicate

echo "agent-write-controls: PASS"
