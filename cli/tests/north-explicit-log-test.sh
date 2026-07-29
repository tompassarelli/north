#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

CUSTOM="$TMP/custom.log"
STATE="$TMP/home/.local/state/north"
SPLIT="$STATE/coordination.log"
TELEMETRY="$STATE/telemetry.log"
mkdir -p "$STATE"
: >"$CUSTOM"
: >"$SPLIT"
: >"$TELEMETRY"

TRACE="$(
  env -u FRAM_TELEMETRY_LOG \
    HOME="$TMP/home" \
    FRAM_LOG="$CUSTOM" \
    bash -x "$ROOT/bin/north" help 2>&1
)"

if ! grep -Fq "+ export FRAM_LOG=$CUSTOM" <<<"$TRACE"; then
  echo "FAIL: public bin/north did not preserve the explicit FRAM_LOG" >&2
  exit 1
fi
if grep -Fq "+ export FRAM_LOG=$SPLIT" <<<"$TRACE"; then
  echo "FAIL: public bin/north redirected an explicit FRAM_LOG to coordination.log" >&2
  exit 1
fi

STAGE_A_TRACE="$(
  env -u FRAM_LOG \
    HOME="$TMP/home" \
    FRAM_TELEMETRY_LOG="$TELEMETRY" \
    NORTH_TELEMETRY_PARTITION=1 \
    NORTH_TELEMETRY_PORT=7978 \
    bash -x "$ROOT/bin/north" help 2>&1
)"

if ! grep -Fq "+ export FRAM_LOG=$SPLIT" <<<"$STAGE_A_TRACE"; then
  echo "FAIL: Stage-A public bin/north did not select the existing coordination.log" >&2
  exit 1
fi
if grep -Fq "+ export FRAM_TELEMETRY_LOG=$STATE/telemetry.log" <<<"$STAGE_A_TRACE"; then
  echo "FAIL: Stage-A public bin/north overwrote an explicit FRAM_TELEMETRY_LOG" >&2
  exit 1
fi

echo "north explicit log: PASS"
