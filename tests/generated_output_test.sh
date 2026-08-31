#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
beagle="${BEAGLE_HOME:-$HOME/code/beagle/main}"
store="${BEAGLE_STORE_HOME:-$beagle/store}"
tmp="$(mktemp -d)"
trap 'rm -rf -- "${tmp:?}"' EXIT

# CLI-local generated authorities live beside their generated host projection.
# Regenerate into scratch from checked-in typed source, then compare bytes.
for module in \
  agent-catalog \
  agent-catalog-cli \
  agents-cli \
  message-audience \
  message-contract \
  message-id \
  message-routing \
  orchestration-staffing \
  orchestration-project-cli \
  wake-receipt-internal; do
  (
    cd "$root"
    "$beagle/bin/beagle-build" \
      "cli/$module.bclj" \
      "$tmp/$module.clj" >/dev/null
  )
  cmp "$tmp/$module.clj" "$root/cli/$module.clj"
  echo "generated pair cli/$module: passed"
done

for module in \
  agent-catalog-import-test \
  agent-catalog-test \
  config-hooks-test \
  map-contract-test \
  orchestration-parity-test \
  orchestration-root-cwd-test; do
  (
    cd "$root"
    BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build" \
      "cli/tests/$module.bclj" \
      "$tmp/$module.clj" >/dev/null
  )
  cmp "$tmp/$module.clj" "$root/cli/tests/$module.clj"
  echo "generated pair cli/tests/$module: passed"
done

rpc_tmp="$tmp/rpc"
mkdir -p "$rpc_tmp"
(
  cd "$root"
  BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build-all" \
    --module-root "store/src=$store/src" \
    cli/store-rpc-client.bclj cli/coord.bclj \
    --out "$rpc_tmp" >/dev/null
)
cmp "$rpc_tmp/north/store_rpc_client.clj" \
  "$root/cli/store-rpc-client.clj"
cmp "$rpc_tmp/north/coord.clj" "$root/cli/coord.clj"
if rg -n '/home/|\^\{:line' \
  "$root/cli/store-rpc-client.clj" "$root/cli/coord.clj"; then
  echo "generated CLI projection contains source-location or absolute-home residue" >&2
  exit 1
fi
echo "generated pair cli/store-rpc-client + cli/coord: passed"

"$beagle/bin/beagle-build" \
  --module-root "store/src=$store/src" \
  --module-root "north/src=$root/src" \
  "$root/cli/orchestration-import-cli.bclj" \
  "$tmp/orchestration-import-cli.clj" >/dev/null
cmp "$tmp/orchestration-import-cli.clj" \
  "$root/cli/orchestration-import-cli.clj"
echo "generated pair cli/orchestration-import-cli: passed"

work_semantic_tmp="$tmp/work-semantic"
mkdir -p "$work_semantic_tmp"
BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build-all" \
  --module-root "north/src=$root/src" \
  --module-root "store/src=$store/src" \
  "$root/cli/store-rpc-client.bclj" \
  "$root/cli/coord.bclj" \
  "$root/cli/work-catalog.bclj" \
  "$root/cli/work-cli.bclj" \
  "$root/cli/tests/work-catalog-test.bclj" \
  "$root/cli/tests/work-cli-test.bclj" \
  "$root/test/north/referents_test.bclj" \
  "$root/test/north/work_occurrences_test.bclj" \
  --out "$work_semantic_tmp" >/dev/null
cmp "$work_semantic_tmp/north/work_catalog.clj" \
  "$root/cli/work-catalog.clj"
cmp "$work_semantic_tmp/north/work_cli.clj" \
  "$root/cli/work-cli.clj"
cmp "$work_semantic_tmp/north/work_catalog_test.clj" \
  "$root/cli/tests/work-catalog-test.clj"
cmp "$work_semantic_tmp/north/work_cli_test.clj" \
  "$root/cli/tests/work-cli-test.clj"
cmp "$work_semantic_tmp/north/referents.clj" \
  "$root/src/north/referents.clj"
cmp "$work_semantic_tmp/north/work_occurrences.clj" \
  "$root/src/north/work_occurrences.clj"
cmp "$work_semantic_tmp/north/referents_test.clj" \
  "$root/test/north/referents_test.clj"
cmp "$work_semantic_tmp/north/work_occurrences_test.clj" \
  "$root/test/north/work_occurrences_test.clj"
echo "generated semantic work authorities and fixtures: passed"

for module in \
  agent-fact-internal \
  agent-provenance \
  delivery-evidence-internal \
  lifecycle-projection \
  run-event-internal \
  run-fact-internal \
  run-ledger \
  terminal-projection \
  reap; do
  (
    cd "$root"
    BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build" \
      "cli/$module.bclj" \
      "$tmp/$module.clj" >/dev/null
  )
  cmp "$tmp/$module.clj" "$root/cli/$module.clj"
  echo "generated pair cli/$module: passed"
