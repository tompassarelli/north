---
name: threejs-textures-reference
description: Detailed Three.js textures rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js textures: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Loading, color, and sampling](references/loading-and-sampling.md) — Texture Loading; Texture Configuration.
- [north-v2:Texture sources and environments](references/sources-and-environments.md) — Texture Types; Cube Textures; HDR Textures; CubeCamera; Procedural Textures.
- [north-v2:UVs, atlases, and material maps](references/uv-and-material-maps.md) — UV Mapping; Texture Atlas; Material Texture Maps.
- [north-v2:Render targets and resource ownership](references/targets-and-memory.md) — Render Targets; Texture Memory Management.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
