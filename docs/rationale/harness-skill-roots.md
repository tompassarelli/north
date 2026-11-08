# Dissolving agent-machinery into harness

Decided 2026-11-08 (north#868) after an outside assessment proposed it. Tom asked for it to be reviewed and, if it held, carried out. Every claim below was checked against north, south, nixos-config, makgora, wisp and the other checkouts under ~/code, plus npm and GitHub Packages.

## Verdict

Option A: dissolve `agent-machinery/` into `harness/`. Option B, keeping a portable wall in its own directory, needed something outside north to consume the portable set. Nothing does.

What the review confirmed:
- The name contradicted north's vocabulary: docs/terms.md defines the harness as the machinery that turns a model into an agent, and the directory was a migration fossil (its provenance record calls the standalone repository a migration input).
- The npm package `@tompassarelli/agent-machinery` was never published. npm answers 404 and GitHub Packages lists no npm package. No file under ~/code imported its index, contracts or exposure-profile validator. Yet every lander warm-up ran `bun install` for it, and every edit under the directory, Markdown skill edits included, ran its bun tests after landing.
- The split broke cross-skill links on GitHub: harness skills linked to portable skills, and the other way round, by sibling paths that resolve only after `agents sync` publishes both roots side by side.
- The only real difference between the two skill roots was the package boundary check (no home paths, product names, hooks or provider metadata).

What the review found that the assessment missed:
- South's catalog named north files by path. `check-owners` verifies those paths against the other repository's main, in both north's and south's landings, so a plain move had no landing order that passed. The fix extends the rule north already used for guards: South names every north-owned unit by a `structure.toml` `[[registered]]` id, and moving a north file edits only north.
- makgora and wisp ran `effect-kit.mjs` by its north path (their CI, makgora's pre-push and its landing check), so their lanes landed right after the move.
- North's root had no licence text, although Cargo.toml declares `MIT OR Apache-2.0`. The package's licence texts became the root's, and its third-party credits (Three.js and Astro notes, the cavecrew and Matt Pocock inspirations) moved word for word into the root NOTICE.
- `doctrine.md` is delivered to the `north` instructions target, which no agent reads. It moved unchanged; north#869 decides whether it is delivered or folded into the skills that own its rules.
- docs/rationale/non-rust-inventory.md had called the package published. It was not.

## What changed

- `8d1da51755` (north): structure.toml `[[registered]]` ids for the 22 north skills, the code bootstrap, the doctrine and the defaults.
- `f213eaaf` (south): the catalog names those units by id, registers the nine skills directly, makes the doctrine a baseline and drops `projectPackage`.
- `1d0ef38094` (north): the npm/Ajv wrapper and its bun warm-up, after-landing test and CI steps are deleted (659 lines).
- `bed4ece78e` (north): the `projectPackage` catalog path and the package check are deleted, and the portable rule moves into tree-policy.
- `beb32ee5f4` (north): the outgoing private-text scan follows renames, so a pure move is not judged as new prose. It had timed out twice on this move.
- `7df4702e42` (north): the move itself.
- One lane each in makgora, wisp, south and nixos-config repoints the old paths (effect-kit in CI, pre-push and landing checks; policy text and the privacy deny-list).

The portable property survives as frontmatter: a skill marked `portable: true` must stay free of home paths, product names, hooks and provider metadata, and `north check harness tree-policy` enforces it with the portable-skill-check canary. The `projectPackage` catalog path, whose only user was this package, is gone, and so are `north config check`'s package form and catalog.json. A scratch `agents sync` against the new catalog published the same skills, skill contents, instructions and active hooks as before; only the two package modules left the active list.
