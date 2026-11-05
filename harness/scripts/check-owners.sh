#!/usr/bin/env bash
set -euo pipefail
exec ./bin/head-north --code crates/north-core/src/catalog/,crates/north-core/src/agent_catalog.rs,crates/north-base/ config agents check-owners --north-root "$PWD" --south-root "$HOME/code/south/main" --nixos-root "$HOME/code/nixos-config/main" # hardcoded-repo-path:allow
