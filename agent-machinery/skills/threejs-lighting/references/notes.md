# Three.js lighting: full notes

`SKILL.md` is the routine guide. Select only the topic needed:

- [Light selection and staging](light-selection.md) — Light Types Overview; AmbientLight; HemisphereLight; DirectionalLight; PointLight; SpotLight; RectAreaLight; Common Lighting Setups.
- [Shadow setup and diagnosis](shadows.md) — Shadow Setup; Light Helpers.
- [Environment lighting, probes, and motion](environment-and-motion.md) — Environment Lighting (IBL); Light Probes (Advanced); Light Animation.

These notes preserve implementation detail and tradeoffs for revising `SKILL.md`.
Examples are illustrative, not browser-verified here; the installed release
owns exact APIs. Alternatives and performance hypotheses are not default rules.

Adapted from CloudAI-X/threejs-skills at
`b1c623076c661fc9b03dac19292e825a5d106823`; upstream's README grants MIT use,
modification, and distribution. Attribution and licensing evidence are retained
in `agent-machinery:PROVENANCE.md` and `agent-machinery:NOTICE`.
