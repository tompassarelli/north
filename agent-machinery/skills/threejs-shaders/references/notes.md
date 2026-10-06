# Three.js shaders: full notes

`SKILL.md` is the routine guide. Select only the topic needed:

- [Shader interfaces and data flow](interfaces.md) — ShaderMaterial vs RawShaderMaterial; Uniforms; Varyings; Common Material Properties.
- [Effect math and patterns](effect-math.md) — Common Shader Patterns; GLSL Built-in Functions.
- [Built-in integration, instancing, and diagnosis](integration-and-diagnostics.md) — Extending Built-in Materials; Shader Includes; Instanced Shaders; Debugging Shaders.

These notes preserve implementation detail and tradeoffs for revising `SKILL.md`.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `agent-machinery:PROVENANCE.md` and `agent-machinery:NOTICE`.
