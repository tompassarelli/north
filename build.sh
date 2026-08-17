#!/usr/bin/env bash
# Recompile North's Beagle (.bclj) sources to Clojure into out/.
#
# North consumes the Fram engine from Beagle's branch-core subtree. Its source
# root is declared explicitly so the type checker resolves fram.* fully, and
# branch-core/out is on the runtime classpath (see bin/north).
# You only need this to rebuild from the .bclj sources (requires Beagle).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/src"; OUT="$HERE/out"
BEAGLE="${BEAGLE_HOME:-$HOME/code/beagle/main}"
FRAM="${FRAM_HOME:-$BEAGLE/branch-core}"

mkdir -p "$OUT/north"
for m in projections validate staleness audit worker_policy main; do
  BEAGLE_EMIT_SRCLOC=0 direnv exec "$BEAGLE" "$BEAGLE/bin/beagle-build" \
    --module-root "north/src=$SRC" \
    --module-root "branch-core/src=$FRAM/src" \
    "$SRC/north/$m.bclj" "$OUT/north/$m.clj" >/dev/null
  echo "  built north/$m"
done
echo "north built -> $OUT  (engine: $FRAM/out on classpath at runtime)"
