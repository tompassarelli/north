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
mkdir -p "$root/.cache"
source_stage="$(mktemp -d "$root/.cache/north-bridge-source.XXXXXX")"
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
  "$root/src/bridge/referent-actions.bjs" \
  "$root/src/bridge/app.bjs" \
  "$root/src/bridge/protocol.bjs" \
  "$root/src/bridge/app-launch-reservation.bjs" \
  "$root/src/bridge/cli.bjs"

mkdir -p \
  "$source_stage/north/bridge" \
  "$output_stage/beagle" \
  "$output_stage/north/bridge" \
  "$output_stage/node_modules"
ln -s "$root/node_modules"/* "$output_stage/node_modules/"
ln -s "$root" "$output_stage/node_modules/north-sdk"

for bridge_source in model referent-actions app protocol app-launch-reservation cli; do
  source_stem="${bridge_source//-/_}"
  install -m 0644 "$root/src/bridge/$bridge_source.bjs" \
    "$source_stage/north/bridge/$source_stem.bjs"
done

for runtime_file in core.js exception-dispatch.js exception-info.js hamt.js host.js; do
  install -m 0644 "$runtime_source/$runtime_file" "$output_stage/beagle/$runtime_file"
done
install -m 0644 "$beagle/LICENSE-MIT" "$output_stage/beagle/LICENSE-MIT"

for bridge_source in model referent-actions app protocol app-launch-reservation cli; do
  source_stem="${bridge_source//-/_}"
  BEAGLE_EMIT_SRCLOC=0 BEAGLE_JS_RUNTIME_PREFIX='../../beagle/' \
    direnv exec "$beagle" "$beagle/bin/beagle-build" \
      --module-root "north-bridge=$source_stage" \
      "$source_stage/north/bridge/$source_stem.bjs" \
      "$output_stage/north/bridge/$bridge_source.js"
  declaration_profile="$bridge_source"
  declaration_input="$output_stage/north/bridge/$bridge_source.js"
  if [[ "$bridge_source" == referent-actions ]]; then
    declaration_profile="model"
  fi
  if [[ "$bridge_source" == model ]]; then
    declaration_input="$output_stage/north/bridge/model.declarations.js"
    sed \
      -e 's/as "->Agent"/as "agent-constructor"/' \
      -e 's/as "->BridgeSnapshot"/as "bridge-snapshot-constructor"/' \
      -e 's/as "->ExecutionItem"/as "execution-item-constructor"/' \
      -e 's/as "->TrackedThing"/as "tracked-thing-constructor"/' \
      -e 's/as "Agent"/as "agent-record"/' \
      -e 's/as "BridgeSnapshot"/as "bridge-snapshot-record"/' \
      -e 's/as "ExecutionItem"/as "execution-item-record"/' \
      -e 's/as "TrackedThing"/as "tracked-thing-record"/' \
      "$output_stage/north/bridge/model.js" > "$declaration_input"
  fi
  direnv exec "$beagle" bun run "$root/scripts/generate-bridge-declarations.ts" \
    "$declaration_profile" \
    "$declaration_input" \
    "$output_stage/north/bridge/$bridge_source.d.ts"
  if [[ "$bridge_source" == model ]]; then
    sed -i \
      -e 's/as "agent-constructor"/as "->Agent"/' \
      -e 's/as "bridge-snapshot-constructor"/as "->BridgeSnapshot"/' \
      -e 's/as "execution-item-constructor"/as "->ExecutionItem"/' \
      -e 's/as "tracked-thing-constructor"/as "->TrackedThing"/' \
      -e 's/as "agent-record"/as "Agent"/' \
      -e 's/as "bridge-snapshot-record"/as "BridgeSnapshot"/' \
      -e 's/as "execution-item-record"/as "ExecutionItem"/' \
      -e 's/as "tracked-thing-record"/as "TrackedThing"/' \
      "$output_stage/north/bridge/model.d.ts"
  fi
  direnv exec "$beagle" bun build \
    "$output_stage/north/bridge/$bridge_source.js" --no-bundle > /dev/null
done

# Generation is complete before the live tree changes. A compiler failure can
# therefore never leave North with only one member of the source/output pair.
mkdir -p "$generated/beagle" "$generated/north/bridge"
for runtime_file in LICENSE-MIT core.js exception-dispatch.js exception-info.js hamt.js host.js; do
  install -m 0644 "$output_stage/beagle/$runtime_file" "$generated/beagle/$runtime_file"
done
for bridge_file in \
  model.js \
  model.d.ts \
  referent-actions.js referent-actions.d.ts \
  app.js app.d.ts \
  cli.js cli.d.ts; do
  install -m 0644 "$output_stage/north/bridge/$bridge_file" \
    "$generated/north/bridge/$bridge_file"
done
rm -f -- \
  "$generated/north/bridge/model.js.map" \
  "$generated/north/bridge/referent-actions.js.map" \
  "$generated/north/bridge/app.js.map" \
  "$generated/north/bridge/protocol.js.map" \
  "$generated/north/bridge/app-launch-reservation.js.map" \
  "$generated/north/bridge/cli.js.map"

printf 'bridge generated -> %s\n' "$generated"
