# agent-machinery

agent-machinery is the provider-independent part of the harness: the delivery doctrine every agent
reads and the engineering procedures any agent can follow. It is a source authority, not a runtime. It does not connect to
providers, manage accounts or leases, dispatch work, keep telemetry, coordinate live sessions,
install hooks or project policy into a harness.

## Who reads it

- **`agents sync`** in the host repository reads [catalog.json](catalog.json) as a project package:
  each unit it lists becomes an installable skill or module, and the operator's catalog chooses
  which ones each agent receives. [doctrine.md](doctrine.md) is delivered as instructions.
- **Agents** load a skill's procedure from its file under [skills/](skills/) once `agents sync` has
  linked it, and read the doctrine as part of their instructions.
- **The host's file guard** reads [file-skills.toml](file-skills.toml) to name the skill an edit to a
  file type should follow.

## How it relates to the host skills

The host repository keeps a second skill root, [../harness/skills/](../harness/skills/), for
procedures that depend on its own machines, accounts, landing path or workforce. This package keeps
only the procedures that hold for any project and any provider. `agents sync` publishes both roots
side by side, so a link from one skill to another resolves in either root. A skill belongs here only
when nothing in it is host-specific.

## Reading order

Stop at the first that answers:

- this file, for what the package holds and where each fact lives;
- [doctrine.md](doctrine.md), the portable delivery rules every agent follows;
- the skill for the task at hand, listed below;
- [catalog.json](catalog.json) and [catalog.schema.json](catalog.schema.json), for what is exported
  and how a consumer resolves it;
- [PROVENANCE.md](PROVENANCE.md) and [NOTICE](NOTICE), for where the material came from.

## Where each fact lives

- Which skills, modules and assets the package exports: [catalog.json](catalog.json).
- The catalog's shape: [catalog.schema.json](catalog.schema.json), and the extra rules
  the host repository's package check enforces.
- A procedure, and which agents receive it (an optional `agents` field in its frontmatter; a skill
  without it keeps its catalog targets): that skill's own file under [skills/](skills/).
- Which skill an edit to a file type should follow: [file-skills.toml](file-skills.toml).
- Each script's flags: its own usage text, for example `bun scripts/effect-kit.mjs --help`.
- Source revisions, imports and attribution: [PROVENANCE.md](PROVENANCE.md).
- The licence choice: [LICENSE](LICENSE), with the texts in [LICENSE-MIT](LICENSE-MIT) and
  [LICENSE-APACHE](LICENSE-APACHE).
- How tests here are kept and run: the [verification skill](skills/verification/SKILL.md).

The lists below mirror the catalog; the host repository's readme_links test
fails when they differ.

## Skills

- [`astro`](skills/astro/SKILL.md)
- [`debugging`](skills/debugging/SKILL.md)
- [`effect-development`](skills/effect-development/SKILL.md)
- [`external-code`](skills/external-code/SKILL.md)
- [`planning`](skills/planning/SKILL.md)
- [`program-craftsmanship`](skills/program-craftsmanship/SKILL.md)
- [`rust-development`](skills/rust-development/SKILL.md)
- [`threejs`](skills/threejs/SKILL.md)
- [`verification`](skills/verification/SKILL.md)

Which of them each module groups is the `members` list of that module in [catalog.json](catalog.json).

## Modules

- `agent-machinery`
- `agent-practice`

## Assets

- [`agent-machinery-doctrine`](doctrine.md)
- [`export-catalog-schema`](catalog.schema.json)

## Scripts

- [effect-kit.mjs](scripts/effect-kit.mjs): the Effect kit that vendors upstream Effect and checks a
  project against it.

## License

Licensed under either the MIT or the Apache license, at your option; see [LICENSE](LICENSE) and
[PROVENANCE.md](PROVENANCE.md) for the retained attribution.
