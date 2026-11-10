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
- `doctrine.md` was delivered to the `north` instructions target, which no agent reads. North#869 (2026-11-08) deleted it and the target: Tom's south policy already carried its Done when and Not required lists, the planning skill its 80/20 hardening rule, and the planning and workers skills now carry the rest (run admission with no shadow roles, the not-facts list, stopping at the repairable boundary, an unblocking discovery is not scope, and when a box needs the owner).
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

## One source for every provider (north#1041)

Claude and Codex read the published generation (`~/.local/state/north/agents/current`) through their home directories. OpenCode and Pi workers run in a confined scratch home with the generation hidden, so until 2026-11-10 they saw only the north prompt, the DeepSeek overlay and the repository's own AGENTS.md.

`Deepseek::host` (crates/north-providers/src/providers/opencode/fleet.rs) now reads the same generation for every Go-route worker of either harness: the `instructions/shared/AGENTS.md` text joins the first-turn context before the overlay, and `skills/shared` is copied under the harness's scratch skills directory (`config/opencode/skills` for OpenCode, `agent/skills` for Pi), where its native skill discovery finds it. The free route gets neither, because it may train on its prompts and the shared text is Tom's private policy; its rules come from the repository's AGENTS.md and the overlay.

`north runtime probe launch claude|opencode|pi`, which the runtime canary runs per host, launches a `--test` worker and asks it for the words of the first heading of the policy it was given, without the `#` mark, which a terse model answered alone (north#1022); the probe fails unless the reply holds the heading of the published `instructions/shared/AGENTS.md`, so a provider whose first turn lacks the shared text fails the canary and rolls the deploy back.

agy is no fleet provider (no `north fleet launch --provider agy`, no runtime to carry text), so it has no first turn to project into; its guard layers are those named in harness/policy-owners.toml.