done

BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build" \
  "$root/cli/tests/presence-online-integration-test.bclj" \
  "$tmp/presence-online-integration-test.clj" >/dev/null
cmp "$tmp/presence-online-integration-test.clj" \
  "$root/cli/tests/presence-online-integration-test.clj"
echo "generated pair cli/tests/presence-online-integration-test: passed"

for module in agent-catalog-test orchestration-parity-test; do
  (
    cd "$root"
    "$beagle/bin/beagle-build" \
      "cli/tests/$module.bclj" \
      "$tmp/$module.clj" >/dev/null
  )
  cmp "$tmp/$module.clj" "$root/cli/tests/$module.clj"
  echo "generated pair cli/tests/$module: passed"
done

runtime_transition_tmp="$tmp/runtime-transition"
mkdir -p "$runtime_transition_tmp"
BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build-all" \
  --module-root "north/src=$root/src" \
  "$root/cli/store-runtime-generation.bclj" \
  "$root/cli/tests/store-runtime-authority-transition-test.bclj" \
  "$root/cli/tests/store-runtime-live-attestation-test.bclj" \
  --out "$runtime_transition_tmp" >/dev/null
cmp "$runtime_transition_tmp/north/store_runtime_generation.clj" \
  "$root/cli/store-runtime-generation.clj"
cmp "$runtime_transition_tmp/north/store_runtime_authority_transition_test.clj" \
  "$root/cli/tests/store-runtime-authority-transition-test.clj"
cmp "$runtime_transition_tmp/north/store_runtime_live_attestation_test.clj" \
  "$root/cli/tests/store-runtime-live-attestation-test.clj"
echo "generated Store runtime transition authorities: passed"

BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build" \
  "$root/cli/provider-native-session-projection.bclj" \
  "$tmp/provider-native-session-projection.clj" >/dev/null
cmp "$tmp/provider-native-session-projection.clj" \
  "$root/cli/provider-native-session-projection.clj"

BEAGLE_EMIT_SRCLOC=0 "$beagle/bin/beagle-build" \
  "$root/cli/tests/embedded-store-coordinator-test.bclj" \
  "$tmp/embedded-store-coordinator-test.clj" >/dev/null
cmp "$tmp/embedded-store-coordinator-test.clj" \
  "$root/cli/tests/embedded-store-coordinator-test.clj"
echo "generated embedded Store coordinator fixture: passed"

generated_sdk_js_pair() {
  local source="$1"
  local label="$2"
  local runtime_prefix="$3"
  BEAGLE_EMIT_SRCLOC=0 \
  BEAGLE_JS_RUNTIME_PREFIX="$runtime_prefix" \
    "$beagle/bin/beagle-build" \
      "$root/$source.bjs" \
      "$tmp/$label.js" >/dev/null
  "$beagle/bin/beagle" dts \
    "$root/$source.bjs" \
    "$tmp/$label.d.ts" >/dev/null
  cmp "$tmp/$label.js" "$root/$source.js"
  cmp "$tmp/$label.d.ts" "$root/$source.d.ts"
  echo "generated pair $source: passed"
}

generated_sdk_js_pair \
  sdk/src/provider-neutral-route provider-neutral-route './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/failover failover './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/provider-routing provider-routing './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/routing-economics routing-economics './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/learning-regime learning-regime './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/managed-learning managed-learning './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/run-provenance run-provenance './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/run-ledger run-ledger './bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/providers/catalog providers-catalog '../bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/providers/index providers-index '../bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/providers/anthropic providers-anthropic '../bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/providers/provider-join providers-provider-join '../bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/providers/codex-execution-allocation providers-codex-execution-allocation '../bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/providers/internal-router providers-internal-router '../bridge/generated/beagle/'
generated_sdk_js_pair \
  sdk/src/bridge/provider bridge-provider './generated/beagle/'

BEAGLE_EMIT_SRCLOC=0 \
BEAGLE_JS_RUNTIME_PREFIX='./bridge/generated/beagle/' \
  "$beagle/bin/beagle-build" \
    "$root/sdk/src/harness.bjs" \
    "$tmp/harness.js" >/dev/null
"$beagle/bin/beagle" dts \
  "$root/sdk/src/harness.bjs" \
  "$tmp/harness.d.ts" >/dev/null
cmp "$tmp/harness.js" "$root/sdk/src/harness.js"
cmp "$tmp/harness.d.ts" "$root/sdk/src/harness.d.ts"
echo "generated pair sdk/src/harness: passed"

