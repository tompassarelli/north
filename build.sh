#!/usr/bin/env bash
# Recompile North's Beagle (.bclj) sources to Clojure into out/.
#
# North consumes Beagle Store from Beagle's store subtree. Its source root is
# declared explicitly so the type checker resolves store.* fully, and
# store/out is on the runtime classpath (see bin/north).
# You only need this to rebuild from the .bclj sources (requires Beagle).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/src"; OUT="$HERE/out"
BEAGLE="${BEAGLE_HOME:-$HOME/code/beagle/main}"
STORE="${BEAGLE_STORE_HOME:-$BEAGLE/store}"

mkdir -p "$OUT/north"
for m in projections validate staleness audit worker_policy store_runtime_manifest main; do
  BEAGLE_EMIT_SRCLOC=0 direnv exec "$BEAGLE" "$BEAGLE/bin/beagle-build" \
    --module-root "north/src=$SRC" \
    --module-root "store/src=$STORE/src" \
    "$SRC/north/$m.bclj" "$OUT/north/$m.clj" >/dev/null
  echo "  built north/$m"
done
echo "north built -> $OUT  (engine: $STORE/out on classpath at runtime)"
