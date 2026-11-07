# Non-Rust inventory

Every non-Rust code file left in North on 2026-11-08 for north#88 (the rows of files since ported or deleted are dropped; git history has them): shell, Python, TypeScript and JavaScript files, and shebang scripts in `bin/` and `harness/`. Tom asked on 2026-11-04 that any non-Rust code have an extremely strong reason to exist; each row below names the port box on north#88 or the reason the file stays. Data (Markdown, TOML, JSON, YAML, fixtures, Nix, `flake.nix`, `flake.lock`) is not code and is not listed.

Rows say `port: ...` (a box on north#88, family named), `stay` (with the reason) or `delete`. A file that stays is either a launcher or PATH name with no Rust equivalent possible, glue of at most 80 code lines, or part of a Bun/Node package whose JS API or JS-only dependency is the point. Where a Rust twin exists and a deployed unit or another repository still names the file, the row names the caller whose change lands first.

## Inventory

| File | Lines | Disposition | Reason / note |
|---|---|---|---|
| agent-machinery/scripts/effect-kit.mjs | 322 | stay | drives the target project's own TypeScript compiler and Effect language service (createRequire into the project's node_modules); only JS can load them |
| bin/agy | 6 | stay | 6-line launcher for the downloaded agy runtime (exports env then execs a vendor binary) |
| bin/automation-chrome | 9 | stay | 9-line flock wrapper that execs the Playwright CLI in lib/automation-chrome |
| bin/automation-chrome.test.sh | 9 | stay | test of the Playwright wrapper |
| bin/bun | 17 | stay | PATH shadow that routes bun through `north capacity gate` admission, then execs the real bun; must be an executable named bun (<=80-line glue) |
| bin/cargo | 17 | stay | same admission shadow for cargo |
| bin/cargo-nextest | 17 | stay | same admission shadow for cargo-nextest |
| bin/depbox | 79 | stay | execs bwrap/unshare sandbox around an arbitrary command (namespace setup before any runtime exists); glue under 80 code lines |
| bin/effect-kit | 4 | stay | 4-line PATH name for agent-machinery/scripts/effect-kit.mjs |
| bin/gh | 19 | stay | 19-line PATH shim: finds the real gh and runs it through `north gh-session` in a fleet session; glue |
| bin/gh-as-app | 4 | stay | 4-line glue: runs gh with the token printed by gh-app-token |
| bin/head-north | 105 | stay | bootstrap: builds, caches per tree and execs the north binary that every land check in `.safe-push` runs, so it cannot be that north; over 80 lines because of the per-tree cache and its lease pruning, which the Rust it launches cannot own |
| bin/land-shell | 16 | stay | workflow-free 16-line wrapper that runs a land-check command inside depbox (and nix develop with --dev); used by .safe-push, bin/head-north and red_check.rs; glue |
| bin/mutant | 3 | stay | 3-line exec of `north test mutant`; three skills document the bare `mutant record` command; delete when nixos-config provides the wrapper |
| bin/nix | 17 | stay | same admission shadow for nix |
| bin/north | 15 | stay | launcher: execs the promoted runtime (selected outside the Nix closure); cannot be Rust |
| bin/pace-control | 3 | stay | 3-line exec of `north fleet pace sample`; nixos-config modules/pace-enforcement/default.nix:39 names ${northBin}/pace-control, so the nixos-config change lands before deletion |
| bin/rustc | 17 | stay | same admission shadow for rustc |
| bin/ssh | 15 | stay | 15-line PATH shim: finds the real ssh and runs it through `north ssh-session` in a fleet session; glue |
| bin/subsystem-fix-commits | 25 | stay | 25-line git blame, awk and sqlite3 report that marks fix commits for docs/rationale/subsystem-ledger.md; glue over three tools with no Rust consumer |
| bin/subsystem-ledger | 47 | stay | 47-line git, awk and sqlite3 report behind docs/rationale/subsystem-ledger.md; glue over the same tools, no Rust consumer |
| bin/supply-alert | 7 | stay | 7-line glue: one ssh exec to the nexus alert host; callers in Rust (supply_chain.rs, lane gc) and the nixos-config supply-chain-scan unit |
| bin/verify-north | 9 | stay | launcher: execs the promoted verify-north runtime binary |
| harness/scripts/canary.sh | 57 | stay | 57-line CI glue that runs a family's own check on a planted fault; must run in CI before or beside the build, reads workflow YAML |
| harness/scripts/cargo-tests.sh | 7 | stay | 7-line CI glue that builds test binaries for the offline land check |
| harness/scripts/mutant-tests.sh | 62 | stay | 62-line CI glue that runs the mutant cargo slots |
| harness/scripts/read-isolation-probe.sh | 51 | stay | 51-line manual probe that drives Claude Code tools under a config; no Rust caller, delete if unused for a month |
| harness/skills/private-desktop-development/scripts/private-desktop.sh | 3 | stay | 3-line exec of `north desktop private`; named by lib/automation-chrome/cli.mjs and the desktop-action guard; delete when those name `north desktop private` |
| harness/split/cut-plan.sh | 47 | stay | 47-line git, rg and awk planner that prices a crate split (churn, rebuilt sets, DAG breaks); manual design aid with no Rust caller, delete when the split plans are done |
| lib/automation-chrome/actions.mjs | 79 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/actions.test.mjs | 54 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/cli.mjs | 96 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/connect.mjs | 27 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/process.mjs | 7 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/service.mjs | 45 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/usage.mjs | 46 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
