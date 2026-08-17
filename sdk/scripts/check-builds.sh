#!/usr/bin/env bash
set -euo pipefail

entries=(
  src/dispatch.ts
  src/spawn.ts
  src/routing-economics-preflight-cli.ts
  src/provider-capability-admission-cli.ts
  src/run-artifact-read-cli.ts
  src/run-share-cli.ts
  src/spawn-doctor-probe.ts
  src/integrations/linear/cli.ts
  src/ccr-cli.ts
  src/succession-cli.ts
  src/acp/cli.ts
  src/tier1-distiller-cli.ts
)
max_parallel=4
index=0

while ((index < ${#entries[@]})); do
  batch_entries=()
  batch_pids=()
  for ((slot = 0; slot < max_parallel && index < ${#entries[@]}; slot += 1)); do
    entry="${entries[index]}"
    bun build "$entry" --no-bundle > /dev/null &
    batch_entries+=("$entry")
    batch_pids+=("$!")
    ((index += 1))
  done

  for batch_slot in "${!batch_pids[@]}"; do
    set +e
    wait "${batch_pids[batch_slot]}"
    status=$?
    set -e
    if ((status != 0)); then
      printf 'build failed: %s (status %d)\n' "${batch_entries[batch_slot]}" "$status" >&2
      exit "$status"
    fi
  done
done
