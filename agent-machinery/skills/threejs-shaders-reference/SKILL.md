---
name: threejs-shaders-reference
description: Detailed Three.js shaders rationale and examples; load only for an unresolved detail or explicit request.
---

# Three.js shaders: full notes

The distilled sibling is the routine guide. Select only the topic needed:

- [north-v2:Shader interfaces and data flow](references/interfaces.md) — ShaderMaterial vs RawShaderMaterial; Uniforms; Varyings; Common Material Properties.
- [north-v2:Effect math and patterns](references/effect-math.md) — Common Shader Patterns; GLSL Built-in Functions.
- [north-v2:Built-in integration, instancing, and diagnosis](references/integration-and-diagnostics.md) — Extending Built-in Materials; Shader Includes; Instanced Shaders; Debugging Shaders.

These notes preserve implementation detail and tradeoffs for re-distillation.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `north-v2:agent-machinery/PROVENANCE.md` and `north-v2:agent-machinery/NOTICE`.
