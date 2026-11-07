# Non-Rust inventory

Every non-Rust code file left in North on 2026-11-06 for north#88 (the rows of files since ported or deleted are dropped; git history has them): shell, Python, TypeScript and JavaScript files, and shebang scripts in `bin/` and `harness/`. Tom asked on 2026-11-04 that any non-Rust code have an extremely strong reason to exist; each row below names the port box on north#88 or the reason the file stays. Data (Markdown, TOML, JSON, YAML, fixtures, Nix, `flake.nix`, `flake.lock`) is not code and is not listed.

Rows say `port: ...` (a box on north#88, family named), `stay` (with the reason) or `delete`. A file that stays is either a launcher or PATH name with no Rust equivalent possible, glue of at most 80 code lines, or part of a Bun/Node package whose JS API or JS-only dependency is the point. Where a Rust twin exists and a deployed unit or another repository still names the file, the row names the caller whose change lands first.

## Inventory

| File | Lines | Disposition | Reason / note |
|---|---|---|---|
| agent-machinery/index.mjs | 42 | stay | entry of the published @tompassarelli/agent-machinery npm package, whose JS API is the product for outside consumers |
| agent-machinery/scripts/contracts.mjs | 29 | stay | exported contract schemas of the published package |
| agent-machinery/scripts/effect-kit.mjs | 336 | stay | drives the target project's own TypeScript compiler and Effect language service (createRequire into the project's node_modules); only JS can load them |
| agent-machinery/scripts/project-exposure-profile.mjs | 128 | stay | exported profile API of the published package (project-exposure-v1) |
| agent-machinery/tests/capability-contract.test.mjs | 19 | stay | bun tests of the published package modules above |
| agent-machinery/tests/contracts.test.mjs | 37 | stay | bun tests of the published package modules above |
| agent-machinery/tests/package-boundary.test.mjs | 24 | stay | bun tests of the published package modules above |
| bin/agy | 6 | stay | 6-line launcher for the downloaded agy runtime (exports env then execs a vendor binary) |
| bin/automation-chrome | 9 | stay | 9-line flock wrapper that execs the Playwright CLI in lib/automation-chrome |
| bin/automation-chrome.test.sh | 9 | stay | test of the Playwright wrapper |
| bin/bun | 14 | stay | PATH shadow that routes bun through `north capacity gate` admission, then execs the real bun; must be an executable named bun (<=80-line glue) |
| bin/cargo | 14 | stay | same admission shadow for cargo |
| bin/depbox | 88 | stay | execs bwrap/unshare sandbox around an arbitrary command (namespace setup before any runtime exists); glue under 80 code lines |
| bin/effect-kit | 4 | stay | 4-line PATH name for agent-machinery/scripts/effect-kit.mjs |
| bin/gh-as-app | 4 | stay | 4-line glue: runs gh with the token printed by gh-app-token |
| bin/gh | 19 | stay | 19-line PATH shim: finds the real gh and runs it through `north gh-session` in a fleet session; glue |
| bin/ssh | 15 | stay | 15-line PATH shim: finds the real ssh and runs it through `north ssh-session` in a fleet session; glue |
| bin/land-shell | 11 | stay | workflow-free 10-line wrapper that runs a land-check command inside depbox (and nix develop with --dev); used by .safe-push, bin/head-north and red_check.rs; glue |
| bin/mutant | 3 | stay | 3-line exec of `north test mutant`; three skills document the bare `mutant record` command; delete when nixos-config provides the wrapper |
| bin/nix | 14 | stay | same admission shadow for nix |
| bin/north | 15 | stay | launcher: execs the promoted runtime (selected outside the Nix closure); cannot be Rust |
| bin/pace-control | 3 | stay | 3-line exec of `north fleet pace sample`; nixos-config modules/pace-enforcement/default.nix:39 names ${northBin}/pace-control, so the nixos-config change lands before deletion |
| bin/safe-push | 2452 | port: landing-train B18 (north#76, north#88 land-check loop box) | a planner is decomposing it; do not edit |
| bin/safe-push.test.sh | 2844 | port: landing-train B18 (north#76) | shell tests of the shell landing path; they move to cargo test with the train loop |
| bin/supply-alert | 7 | stay | 7-line glue: one ssh exec to the nexus alert host; callers in Rust (supply_chain.rs, lane gc) and the nixos-config supply-chain-scan unit |
| bin/verify-north | 9 | stay | launcher: execs the promoted verify-north runtime binary |
| harness/scripts/canary.sh | 55 | stay | 55-line CI glue that runs a family's own check on a planted fault; must run in CI before or beside the build, reads workflow YAML |
| harness/scripts/cargo-tests.sh | 6 | stay | 6-line CI glue that builds test binaries for the offline land check |
| harness/scripts/mutant-tests.sh | 57 | stay | 57-line CI glue that runs the mutant cargo slots |
| harness/scripts/read-isolation-probe.sh | 51 | stay | 51-line manual probe that drives Claude Code tools under a config; no Rust caller, delete if unused for a month |
| harness/skills/private-desktop-development/scripts/private-desktop.sh | 3 | stay | 3-line exec of `north desktop private`; named by lib/automation-chrome/cli.mjs and the desktop-action guard; delete when those name `north desktop private` |
| lib/automation-chrome/actions.mjs | 79 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/actions.test.mjs | 54 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/cli.mjs | 96 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/connect.mjs | 27 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/process.mjs | 7 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/service.mjs | 45 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/usage.mjs | 46 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
