#!/usr/bin/env bash
set -euo pipefail
exec ./bin/head-north --code src/cli/,crates/north-core/src/ops/citation_check/ citation-check --root "$PWD"
