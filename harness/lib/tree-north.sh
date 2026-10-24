# shellcheck shell=bash
tree_north_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tree_north_build="cd $tree_north_root && nix develop -c cargo build --locked --offline --bin north"
tree_north_stale() {
  local dep deps
  [ -f "$1.d" ] || return 0
  read -ra deps <<<"$(sed -n '1s/^[^:]*: //p' "$1.d")"
  for dep in "${deps[@]}"; do [ "$dep" -nt "$1" ] && return 0; done
  return 1
}
if [ -z "${NORTH_PACE_BIN:-}" ]; then
  NORTH_PACE_BIN="$(cd "$tree_north_root" && realpath -m "${CARGO_TARGET_DIR:-target}")/debug/north"
  if [ -x "$NORTH_PACE_BIN" ] && tree_north_stale "$NORTH_PACE_BIN"; then
    echo "$(basename "$0"): $NORTH_PACE_BIN is older than a source it was built from; rebuild it: $tree_north_build" >&2; exit 1
  fi
fi
[ -x "$NORTH_PACE_BIN" ] || { echo "$(basename "$0"): $NORTH_PACE_BIN is missing; build north from this tree first: $tree_north_build" >&2; exit 1; }
export NORTH_PACE_BIN
