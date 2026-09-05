---
name: threejs-geometry-reference
description: Detailed Three.js geometry rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js geometry: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Built-in shapes and render primitives](references/primitives.md) — Built-in Geometries; EdgesGeometry & WireframeGeometry; Points; Lines.
- [north-v2:Buffers, attributes, and topology](references/buffers-and-topology.md) — BufferGeometry; Geometry Utilities; Common Patterns.
- [north-v2:Instancing and geometry cost](references/instancing-and-cost.md) — InstancedMesh; InstancedBufferGeometry (Advanced).

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
