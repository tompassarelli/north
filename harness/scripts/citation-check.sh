#!/usr/bin/env bash
set -euo pipefail
runtime=$(readlink -f "$HOME/.local/state/north/current" 2>/dev/null || true)
sha=${runtime##*/}
if [ -x "$runtime/north" ] && git diff --quiet "$sha" "${SAFE_PUSH_LAND_HEAD:-HEAD}" -- src/cli crates/north-core/src/ops/citation_check Cargo.lock 2>/dev/null; then
  exec "$runtime/north" citation-check --root "$PWD"
fi
exec nix develop -c cargo run --offline --bin north -- citation-check --root "$PWD"
