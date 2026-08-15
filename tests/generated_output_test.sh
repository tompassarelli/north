#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
beagle="${BEAGLE_HOME:-$HOME/code/beagle/main}"
fram="${FRAM_HOME:-$HOME/code/fram/main}"
tmp="$(mktemp -d)"
trap 'rm -rf -- "${tmp:?}"' EXIT
ln -sfn "$fram/src/fram" "$root/src/fram"
for module in projections validate staleness audit worker_policy main; do
  BEAGLE_EMIT_SRCLOC=0 direnv exec "$beagle" "$beagle/bin/beagle-build" \
    "$root/src/north/$module.bclj" "$tmp/$module.clj" >/dev/null
  cmp "$tmp/$module.clj" "$root/out/north/$module.clj"
done
if rg -n '/home/|\^\{:line' "$root/out/north"/*.clj; then
  echo "generated output contains source-location or absolute-home residue" >&2
  exit 1
fi
echo "generated-output: passed"
