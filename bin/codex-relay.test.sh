#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
state="$(mktemp -d)"
trap 'rm -rf "$state"' EXIT
if echo "" | CODEX_BEHAVIOR_STATE="$state" "$here/codex-relay" s1 999999 2>/dev/null; then
  echo "FAIL empty message accepted"
  exit 1
fi
echo "codex-relay: empty input refused"
