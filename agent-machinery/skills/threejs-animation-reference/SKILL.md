---
name: threejs-animation-reference
description: Detailed Three.js animation rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js animation: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Clips, mixers, and playback](references/playback.md) — Animation System Overview; AnimationClip; AnimationMixer; AnimationAction; Loading GLTF Animations.
- [north-v2:Skeletons, morphs, and blending](references/skeletons-and-morphs.md) — Skeletal Animation; Morph Targets; Animation Blending; Animation Utilities.
- [north-v2:Procedural motion and cost](references/procedural-motion.md) — Procedural Animation Patterns.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
