#!/usr/bin/env bash
set -euo pipefail
for tool in cargo jq; do command -v "$tool" >/dev/null || { printf "cargo-tests: required tool %s not found on PATH\n" "$tool" >&2; exit 127; }; done
target=${CARGO_TARGET_DIR:-target}
built=$(cargo test --locked --offline --no-run --message-format=json-render-diagnostics | jq -r 'select(.reason == "compiler-artifact" and .profile.test) | .executable // empty')
mapfile -t tests <<<"$built"
"$target/debug/north" land-tests "${tests[@]}"
