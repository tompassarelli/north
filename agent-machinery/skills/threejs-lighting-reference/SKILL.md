---
name: threejs-lighting-reference
description: Detailed Three.js lighting rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js lighting: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Light selection and staging](references/light-selection.md) — Light Types Overview; AmbientLight; HemisphereLight; DirectionalLight; PointLight; SpotLight; RectAreaLight; Common Lighting Setups.
- [north-v2:Shadow setup and diagnosis](references/shadows.md) — Shadow Setup; Light Helpers.
- [north-v2:Environment lighting, probes, and motion](references/environment-and-motion.md) — Environment Lighting (IBL); Light Probes (Advanced); Light Animation.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
