---
name: threejs-postprocessing-reference
description: Detailed Three.js postprocessing rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js postprocessing: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Pipeline ownership, output, and resize](references/pipeline-and-resize.md) — EffectComposer Setup; Handle Resize.
- [north-v2:Effect recipes and tradeoffs](references/effects.md) — Common Effects; Custom ShaderPass; Combining Multiple Effects.
- [north-v2:Targets, composition, and backend differences](references/composition-and-backends.md) — Render to Texture; Multi-Pass Rendering; WebGPU Post-Processing (Three.js r150+).

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
