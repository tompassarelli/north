---
name: elicit-distilled
description: Calibrate a orchestration delta for a model the plugin doesn't cover yet (a new Claude tier, a new generation, or any agent model) using the elicit → subtract → compile method. Use when adding a new model to the squad or when a model generation changes and its delta may be stale.
---

# Calibrate a model delta

Keep only model-specific gaps. Isolate the self-report: the target reads/searches no files first. Subtract native behavior; classify every remainder: limit→procedure, tell→trigger, stale→correction, skipped→checkpoint. Compile in model vocabulary as phase-grouped numbered one-line prompts. Keep `docs/deltas/<model>.md` ≤50 lines; rebuild its stock templates. Deltas transfer mode switches, not capacity.

Details: `agents path elicit-reference`.
