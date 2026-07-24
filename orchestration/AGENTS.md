# Gaffer contributor instructions

Gaffer is the provider-neutral routing doctrine for delegated agent work. Keep
task shape, role/function, task grade, domain requirements, topology, posture,
semantic tier, deliberation, and provider/model selection as separate concepts.
Human-readable task grades describe the work; semantic tiers describe model
capability. Neither implies the other.

- `doctrine.md` is the canonical runtime doctrine.
- `docs/routing.md` defines the provider-neutral routing contract.
- `providers/*.json` are provider catalogs; concrete model names belong there,
  not in routing laws or North-facing contracts.
- `agents/*.md` and `docs/adapters/north.md` are generated. Change source docs,
  provider catalogs, or `scripts/build-agents.mjs`, then rebuild them.
- Preserve the Claude Code plugin as one adapter. Do not make Claude-specific
  frontmatter or hook behavior the portable contract.
- Run `node scripts/validate.mjs` before finishing changes.

Use semantic tiers (`economy`, `standard`, `senior`, `frontier`) in new shared
interfaces. A provider adapter resolves a tier to its concrete model and
reasoning/effort controls at dispatch time.
