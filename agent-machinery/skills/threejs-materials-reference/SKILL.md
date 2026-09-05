---
name: threejs-materials-reference
description: Detailed Three.js materials rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js materials: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Material selection and parameters](references/material-families.md) — Material Types Overview; MeshBasicMaterial; MeshLambertMaterial; MeshPhongMaterial; MeshStandardMaterial (PBR); MeshPhysicalMaterial (Advanced PBR); MeshToonMaterial; MeshNormalMaterial; MeshDepthMaterial; PointsMaterial; LineBasicMaterial & LineDashedMaterial.
- [north-v2:Render state, maps, and shared ownership](references/render-state-and-ownership.md) — Common Material Properties; Multiple Materials; Environment Maps; Material Cloning and Modification.
- [north-v2:Custom shader boundaries](references/custom-materials.md) — ShaderMaterial; RawShaderMaterial.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
