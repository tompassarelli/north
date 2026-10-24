# shellcheck shell=bash
NORTH_PACE_BIN="${NORTH_PACE_BIN:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && realpath -m "${CARGO_TARGET_DIR:-target}")/debug/north}"
[ -x "$NORTH_PACE_BIN" ] || { echo "$(basename "$0"): build north from this tree first; $NORTH_PACE_BIN is missing" >&2; exit 1; }
export NORTH_PACE_BIN
