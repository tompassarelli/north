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
| bin/capacity-watchdog | 549 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | Bun TS using bun:sqlite (rusqlite replaces it) and reading handoff_ctx from harness/defaults.toml; no Bun-only API that Rust lacks |
| bin/cargo | 14 | stay | same admission shadow for cargo |
| bin/code-health | 3 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | shim to north code-health; deleted once no host unit names it |
| bin/code-health-tick | 3 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | shim to north code-health tick; nixos-config modules/code-health-review/default.nix names it, so the unit change lands first |
| bin/depbox | 88 | stay | execs bwrap/unshare sandbox around an arbitrary command (namespace setup before any runtime exists); glue under 80 code lines |
| bin/effect-kit | 4 | stay | 4-line PATH name for agent-machinery/scripts/effect-kit.mjs |
| bin/follows-release-gates.test.sh | 59 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/follows-release.lib.sh | 58 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/follows-release-staffed.test.sh | 40 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/follows-release.test.sh | 33 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/gh-app-token | 90 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | Bun TS signing an RS256 JWT with node:crypto; Rust has rsa/jsonwebtoken |
| bin/gh-app-token.test.sh | 93 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | test of gh-app-token |
| bin/gh-as-app | 4 | stay | 4-line glue: runs gh with the token printed by gh-app-token |
| bin/gh | 19 | stay | 19-line PATH shim: finds the real gh and runs it through `north gh-session` in a fleet session; glue |
| bin/ssh | 15 | stay | 15-line PATH shim: finds the real ssh and runs it through `north ssh-session` in a fleet session; glue |
| bin/job-run | 86 | port: north#88 box "run-bounded and job-run" (family: landing path, north#83) | ported as `north job-run`; the script stays until the nixos-config units switch to the verb on every host (north#88 box "run-bounded and job-run"); deployed systemd units name bin/job-run (nixos-config modules/job-run, delivery-liveness, code-health-review, worker-ledger), so the nixos-config unit change lands first; tests/job_run.rs drives it |
| bin/land-shell | 11 | stay | workflow-free 10-line wrapper that runs a land-check command inside depbox (and nix develop with --dev); used by .safe-push, bin/head-north and red_check.rs; glue |
| bin/mutant | 3 | stay | 3-line exec of `north mutant`; three skills document the bare `mutant record` command; delete when nixos-config provides the wrapper |
| bin/nix | 14 | stay | same admission shadow for nix |
| bin/north | 15 | stay | launcher: execs the promoted runtime (selected outside the Nix closure); cannot be Rust |
| bin/pace-control | 3 | stay | 3-line exec of `north pace sample`; nixos-config modules/pace-enforcement/default.nix:39 names ${northBin}/pace-control, so the nixos-config change lands before deletion |
| bin/safe-push | 2452 | port: landing-train B18 (north#76, north#88 land-check loop box) | a planner is decomposing it; do not edit |
| bin/safe-push.test.sh | 2844 | port: landing-train B18 (north#76) | shell tests of the shell landing path; they move to cargo test with the train loop |
| bin/supply-alert | 7 | stay | 7-line glue: one ssh exec to the nexus alert host; callers in Rust (supply_chain.rs, lane-gc) and the nixos-config supply-chain-scan unit |
| bin/verify-north | 9 | stay | launcher: execs the promoted verify-north runtime binary |
| harness/hooks/firn-system-policy | 42 | stay | 42-line hook owned by Firn and installed from this tree; belongs in nixos-config, move there rather than port |
| harness/hooks/language-guard.sh | 10 | stay | 10-line hook wrapper that execs `north guard language`; same condition |
| harness/hooks/north-hook.sh | 9 | stay | 9-line hook wrapper that execs `north hook`; same condition |
| harness/hooks/prompt-grounding-membership.test.sh | 29 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests the Rust grounding hook through its wrapper |
| harness/hooks/prompt-grounding.sh | 32 | stay | 32-line hook wrapper that skips fleet sessions then calls `north grounding hook`; deleted with authoring-killswitch.sh when hook configs name `north hook` directly |
| harness/hooks/prompt-grounding.test.sh | 94 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests the Rust grounding hook through its wrapper |
| harness/lib/authoring-killswitch.sh | 41 | port: north#88 box "hook and harness shell to Rust" (family: tests) | sourced by hook wrappers to read kill switches; goes when provider hook configs name `north hook` directly (nixos-config change first) |
| harness/lib/authoring-killswitch.test.sh | 24 | port: north#88 box "hook and harness shell to Rust" (family: tests) | test of authoring-killswitch.sh |
| harness/lib/north-agent-activation.sh | 83 | port: north#88 box "hook and harness shell to Rust" (family: tests) | sourced activation reader; Rust reads the same generation |
| harness/scripts/canary.sh | 55 | stay | 55-line CI glue that runs a family's own check on a planted fault; must run in CI before or beside the build, reads workflow YAML |
| harness/scripts/cargo-tests.sh | 6 | stay | 6-line CI glue that builds test binaries for the offline land check |
| harness/scripts/mutant-tests.sh | 57 | stay | 57-line CI glue that runs the mutant cargo slots |
| harness/scripts/read-isolation-probe.sh | 51 | stay | 51-line manual probe that drives Claude Code tools under a config; no Rust caller, delete if unused for a month |
| harness/skills/private-desktop-development/scripts/private-desktop.sh | 3 | stay | 3-line exec of `north private-desktop`; named by lib/automation-chrome/cli.mjs and the desktop-action guard; delete when those name `north private-desktop` |
| lib/automation-chrome/actions.mjs | 79 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/actions.test.mjs | 54 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/cli.mjs | 96 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/connect.mjs | 27 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/process.mjs | 7 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/service.mjs | 45 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/usage.mjs | 46 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
