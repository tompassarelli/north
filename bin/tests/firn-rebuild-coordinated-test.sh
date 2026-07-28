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
[[ "$*" == "coord-ready" ]]
printf 'coordinator runtime identity ready\n'
exit "${NORTH_READY_RC:-0}"
SH

cat >"$firn_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'firn %s\n' "$*" >>"${CALLS:?}"
exit "${FIRN_FAKE_RC:-0}"
SH
chmod +x "$north_fake" "$post_north_fake" "$firn_fake"

run_wrapper() {
  env \
    CALLS="$calls" \
    INTENT_ID="$intent_id" \
    NORTH_BIN="$north_fake" \
    NORTH_POST_REBUILD_BIN="$post_north_fake" \
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
[[ "${success_calls[4]}" == "north-current coord-ready" ]]
[[ "${success_calls[5]}" == \
  "north rebuild-intent deployment-verified $intent_id firn rebuild rc 0; north coord-ready rc 0" ]]
grep -Fq 'deployment verified: firn rebuild rc 0; north coord-ready rc 0' \
  "$scratch/success.out"
if grep -Fq 'coord-doctor' "$calls"; then
  printf 'post-rebuild gate invoked the full coordinator doctor\n' >&2
  exit 1
fi

: >"$calls"
set +e
FIRN_FAKE_RC=7 run_wrapper >"$scratch/failure.out" 2>"$scratch/failure.err"
failure_rc=$?
set -e
[[ "$failure_rc" -eq 7 ]]
grep -Fxq "north rebuild-intent failed $intent_id firn rebuild rc 7" "$calls"
if grep -Eq 'coord-ready|coord-doctor' "$calls"; then
  printf 'failed rebuild incorrectly ran deployment verification\n' >&2
  exit 1
fi

: >"$calls"
set +e
NORTH_READY_RC=9 run_wrapper \
  >"$scratch/readiness-failure.out" 2>"$scratch/readiness-failure.err"
readiness_failure_rc=$?
set -e
[[ "$readiness_failure_rc" -eq 9 ]]
grep -Fxq \
  "north rebuild-intent failed $intent_id firn rebuild rc 0; north coord-ready rc 9 after 1 attempts" \
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

# Exercise the public cheap-readiness primitive itself. Run a copied North
# wrapper against a fake north-coord-up so the test proves exact per-writer
# environment selection without needing either daemon or a corpus.
north_cli_root="$scratch/north-cli-root"
north_cli="$north_cli_root/bin/north"
coord_up_fake="$north_cli_root/bin/north-coord-up"
bb_fail="$scratch/bb-must-not-run"
ready_calls="$scratch/ready-calls"
bb_calls="$scratch/bb-calls"
coord_log="$scratch/coordination.log"
telemetry_log="$scratch/telemetry.log"
coord_state="$scratch/state/fram-runtime"
telemetry_state="$scratch/state/fram-telemetry-runtime"
mkdir -p "$north_cli_root/bin"
cp "$root/bin/north" "$north_cli"

cat >"$coord_up_fake" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
port="${FRAM_PORT:-7977}"
printf 'port=%s log=%s state=%s unit=%s partition=%s telemetry-log=%s\n' \
  "$port" \
  "${FRAM_LOG:?}" \
  "${NORTH_COORD_RUNTIME_STATE:-${HOME:?}/.local/state/north/fram-runtime}" \
  "${NORTH_COORD_SYSTEMD_UNIT:-north-coord.service}" \
  "${NORTH_TELEMETRY_PARTITION:-0}" \
  "${FRAM_TELEMETRY_LOG:-unset}" \
  >>"${READY_CALLS:?}"
[[ "$*" == "--check-runtime" ]]
[[ "${READY_FAIL_PORT:-}" != "$port" ]] || exit 19
printf 'coordinator runtime identity OK on :%s\n' "$port"
SH

cat >"$bb_fail" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'bb %s\n' "$*" >>"${BB_CALLS:?}"
exit 99
SH
chmod +x "$north_cli" "$coord_up_fake" "$bb_fail"

run_ready() {
  env \
    HOME="$scratch/home" \
    NORTH_BB="$bb_fail" \
    BB_CALLS="$bb_calls" \
    READY_CALLS="$ready_calls" \
    FRAM_LOG="$coord_log" \
    NORTH_COORD_RUNTIME_STATE="$coord_state" \
    NORTH_TELEMETRY_RUNTIME_STATE="$telemetry_state" \
    NORTH_TELEMETRY_PARTITION="${NORTH_TELEMETRY_PARTITION:-0}" \
    NORTH_TELEMETRY_PORT="${NORTH_TELEMETRY_PORT:-7978}" \
    FRAM_TELEMETRY_LOG="${FRAM_TELEMETRY_LOG:-$telemetry_log}" \
    "$north_cli" coord-ready
}

: >"$ready_calls"
: >"$bb_calls"
NORTH_TELEMETRY_PARTITION=0 run_ready >"$scratch/coord-ready.out"
[[ "$(wc -l <"$ready_calls")" -eq 1 ]]
grep -Fxq \
  "port=7977 log=$coord_log state=$coord_state unit=north-coord.service partition=0 telemetry-log=$telemetry_log" \
  "$ready_calls"

: >"$ready_calls"
NORTH_TELEMETRY_PARTITION=1 run_ready >"$scratch/stage-a-ready.out"
mapfile -t stage_a_calls <"$ready_calls"
[[ "${#stage_a_calls[@]}" -eq 2 ]]
[[ "${stage_a_calls[0]}" == \
  "port=7977 log=$coord_log state=$coord_state unit=north-coord.service partition=1 telemetry-log=$telemetry_log" ]]
[[ "${stage_a_calls[1]}" == \
  "port=7978 log=$telemetry_log state=$telemetry_state unit=north-telemetry-coord.service partition=0 telemetry-log=unset" ]]

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
[[ "$(wc -l <"$ready_calls")" -eq 2 ]]

if [[ -s "$bb_calls" ]]; then
  printf 'coord-ready invoked north.main/full doctor: %s\n' "$(<"$bb_calls")" >&2
  exit 1
fi

echo "PASS coordinated wrapper orders intent/hold/all-clear/rebuild/cheap-readiness, reports failure, and shares the runtime restart mutex"
