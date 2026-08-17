#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/bin"

printf '#!/bin/sh\npwd > %q\nenv | sort > %q\n' \
  "$scratch/pwd" "$scratch/env" > "$scratch/bin/codex"
chmod +x "$scratch/bin/codex"

output="$(PATH="$scratch/bin:$PATH" NORTH_PORT=7977 AGENT_ID=leak BEAGLE_STORE_LOG=bad \
  "$repo/bin/north" zero codex 2>&1)"

grep -F 'north zero: strict zero session (codex)' <<<"$output" >/dev/null
grep -F 'disabled: inherited environment (including North/agent variables)' <<<"$output" >/dev/null
grep -F 'home:' <<<"$output" >/dev/null
grep -F 'machine-wide surfaces not hidden:' <<<"$output" >/dev/null || \
  grep -F 'disabled: /etc/codex/requirements.toml and /etc/codex/hooks' <<<"$output" >/dev/null
test "$(dirname "$(cat "$scratch/pwd")")" != "$repo"
grep -E '^(NORTH|AGENT|STORE)_' "$scratch/env" && exit 1 || true
grep -E '^HOME=/tmp/north-zero\.' "$scratch/env" >/dev/null
grep -E '^CODEX_HOME=/tmp/north-zero\.' "$scratch/env" >/dev/null

printf 'north zero test: PASS\n'
