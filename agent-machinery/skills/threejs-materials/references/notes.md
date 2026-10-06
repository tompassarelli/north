# Three.js materials: full notes

`SKILL.md` is the routine guide. Select only the topic needed:

- [Material selection and parameters](material-families.md) — Material Types Overview; MeshBasicMaterial; MeshLambertMaterial; MeshPhongMaterial; MeshStandardMaterial (PBR); MeshPhysicalMaterial (Advanced PBR); MeshToonMaterial; MeshNormalMaterial; MeshDepthMaterial; PointsMaterial; LineBasicMaterial & LineDashedMaterial.
- [Render state, maps, and shared ownership](render-state-and-ownership.md) — Common Material Properties; Multiple Materials; Environment Maps; Material Cloning and Modification.
- [Custom shader boundaries](custom-materials.md) — ShaderMaterial; RawShaderMaterial.

These notes preserve implementation detail and tradeoffs for revising `SKILL.md`.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `agent-machinery:PROVENANCE.md` and `agent-machinery:NOTICE`.
