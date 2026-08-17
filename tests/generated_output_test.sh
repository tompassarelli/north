#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
beagle="${BEAGLE_HOME:-$HOME/code/beagle/main}"
fram="${BEAGLE_STORE_HOME:-$beagle/store}"
tmp="$(mktemp -d)"
trap 'rm -rf -- "${tmp:?}"' EXIT
for module in projections validate staleness audit worker_policy main; do
  BEAGLE_EMIT_SRCLOC=0 direnv exec "$beagle" "$beagle/bin/beagle-build" \
    --module-root "north/src=$root/src" \
    --module-root "store/src=$fram/src" \
    "$root/src/north/$module.bclj" "$tmp/$module.clj" >/dev/null
  cmp "$tmp/$module.clj" "$root/out/north/$module.clj"
done
if rg -n '/home/|\^\{:line' "$root/out/north"/*.clj; then
  echo "generated output contains source-location or absolute-home residue" >&2
  exit 1
fi
echo "generated-output: passed"
