# Three.js textures: full notes

`SKILL.md` is the routine guide. Select only the topic needed:

- [Loading, color, and sampling](loading-and-sampling.md) — Texture Loading; Texture Configuration.
- [Texture sources and environments](sources-and-environments.md) — Texture Types; Cube Textures; HDR Textures; CubeCamera; Procedural Textures.
- [UVs, atlases, and material maps](uv-and-material-maps.md) — UV Mapping; Texture Atlas; Material Texture Maps.
- [Render targets and resource ownership](targets-and-memory.md) — Render Targets; Texture Memory Management.

These notes preserve implementation detail and tradeoffs for revising `SKILL.md`.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `agent-machinery:PROVENANCE.md` and `agent-machinery:NOTICE`.
