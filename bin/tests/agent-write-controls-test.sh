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

for topology in worker orchestrator native; do
  agent_env=("${common_env[@]}" AGENT_ID="$topology-agent")
  [ "$topology" = native ] || agent_env+=(AGENT_TOPOLOGY="$topology")
  for predicate in started checkpoint blocked landed handoff; do
    env "${agent_env[@]}" "$root/bin/north" tell \
      "$thread_a" "$predicate" "$topology $predicate" >/dev/null
    find "$runtime/north-agent-reports" -type f ! -name '*.lock' \
      -exec touch -d '11 minutes ago' {} +
  done
  expect_denied "$topology arbitrary tell" \
    env "${agent_env[@]}" "$root/bin/north" tell "$thread_a" progress report
  expect_denied "$topology retract" \
    env "${agent_env[@]}" "$root/bin/north" retract "$thread_a" checkpoint report
  expect_denied "$topology capture" \
    env "${agent_env[@]}" "$root/bin/north" capture "agent-created thread"
  expect_denied "$topology merge" \
    env "${agent_env[@]}" "$root/bin/north" merge "$thread_a" "$thread_b"
  expect_denied "$topology import" \
    env "${agent_env[@]}" "$root/bin/north" import
done

expect_denied "native/no-topology arbitrary tell" \
  env -u AGENT_TOPOLOGY -u AGENT_ID -u NORTH_AGENT_ID "${common_env[@]}" \
  "$root/bin/north" tell "$thread_a" progress "native report"
expect_denied "native/no-topology capture" \
  env -u AGENT_TOPOLOGY -u AGENT_ID -u NORTH_AGENT_ID "${common_env[@]}" \
  "$root/bin/north" capture "native-created thread"

forged_token="$(printf 'a%.0s' {1..64})"
expect_denied "legacy trusted boolean cannot bypass authority" \
  env -u AGENT_TOPOLOGY -u AGENT_ID "${common_env[@]}" \
  NORTH_TRUSTED_HARNESS_WRITE=1 \
  "$root/bin/north" tell "$thread_a" progress forged
expect_denied "token without its private file cannot bypass authority" \
  env -u AGENT_TOPOLOGY -u AGENT_ID "${common_env[@]}" \
  NORTH_THREAD_WRITE_CAPABILITY="$forged_token" \
  "$root/bin/north" tell "$thread_a" progress forged

env -u AGENT_TOPOLOGY -u AGENT_ID "${common_env[@]}" \
  "$root/bin/north-author" tell "$thread_a" progress "trusted report" >/dev/null
env -u AGENT_TOPOLOGY -u AGENT_ID "${common_env[@]}" \
  "$root/bin/north-author" capture "trusted thread" >/dev/null
if find "$runtime/north-thread-write-authority" -type f -print -quit |
   grep -q .; then
  echo "agent write controls: north-author left a capability file behind" >&2
  exit 1
fi

rm -rf "${runtime:?}/north-agent-reports"
: >"$calls"
rate_env=("${common_env[@]}" AGENT_TOPOLOGY=worker AGENT_ID=rate-agent)

for predicate in started checkpoint blocked landed handoff; do
  rm -rf "${runtime:?}/north-agent-reports"
  : >"$calls"
  env "${rate_env[@]}" "$root/bin/north" tell \
    "$thread_a" "$predicate" first >/dev/null
  expect_denied "second $predicate report inside 600 seconds" \
    env "${rate_env[@]}" "$root/bin/north" tell "$thread_a" "$predicate" second
  grep -Fq 'rate-limited to one per agent/thread every 600s' "$scratch/denied.out"
  [[ "$(wc -l <"$calls")" -eq 1 ]]
done

rm -rf "${runtime:?}/north-agent-reports"
: >"$calls"
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" started first >/dev/null
expect_denied "different report predicate shares the agent/thread limit" \
  env "${rate_env[@]}" "$root/bin/north" tell "$thread_a" checkpoint second
grep -Fq 'rate-limited to one per agent/thread every 600s' "$scratch/denied.out"
[[ "$(wc -l <"$calls")" -eq 1 ]]

env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_b" checkpoint "different thread" >/dev/null
env "${common_env[@]}" AGENT_TOPOLOGY=worker AGENT_ID=other-agent \
  "$root/bin/north" tell "$thread_a" checkpoint "different agent" >/dev/null

find "$runtime/north-agent-reports" -type f ! -name '*.lock' \
  -exec touch -d '11 minutes ago' {} +
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint "after interval" >/dev/null

rm -rf "${runtime:?}/north-agent-reports"
if env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint fail >"$scratch/fail.out" 2>&1; then
  echo "agent write controls: fake failed checkpoint unexpectedly succeeded" >&2
  exit 1
fi
env "${rate_env[@]}" "$root/bin/north" tell \
  "$thread_a" checkpoint "retry after failure" >/dev/null

rm -rf "${runtime:?}/north-agent-reports"
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
grep -Fq 'rate-limited to one per agent/thread every 600s' \
  "$scratch/concurrent-a.out" "$scratch/concurrent-b.out"

rm -rf "${runtime:?}/north-agent-reports"
env "${rate_env[@]}" "$root/bin/north" tell \
  @alias checkpoint "resolved handle" >/dev/null
expect_denied "handle and UUID share a canonical checkpoint limit" \
  env "${rate_env[@]}" "$root/bin/north" tell "$thread_a" checkpoint duplicate

echo "agent-write-controls: PASS"
