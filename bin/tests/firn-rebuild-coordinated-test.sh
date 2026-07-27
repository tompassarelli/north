#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
wrapper="$root/bin/firn-rebuild-coordinated"
scratch="$(mktemp -d -t firn-rebuild-coordinated-test.XXXXXX)"
trap 'rm -rf "${scratch:?}"' EXIT

calls="$scratch/calls"
north_fake="$scratch/north"
firn_fake="$scratch/firn"
runtime_state="$scratch/state/fram-runtime"
restart_lock="$scratch/state/.fram-runtime.restart.lock"
intent_id=01234567-89ab-cdef-8123-456789abcdef

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
  "coord-doctor")
    printf 'serving the canonical log\n'
    exit "${NORTH_DOCTOR_RC:-0}"
    ;;
  *)
    printf 'unexpected north call: %s\n' "$*" >&2
    exit 2
    ;;
esac
SH

cat >"$firn_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'firn %s\n' "$*" >>"${CALLS:?}"
exit "${FIRN_FAKE_RC:-0}"
SH
chmod +x "$north_fake" "$firn_fake"

run_wrapper() {
  env \
    CALLS="$calls" \
    INTENT_ID="$intent_id" \
    NORTH_BIN="$north_fake" \
    FIRN_BIN="$firn_fake" \
    NORTH_COORD_RUNTIME_STATE="$runtime_state" \
    NORTH_COORD_RESTART_LOCK_TIMEOUT="${NORTH_COORD_RESTART_LOCK_TIMEOUT:-1}" \
    NORTH_REBUILD_COORD_RETRY_ATTEMPTS="${NORTH_REBUILD_COORD_RETRY_ATTEMPTS:-1}" \
    "$wrapper" --why "test generation" --hold 1s --max-delay 2s -- --verbose
}

: >"$calls"
run_wrapper >"$scratch/success.out"
mapfile -t success_calls <"$calls"
[[ "${success_calls[0]}" == north\ rebuild-intent\ start* ]]
[[ "${success_calls[1]}" == "north rebuild-intent await $intent_id" ]]
[[ "${success_calls[2]}" == "north rebuild-intent mark-started $intent_id" ]]
[[ "${success_calls[3]}" == "firn rebuild --verbose" ]]
[[ "${success_calls[4]}" == "north coord-doctor" ]]
[[ "${success_calls[5]}" == \
  "north rebuild-intent deployment-verified $intent_id firn rebuild rc 0; north coord-doctor rc 0" ]]
grep -Fq 'deployment verified: firn rebuild rc 0; north coord-doctor rc 0' \
  "$scratch/success.out"

: >"$calls"
set +e
FIRN_FAKE_RC=7 run_wrapper >"$scratch/failure.out" 2>"$scratch/failure.err"
failure_rc=$?
set -e
[[ "$failure_rc" -eq 7 ]]
grep -Fxq "north rebuild-intent failed $intent_id firn rebuild rc 7" "$calls"
if grep -Fq 'north coord-doctor' "$calls"; then
  printf 'failed rebuild incorrectly ran deployment verification\n' >&2
  exit 1
fi

: >"$calls"
set +e
NORTH_DOCTOR_RC=9 run_wrapper \
  >"$scratch/doctor-failure.out" 2>"$scratch/doctor-failure.err"
doctor_failure_rc=$?
set -e
[[ "$doctor_failure_rc" -eq 9 ]]
grep -Fxq \
  "north rebuild-intent failed $intent_id firn rebuild rc 0; north coord-doctor rc 9 after 1 attempts" \
  "$calls"
if grep -Fq 'deployment-verified' "$calls"; then
  printf 'failed deployment probe incorrectly reported verification\n' >&2
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

echo "PASS coordinated wrapper orders intent/hold/all-clear/rebuild/verification, reports failure, and shares the runtime restart mutex"
