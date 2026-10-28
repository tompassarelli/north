#!/usr/bin/env bash
set -euo pipefail
target=${CARGO_TARGET_DIR:-target}
built=$(cargo test --locked --offline --no-run --message-format=json-render-diagnostics | jq -r 'select(.reason == "compiler-artifact" and .profile.test) | .executable // empty')
mapfile -t tests <<<"$built"
"$target/debug/north" land-tests "${tests[@]}"
