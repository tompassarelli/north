#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
wrapper="$root/bin/firn-rebuild-coordinated"
scratch="$(mktemp -d -t firn-rebuild-coordinated-test.XXXXXX)"
trap 'rm -rf "${scratch:?}"' EXIT

calls="$scratch/calls"
north_fake="$scratch/north"
post_north_fake="$scratch/north-current"
firn_fake="$scratch/firn"
old_generation="$scratch/system-old"
new_generation="$scratch/system-new"
system_profile="$scratch/current-system"
runtime_state="$scratch/state/fram-runtime"
restart_lock="$scratch/state/.fram-runtime.restart.lock"
harness_conf="$scratch/harness.conf"
off_harness_conf="$scratch/harness-off.conf"
intent_id=01234567-89ab-cdef-8123-456789abcdef
printf 'rebuild-coordination=on\n' >"$harness_conf"
printf 'rebuild-coordination=off\n' >"$off_harness_conf"

cat >"$north_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'north %s\n' "$*" >>"${CALLS:?}"
case "$*" in
  "rebuild-intent start "*)
    printf '%s\n' "${INTENT_ID:?}"
    ;;
  "rebuild-intent await "*)
    printf 'all-clear %s\n' "${INTENT_ID:?}"
    ;;
  "rebuild-intent mark-started "*)
    printf 'rebuild-started %s\n' "${INTENT_ID:?}"
    ;;
  "rebuild-intent deployment-verified "*)
    printf 'deployment-verified %s\n' "${INTENT_ID:?}"
    ;;
  "rebuild-intent failed "*)
    printf 'failed %s\n' "${INTENT_ID:?}"
    ;;
  *)
    printf 'unexpected north call: %s\n' "$*" >&2
    exit 2
    ;;
esac
SH

cat >"$post_north_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'north-current %s\n' "$*" >>"${CALLS:?}"
case "$*" in
  --help)
    printf 'north coord-safety\n'
    ;;
  coord-safety)
    printf 'source checkout is not authorable\n' >&2
    exit "${NORTH_SAFETY_RC:-9}"
    ;;
  coord-ready)
    printf 'coordinator runtime identity ready\n'
    exit "${NORTH_READY_RC:-0}"
    ;;
  *)
    printf 'unexpected current-generation north call: %s\n' "$*" >&2
    exit 2
    ;;
esac
SH

cat >"$firn_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'firn %s\n' "$*" >>"${CALLS:?}"
rc="${FIRN_FAKE_RC:-0}"
if [[ "$rc" -eq 0 ]]; then
  ln -sfn "${NEW_SYSTEM_PROFILE:?}" "${CURRENT_SYSTEM_PROFILE:?}"
fi
exit "$rc"
SH
chmod +x "$north_fake" "$post_north_fake" "$firn_fake"
mkdir -p "$old_generation/bin" "$new_generation/bin"
ln -s "$north_fake" "$old_generation/bin/north"
ln -s "$post_north_fake" "$new_generation/bin/north"

run_wrapper() {
  ln -sfn "$old_generation" "$system_profile"
  env \
    CALLS="$calls" \
    INTENT_ID="$intent_id" \
    NORTH_BIN="$north_fake" \
    NORTH_SYSTEM_PROFILE="$system_profile" \
    FIRN_BIN="$firn_fake" \
    CURRENT_SYSTEM_PROFILE="$system_profile" \
    NEW_SYSTEM_PROFILE="$new_generation" \
    NORTH_COORD_RUNTIME_STATE="$runtime_state" \
    NORTH_HARNESS_CONF="${NORTH_HARNESS_CONF:-$harness_conf}" \
    NORTH_COORD_RESTART_LOCK_TIMEOUT="${NORTH_COORD_RESTART_LOCK_TIMEOUT:-1}" \
    NORTH_REBUILD_COORD_RETRY_ATTEMPTS="${NORTH_REBUILD_COORD_RETRY_ATTEMPTS:-1}" \
    "$wrapper" "$@" --why "test generation" --hold 1s --max-delay 2s -- --verbose
}

: >"$calls"
# The wrapper that began the deployment is deliberately unable to attest the
# newly activated generation. A correct implementation must not call it for
# post-switch readiness.
set +e
CALLS="$calls" INTENT_ID="$intent_id" "$north_fake" coord-safety \
  >"$scratch/old-self-check.out" 2>"$scratch/old-self-check.err"
