---
name: threejs-interaction-reference
description: Detailed Three.js interaction rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js interaction: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Picking, selection, and coordinates](references/picking-and-selection.md) — Raycaster; Selection System; World-Screen Coordinate Conversion.
- [north-v2:Controls and gesture ownership](references/controls-and-gestures.md) — Camera Controls; TransformControls; DragControls.
- [north-v2:Input lifecycle and cost](references/input-lifecycle.md) — Keyboard Input; Event Handling Best Practices.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
