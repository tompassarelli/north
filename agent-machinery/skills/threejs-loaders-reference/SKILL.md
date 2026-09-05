---
name: threejs-loaders-reference
description: Detailed Three.js loaders rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js loaders: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Asset formats and decoder setup](references/formats-and-decoders.md) — Texture Loading; GLTF/GLB Loading; Other Model Formats.
- [north-v2:Async readiness, progress, and caching](references/async-and-caching.md) — LoadingManager; Async/Promise Loading; Caching.
- [north-v2:Source URLs and failures](references/sources-and-errors.md) — Loading from Different Sources; Error Handling.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