old_self_check_rc=$?
set -e
[[ "$old_self_check_rc" -eq 2 ]]
: >"$calls"
run_wrapper >"$scratch/success.out"
mapfile -t success_calls <"$calls"
[[ "${success_calls[0]}" == north\ rebuild-intent\ start* ]]
[[ "${success_calls[1]}" == "north rebuild-intent await $intent_id" ]]
[[ "${success_calls[2]}" == "north rebuild-intent mark-started $intent_id" ]]
[[ "${success_calls[3]}" == "firn rebuild --verbose" ]]
[[ "${success_calls[4]}" == "north-current coord-ready" ]]
[[ "${success_calls[5]}" == \
  "north rebuild-intent deployment-verified $intent_id firn rebuild rc 0; north coord-ready rc 0 (live runtime identity healthy)" ]]
grep -Fq 'deployment verified: firn rebuild rc 0; north coord-ready rc 0 (live runtime identity healthy)' \
  "$scratch/success.out"
if grep -Eq '^north (coord-safety|coord-ready)$' "$calls"; then
  printf 'post-rebuild gate self-verified through the old wrapper\n' >&2
  exit 1
fi

: >"$calls"
NORTH_HARNESS_CONF="$off_harness_conf" run_wrapper --automatic \
  >"$scratch/automatic-success.out"
mapfile -t automatic_success_calls <"$calls"
[[ "${automatic_success_calls[0]}" == "firn rebuild --verbose" ]]
[[ "${automatic_success_calls[1]}" == "north-current coord-ready" ]]
[[ "${#automatic_success_calls[@]}" -eq 2 ]]
grep -Fq \
  'deployment verified: firn rebuild rc 0; north coord-ready rc 0 (live runtime identity healthy)' \
  "$scratch/automatic-success.out"

: >"$calls"
set +e
FIRN_FAKE_RC=7 run_wrapper --automatic \
  >"$scratch/automatic-failure.out" 2>"$scratch/automatic-failure.err"
automatic_failure_rc=$?
set -e
[[ "$automatic_failure_rc" -eq 7 ]]
grep -Fxq "firn rebuild --verbose" "$calls"
[[ "$(wc -l <"$calls")" -eq 1 ]]
grep -Fq 'firn rebuild rc 7' "$scratch/automatic-failure.err"

: >"$calls"
set +e
FIRN_FAKE_RC=7 run_wrapper >"$scratch/failure.out" 2>"$scratch/failure.err"
failure_rc=$?
set -e
[[ "$failure_rc" -eq 7 ]]
grep -Fxq "north rebuild-intent failed $intent_id firn rebuild rc 7" "$calls"
if grep -Eq 'north-current (coord-safety|coord-ready)|^north (coord-safety|coord-ready)$' "$calls"; then
  printf 'failed rebuild incorrectly ran deployment verification\n' >&2
  exit 1
fi

: >"$calls"
set +e
NORTH_READY_RC=9 run_wrapper \
  >"$scratch/readiness-failure.out" 2>"$scratch/readiness-failure.err"
readiness_failure_rc=$?
set -e
[[ "$readiness_failure_rc" -eq 0 ]]
grep -Fxq \
  "north rebuild-intent deployment-verified $intent_id firn rebuild rc 0; coordination readiness degraded: north coord-ready rc 9 after 1 attempts" \
  "$calls"
grep -Fq \
  'deployment succeeded; coordination readiness degraded: north coord-ready rc 9 after 1 attempts' \
  "$scratch/readiness-failure.err"
if grep -Fq 'rebuild-intent failed' "$calls"; then
  printf 'degraded deployment readiness incorrectly overwrote rebuild success\n' >&2
  exit 1
fi

: >"$calls"
chmod -x "$post_north_fake"
run_wrapper >"$scratch/missing-post-cli.out" 2>"$scratch/missing-post-cli.err"
chmod +x "$post_north_fake"
grep -Fxq \
  "north rebuild-intent deployment-verified $intent_id firn rebuild rc 0; coordination readiness degraded: post-rebuild North CLI is unavailable: $system_profile/bin/north" \
  "$calls"
grep -Fq \
  'deployment succeeded; coordination readiness degraded: post-rebuild North CLI is unavailable:' \
  "$scratch/missing-post-cli.err"
if grep -Fq 'rebuild-intent failed' "$calls"; then
  printf 'missing readiness probe incorrectly overwrote rebuild success\n' >&2
  exit 1
fi

: >"$calls"
mkdir -p "${restart_lock%/*}"
exec 8>"$restart_lock"
flock 8
set +e
NORTH_COORD_RESTART_LOCK_TIMEOUT=0 run_wrapper \
  >"$scratch/mutex.out" 2>"$scratch/mutex.err"
mutex_rc=$?
set -e
flock -u 8
[[ "$mutex_rc" -eq 2 ]]
grep -Fq 'restart-mutex-busy:' "$scratch/mutex.err"
if grep -Eq 'mark-started|^firn ' "$calls"; then
  printf 'mutex-busy rebuild crossed the restart boundary\n' >&2
  exit 1
