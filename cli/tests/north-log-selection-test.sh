#!/usr/bin/env bash
# bin/north must talk to the corpus the coordinator actually serves.
#
# THE BUG THIS PINS (2026-07-29). The log-selection branch was gated on BOTH
# "the caller did not pin FRAM_LOG" AND "FRAM_TELEMETRY_LOG is unset". The
# packaged wrapper exports FRAM_TELEMETRY_LOG, so the branch never ran, FRAM_LOG
# stayed at facts.log, and the coordinator serves coordination.log. Every single
# `north tell` then died with
#   REFUSED — subject resolver unavailable (coordinator unavailable or incompatible)
# which reads as a dead daemon and is actually a log-identity mismatch. The
# identical write through `fram` with FRAM_LOG=coordination.log committed fine.
#
# The two variables answer different questions — FRAM_LOG is "which corpus",
# FRAM_TELEMETRY_LOG is "where telemetry goes" — and letting the second veto the
# first is what broke writes.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
NORTH_BIN="$HERE/../../bin/north"
pass=0
fail=0

check() { # check <label> <expected> <actual>
  if [ "$2" = "$3" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    printf 'FAIL %s\n  expected: %s\n  actual:   %s\n' "$1" "$2" "$3" >&2
  fi
}

# A fake HOME whose state dir holds BOTH logs, so the branch has something to
# select. The wrapper derives from $HOME/.local/state/north, so the fixture has
# to mirror that layout exactly.
fixture="$(mktemp -d)"
trap 'rm -rf "${fixture:?}"' EXIT
state="$fixture/.local/state/north"
mkdir -p "$state"
: >"$state/facts.log"
: >"$state/coordination.log"
: >"$state/telemetry.log"

# Report the resolved env without executing any north verb: source the wrapper
# with a sentinel arg it does not handle, after neutering the exec at the end.
resolved() { # resolved <env assignments...> -> "FRAM_LOG|FRAM_TELEMETRY_LOG"
  env -u FRAM_LOG -u FRAM_TELEMETRY_LOG "$@" bash -c '
    # Stop before dispatch: only the env-resolution prologue matters here.
    eval "$(sed -n "1,/^# TRANSITIONAL_SCHEMA_BOOTSTRAP/p" "$1" | grep -v "^#")"
    printf "%s|%s\n" "$FRAM_LOG" "$FRAM_TELEMETRY_LOG"
  ' _ "$NORTH_BIN" 2>/dev/null
}

# 1. THE REGRESSION: telemetry log preset must NOT veto coordination selection.
got="$(resolved "HOME=$fixture" "FRAM_TELEMETRY_LOG=$state/telemetry.log")"
check "preset FRAM_TELEMETRY_LOG still selects coordination.log" \
  "$state/coordination.log|$state/telemetry.log" "$got"

# 2. Neither preset: derive both.
got="$(resolved "HOME=$fixture")"
check "neither preset derives coordination + telemetry" \
  "$state/coordination.log|$state/telemetry.log" "$got"

# 3. An explicitly pinned FRAM_LOG is still honoured — the caller wins.
got="$(resolved "HOME=$fixture" "FRAM_LOG=$state/facts.log")"
check "explicit FRAM_LOG is never overridden" \
  "$state/facts.log|" "$got"

# 4. An explicit telemetry route is preserved, not clobbered by the derivation.
got="$(resolved "HOME=$fixture" "FRAM_TELEMETRY_LOG=$state/custom-telemetry.log")"
check "explicit FRAM_TELEMETRY_LOG survives coordination selection" \
  "$state/coordination.log|$state/custom-telemetry.log" "$got"

printf 'north-log-selection: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
