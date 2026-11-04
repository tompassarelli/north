#!/usr/bin/env bash
set -euo pipefail
roots=(--north-root "$PWD" --south-root "$HOME/code/south/main" --nixos-root "$HOME/code/nixos-config/main") # hardcoded-repo-path:allow
runtime=$(readlink -f "$HOME/.local/state/north/current" 2>/dev/null || true)
sha=${runtime##*/}
if [ -x "$runtime/north" ] && git diff --quiet "$sha" "${SAFE_PUSH_LAND_HEAD:-HEAD}" -- crates/north-core/src/catalog crates/north-core/src/agent_catalog.rs crates/north-base Cargo.lock 2>/dev/null; then
  exec "$runtime/north" config agents check-owners "${roots[@]}"
fi
exec nix develop -c cargo run --offline --bin north -- config agents check-owners "${roots[@]}"