fi

: >"$calls"
flock 8
set +e
NORTH_COORD_RESTART_LOCK_TIMEOUT=0 run_wrapper --automatic \
  >"$scratch/automatic-mutex.out" 2>"$scratch/automatic-mutex.err"
automatic_mutex_rc=$?
set -e
flock -u 8
[[ "$automatic_mutex_rc" -eq 2 ]]
grep -Fq 'restart-mutex-busy:' "$scratch/automatic-mutex.err"
[[ ! -s "$calls" ]]

# Exercise the public cheap-readiness primitive itself. Run a copied North
# wrapper against a fake Babashka boundary so the test proves the exact
# per-SpaceId FRAMRPC status and runtime-attestation calls without a live server.
north_cli_root="$scratch/north-cli-root"
north_cli="$north_cli_root/bin/north"
bb_fake="$scratch/bb"
ready_calls="$scratch/ready-calls"
coord_log="$scratch/coordination.log"
telemetry_log="$scratch/telemetry.log"
state_home="$scratch/state"
mkdir -p "$north_cli_root/bin"
cp "$root/bin/north" "$north_cli"

cat >"$bb_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'bb'
  printf ' %s' "$@"
  printf '\n'
} >>"${READY_CALLS:?}"
for argument in "$@"; do
  [[ -z "${READY_FAIL_PORT:-}" || "$argument" != "$READY_FAIL_PORT" ]] || exit 19
done
case "${1##*/}" in
  coord.clj|runtime-attestation.clj) ;;
  *) printf 'unexpected bb entrypoint: %s\n' "$1" >&2; exit 2 ;;
esac
SH
chmod +x "$north_cli" "$bb_fake"

run_ready() {
  env \
    HOME="$scratch/home" \
    XDG_STATE_HOME="$state_home" \
    NORTH_BB="$bb_fake" \
    READY_CALLS="$ready_calls" \
    FRAM_LOG="$coord_log" \
    NORTH_TELEMETRY_PARTITION="${NORTH_TELEMETRY_PARTITION:-0}" \
    NORTH_TELEMETRY_PORT="${NORTH_TELEMETRY_PORT:-7978}" \
    FRAM_TELEMETRY_LOG="$telemetry_log" \
    "$north_cli" coord-ready
}

: >"$ready_calls"
NORTH_TELEMETRY_PARTITION=0 run_ready >"$scratch/coord-ready.out"
mapfile -t coordination_calls <"$ready_calls"
[[ "${#coordination_calls[@]}" -eq 2 ]]
[[ "${coordination_calls[0]}" == \
  "bb $north_cli_root/cli/coord.clj 7977 coordination" ]]
[[ "${coordination_calls[1]}" == \
  "bb $north_cli_root/cli/runtime-attestation.clj 7977 $coord_log north-coordination $state_home/north/framrpc-runtime/north-fram.runtime north-fram.service" ]]

: >"$ready_calls"
NORTH_TELEMETRY_PARTITION=1 run_ready >"$scratch/stage-a-ready.out"
mapfile -t stage_a_calls <"$ready_calls"
[[ "${#stage_a_calls[@]}" -eq 4 ]]
[[ "${stage_a_calls[0]}" == "${coordination_calls[0]}" ]]
[[ "${stage_a_calls[1]}" == "${coordination_calls[1]}" ]]
[[ "${stage_a_calls[2]}" == \
  "bb $north_cli_root/cli/coord.clj 7978 telemetry" ]]
[[ "${stage_a_calls[3]}" == \
  "bb $north_cli_root/cli/runtime-attestation.clj 7978 $telemetry_log north-telemetry $state_home/north/framrpc-runtime/north-telemetry-coord.runtime north-telemetry-coord.service" ]]

: >"$ready_calls"
set +e
READY_FAIL_PORT=7977 NORTH_TELEMETRY_PARTITION=1 run_ready \
  >"$scratch/coord-not-ready.out" 2>"$scratch/coord-not-ready.err"
coord_not_ready_rc=$?
set -e
[[ "$coord_not_ready_rc" -eq 19 ]]
[[ "$(wc -l <"$ready_calls")" -eq 1 ]]

: >"$ready_calls"
set +e
READY_FAIL_PORT=7978 NORTH_TELEMETRY_PARTITION=1 run_ready \
  >"$scratch/telemetry-not-ready.out" 2>"$scratch/telemetry-not-ready.err"
telemetry_not_ready_rc=$?
set -e
[[ "$telemetry_not_ready_rc" -eq 19 ]]
[[ "$(wc -l <"$ready_calls")" -eq 3 ]]

echo "PASS coordinated wrapper preserves human ceremony and gives automatic windows mutexed rebuild/readiness without coordinator intent traffic"