BEAGLE_EMIT_SRCLOC=0 \
BEAGLE_JS_RUNTIME_PREFIX='../../sdk/src/bridge/generated/beagle/' \
  "$beagle/bin/beagle-build" \
    "$root/agent-runtime/hooks/agent-spawn-guard.bjs" \
    "$tmp/agent-spawn-guard.js" >/dev/null
cmp "$tmp/agent-spawn-guard.js" \
  "$root/agent-runtime/hooks/agent-spawn-guard.js"

BEAGLE_EMIT_SRCLOC=0 \
BEAGLE_JS_RUNTIME_PREFIX='../sdk/src/bridge/generated/beagle/' \
  "$beagle/bin/beagle-build" \
    "$root/bin/north-lifecycle.bjs" \
    "$tmp/north-lifecycle.js" >/dev/null
cmp "$tmp/north-lifecycle.js" "$root/bin/north-lifecycle.js"
echo "generated hook authorities: passed"

sdk_module_root="$tmp/sdk-module-root"
mkdir -p "$sdk_module_root/north/sdk"
cp "$root/sdk/src/routing-metadata.bjs" \
  "$sdk_module_root/north/sdk/routing_metadata.bjs"

BEAGLE_EMIT_SRCLOC=0 BEAGLE_JS_RUNTIME_PREFIX='./bridge/generated/beagle/' \
  "$beagle/bin/beagle-build" \
  "$root/sdk/src/routing-metadata.bjs" \
  "$tmp/routing-metadata.js" >/dev/null
"$beagle/bin/beagle" dts \
  --module-root "north/sdk=$root/sdk/src" \
  "$root/sdk/src/routing-metadata.bjs" \
  "$tmp/routing-metadata.d.ts" >/dev/null
BEAGLE_EMIT_SRCLOC=0 BEAGLE_JS_RUNTIME_PREFIX='./bridge/generated/beagle/' \
  "$beagle/bin/beagle" build \
  --module-root "north/sdk=$sdk_module_root" \
  "$root/sdk/src/orchestration-staffing.bjs" \
  "$tmp/orchestration-staffing.js" >/dev/null
"$beagle/bin/beagle" dts \
  --module-root "north/sdk=$root/sdk/src" \
  --provider "$root/sdk/src/routing-metadata.bjs" \
  "$root/sdk/src/orchestration-staffing.bjs" \
  "$tmp/orchestration-staffing.d.ts" >/dev/null

for module in routing-metadata orchestration-staffing; do
  cmp "$tmp/$module.js" "$root/sdk/src/$module.js"
  cmp "$tmp/$module.d.ts" "$root/sdk/src/$module.d.ts"
done
echo "generated SDK routing authorities: passed"

BEAGLE_EMIT_SRCLOC=0 \
BEAGLE_JS_RUNTIME_PREFIX='./bridge/generated/beagle/' \
  "$beagle/bin/beagle-build" \
    "$root/sdk/src/telemetry.bjs" \
    "$tmp/telemetry.js" >/dev/null
"$beagle/bin/beagle-dts" \
  "$root/sdk/src/telemetry.bjs" \
  "$tmp/telemetry.d.ts" >/dev/null
cmp "$tmp/telemetry.js" "$root/sdk/src/telemetry.js"
cmp "$tmp/telemetry.d.ts" "$root/sdk/src/telemetry.d.ts"
echo "generated SDK telemetry projections: passed"

BEAGLE_EMIT_SRCLOC=0 \
BEAGLE_JS_RUNTIME_PREFIX='./bridge/generated/beagle/' \
  "$beagle/bin/beagle-build" \
    "$root/sdk/src/spawn.bjs" \
    "$tmp/spawn.js" >/dev/null
"$beagle/bin/beagle-dts" \
  "$root/sdk/src/spawn.bjs" \
  "$tmp/spawn.d.ts" >/dev/null
cmp "$tmp/spawn.js" "$root/sdk/src/spawn.js"
cmp "$tmp/spawn.d.ts" "$root/sdk/src/spawn.d.ts"
echo "generated SDK spawn projections: passed"

for module in projections validate staleness audit worker_policy store_runtime_manifest coordinator main; do
  BEAGLE_EMIT_SRCLOC=0 direnv exec "$beagle" "$beagle/bin/beagle-build" \
    --module-root "north/src=$root/src" \
    --module-root "store/src=$store/src" \
    "$root/src/north/$module.bclj" "$tmp/$module.clj" >/dev/null
  cmp "$tmp/$module.clj" "$root/out/north/$module.clj"
done
if rg -n '/home/|\^\{:line' "$root/out/north"/*.clj; then
  echo "generated output contains source-location or absolute-home residue" >&2
  exit 1
fi
echo "generated-output: passed"
