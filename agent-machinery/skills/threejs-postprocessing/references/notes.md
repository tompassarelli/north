# Three.js postprocessing: full notes

`SKILL.md` is the routine guide. Select only the topic needed:

- [Pipeline ownership, output, and resize](pipeline-and-resize.md) — EffectComposer Setup; Handle Resize.
- [Effect recipes and tradeoffs](effects.md) — Common Effects; Custom ShaderPass; Combining Multiple Effects.
- [Targets, composition, and backend differences](composition-and-backends.md) — Render to Texture; Multi-Pass Rendering; WebGPU Post-Processing (Three.js r150+).

These notes preserve implementation detail and tradeoffs for revising `SKILL.md`.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `agent-machinery:PROVENANCE.md` and `agent-machinery:NOTICE`.
