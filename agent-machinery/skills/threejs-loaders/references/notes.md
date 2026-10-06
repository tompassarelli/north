# Three.js loaders: full notes

`SKILL.md` is the routine guide. Select only the topic needed:

- [Asset formats and decoder setup](formats-and-decoders.md) — Texture Loading; GLTF/GLB Loading; Other Model Formats.
- [Async readiness, progress, and caching](async-and-caching.md) — LoadingManager; Async/Promise Loading; Caching.
- [Source URLs and failures](sources-and-errors.md) — Loading from Different Sources; Error Handling.

These notes preserve implementation detail and tradeoffs for revising `SKILL.md`.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `agent-machinery:PROVENANCE.md` and `agent-machinery:NOTICE`.
