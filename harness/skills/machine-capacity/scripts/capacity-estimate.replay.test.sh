#!/usr/bin/env bash
set -euo pipefail
bun "$(dirname "${BASH_SOURCE[0]}")/capacity-estimate.replay.test.ts"
