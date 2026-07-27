#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "-e" ]]; then
  printf '%s\t%s\t%s\t%s\n' \
    "${NORTH_NATIVE_REPO:-}" \
    "${NORTH_NATIVE_ROLE:-}" \
    "${NORTH_NATIVE_ROLE_ALIAS:-}" \
    "${NORTH_NATIVE_SUBJECT:-}" \
    >"${ROLE_ALIAS_TEST_LOG:?}"
fi
