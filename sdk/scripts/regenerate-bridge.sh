#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -z "${BEAGLE_HOME:-}" || "$BEAGLE_HOME" != /* ]]; then
  printf 'BEAGLE_HOME must be set to an absolute Beagle checkout path\n' >&2
  exit 1
fi
beagle="$BEAGLE_HOME"
generated="$root/src/bridge/generated"
runtime_source="$beagle/beagle-lib/lib/beagle"
source_stage="$(mktemp -d "${TMPDIR:-/tmp}/north-bridge-source.XXXXXX")"
output_stage="$(mktemp -d "${TMPDIR:-/tmp}/north-bridge-output.XXXXXX")"

cleanup() {
  rm -rf -- "$source_stage" "$output_stage"
}
trap cleanup EXIT

for required in \
  "$beagle/bin/beagle-build" \
  "$beagle/bin/beagle-fmt" \
  "$runtime_source/core.js" \
  "$beagle/LICENSE-MIT" \
  "$root/scripts/generate-bridge-declarations.ts"; do
  if [[ ! -f "$required" ]]; then
    printf 'missing Beagle bridge-generation input: %s\n' "$required" >&2
    exit 1
  fi
done

direnv exec "$beagle" "$beagle/bin/beagle-fmt" --check \
  "$root/src/bridge/model.bjs" \
  "$root/src/bridge/app.bjs"

mkdir -p \
  "$source_stage/north/bridge" \
  "$output_stage/beagle" \
  "$output_stage/north/bridge"

install -m 0644 "$root/src/bridge/model.bjs" "$source_stage/north/bridge/model.bjs"
install -m 0644 "$root/src/bridge/app.bjs" "$source_stage/north/bridge/app.bjs"

for runtime_file in core.js exception-dispatch.js exception-info.js hamt.js host.js; do
  install -m 0644 "$runtime_source/$runtime_file" "$output_stage/beagle/$runtime_file"
done
install -m 0644 "$beagle/LICENSE-MIT" "$output_stage/beagle/LICENSE-MIT"

BEAGLE_EMIT_SRCLOC=0 BEAGLE_JS_RUNTIME_PREFIX='../../beagle/' \
  direnv exec "$beagle" "$beagle/bin/beagle-build" \
    --module-root "north-bridge=$source_stage" \
    "$source_stage/north/bridge/model.bjs" \
    "$output_stage/north/bridge/model.js"

BEAGLE_EMIT_SRCLOC=0 BEAGLE_JS_RUNTIME_PREFIX='../../beagle/' \
  direnv exec "$beagle" "$beagle/bin/beagle-build" \
    --module-root "north-bridge=$source_stage" \
    "$source_stage/north/bridge/app.bjs" \
    "$output_stage/north/bridge/app.js"

bun run "$root/scripts/generate-bridge-declarations.ts" \
  "$output_stage/north/bridge/app.js" \
  "$output_stage/north/bridge/app.d.ts"

node --check "$output_stage/north/bridge/model.js"
node --check "$output_stage/north/bridge/app.js"

# Generation is complete before the live tree changes. A compiler failure can
# therefore never leave North with only one member of the source/output pair.
mkdir -p "$generated/beagle" "$generated/north/bridge"
for runtime_file in LICENSE-MIT core.js exception-dispatch.js exception-info.js hamt.js host.js; do
  install -m 0644 "$output_stage/beagle/$runtime_file" "$generated/beagle/$runtime_file"
done
for bridge_file in model.js app.js app.d.ts; do
  install -m 0644 "$output_stage/north/bridge/$bridge_file" \
    "$generated/north/bridge/$bridge_file"
done
rm -f -- \
  "$generated/north/bridge/model.js.map" \
  "$generated/north/bridge/app.js.map"

printf 'bridge generated -> %s\n' "$generated"
