# Non-Rust inventory

Every non-Rust code file in North, written 2026-11-04 for north#88: shell, Python, TypeScript and JavaScript files, and shebang scripts in `bin/` and `harness/`. Tom asked on 2026-11-04 that any non-Rust code have an extremely strong reason to exist; each row below names the port box on north#88 or the reason the file stays. Data (Markdown, TOML, JSON, YAML, fixtures, Nix, `flake.nix`, `flake.lock`) is not code and is not listed.

Rows say `port: ...` (a box on north#88, family named), `stay` (with the reason) or `delete`. A file that stays is either a launcher or PATH name with no Rust equivalent possible, glue of at most 80 code lines, or part of a Bun/Node package whose JS API or JS-only dependency is the point. Where a Rust twin exists and a deployed unit or another repository still names the file, the row names the caller whose change lands first.

## Deleted in this change

Each name was searched across north, south and nixos-config (Markdown, decision records and ledgers excluded) before deletion.

| File | Lines | Why it is safe |
|---|---|---|
| bin/worker-ledger | 3 | transient shim for the worker-ledger unit; `systemctl --user cat worker-ledger.service` on whiterabbit, nexus and gex45 on 2026-11-04 shows ExecStart `north worker-ledger` (nixos-config `modules/worker-ledger/default.nix:43`) |
| bin/danger | 4 | no caller in north, south or nixos-config; the rebuild craftsmanship audit (south) already marked it delete (W15) |
| bin/farm-runner, bin/farm-runner-image, bin/farm-runner.test.sh | 101, 19, 42 | no caller in north, south or nixos-config; the deployed farm runners are nixos-config `modules/farm-runners` (its own image build and gate), and `north laptop-runner` replaced the laptop script |
| bin/iteration-ledger, bin/iteration-ledger-page | 458, 56 | deleted 2026-11-05 instead of ported: its only caller was the Feature Bottlenecks refresh steps in harness/skills/workers/references/payback.md, last run 2026-10-15, and every Tom-ask input it read stopped then (last proxy KICKOFF and LAND-READY events in the threads snapshot 2026-10-15, land-checks.tsv 2026-10-12, handoffs/throughput-architect-measure.md 2026-10-15); payback rates now come from `git log --first-parent` |

## Deleted for the north#88 hook and harness shell box (2026-11-05)

Searched with `rg -l NAME` across north, south and nixos-config, decision records excluded.

| File | Lines | Why it is safe |
|---|---|---|
| harness/lib/tree-north.sh, harness/lib/tree-north.test.sh | 18, 45 | nothing sources it; the only other hits were its own test and the harness-tests CI canary, which now breaks `bin/gh-app-token` instead (`tests/it/land_checks.rs` uses `tree-north` only as a stub trace label) |
| harness/hooks/lib/bench.sh | 100 | no caller; nixos-config `bench-shield` is an unrelated core-reservation tool |
| harness/scripts/rust-build-timings.sh, harness/scripts/rust-build-timings.test.sh | 47, 28 | only the rust-development compile-times reference named it, and `north code-health build-time` already times a cold build, edit rebuilds and test builds in a fresh target directory; the reference now names that verb |

## north#88 part (a) outcome

- Already gone from `origin/main` before this change: dep-gate, provenance, convo, flow, report (Rust: `north dep-gate`, `provenance`, `convo`, `flow`, `report`), and the old `bin/follows-release` script itself.
- Still in the tree, with the live caller named:
  - `bin/follows-release.lib.sh` and its three `.test.sh` files drive `bin/safe-push`, which calls the Rust `north follows-release`; they leave with the safe-push port (landing-train B18).
  - `bin/job-run` is ported as `north job-run` but stays until the nixos-config units that name it switch to the verb on every host; `bin/run-bounded` is ported as `north run-bounded` and deleted.

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
| bin/job-run | 86 | port: north#88 box "run-bounded and job-run" (family: landing path, north#83) | ported as `north job-run`; the script stays until the nixos-config units switch to the verb on every host (north#88 box "run-bounded and job-run"); deployed systemd units name bin/job-run (nixos-config modules/job-run, delivery-liveness, code-health-review, worker-ledger), so the nixos-config unit change lands first; tests/job_run.rs drives it |
| bin/land-shell | 11 | stay | workflow-free 10-line wrapper that runs a land-check command inside depbox (and nix develop with --dev); used by .safe-push, bin/head-north and red_check.rs; glue |
| bin/mutant | 3 | stay | 3-line exec of `north mutant`; three skills document the bare `mutant record` command; delete when nixos-config provides the wrapper |
| bin/nix | 14 | stay | same admission shadow for nix |
| bin/north | 15 | stay | launcher: execs the promoted runtime (selected outside the Nix closure); cannot be Rust |
| bin/pace-control | 3 | stay | 3-line exec of `north pace sample`; nixos-config modules/pace-enforcement/default.nix:39 names ${northBin}/pace-control, so the nixos-config change lands before deletion |
| bin/safe-push | 2452 | port: landing-train B18 (north#76, north#88 land-check loop box) | a planner is decomposing it; do not edit |
| bin/safe-push.test.sh | 2844 | port: landing-train B18 (north#76) | shell tests of the shell landing path; they move to cargo test with the train loop |
| bin/supply-alert | 7 | stay | 7-line glue: one ssh exec to the nexus alert host; callers in Rust (supply_chain.rs, lane-gc) and the nixos-config supply-chain-scan unit |
| bin/threads | 789 | port: north#77 (bin/threads) | 789 lines of Python; verbs already move to Rust (RUST_VERBS) |
| bin/threads-resume.test.sh | 80 | port: north#77 (bin/threads) | test of bin/threads |
| bin/threads.test.sh | 684 | port: north#77 (bin/threads) | test of bin/threads |
| bin/verify-north | 9 | stay | launcher: execs the promoted verify-north runtime binary |
| harness/hooks/firn-system-policy | 42 | stay | 42-line hook owned by Firn and installed from this tree; belongs in nixos-config, move there rather than port |
| harness/hooks/language-guard.sh | 10 | stay | 10-line hook wrapper that execs `north guard language`; same condition |
| harness/hooks/north-hook.sh | 9 | stay | 9-line hook wrapper that execs `north hook`; same condition |
| harness/hooks/prompt-grounding-membership.test.sh | 29 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests the Rust grounding hook through its wrapper |
| harness/hooks/prompt-grounding.sh | 32 | stay | 32-line hook wrapper that skips fleet sessions then calls `north grounding hook`; deleted with authoring-killswitch.sh when hook configs name `north hook` directly |
| harness/hooks/prompt-grounding.test.sh | 94 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests the Rust grounding hook through its wrapper |
| harness/lib/authoring-killswitch.sh | 41 | port: north#88 box "hook and harness shell to Rust" (family: tests) | sourced by hook wrappers to read kill switches; goes when provider hook configs name `north hook` directly (nixos-config change first) |
| harness/lib/authoring-killswitch.test.sh | 24 | port: north#88 box "hook and harness shell to Rust" (family: tests) | test of authoring-killswitch.sh |
| harness/lib/billing_digitalocean.py | 54 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/calibration.py | 103 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/claude_profiles.py | 91 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/claude_profiles.test.py | 100 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/defaults.py | 12 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/durable_events.py | 108 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/durable-events.test.py | 166 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/goal_eta.py | 259 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/goal_eta.test.py | 64 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/ledger_context.py | 65 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/north-agent-activation.sh | 83 | port: north#88 box "hook and harness shell to Rust" (family: tests) | sourced activation reader; Rust reads the same generation |
| harness/lib/plan_usage.py | 213 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/presence.py | 301 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/presence.test.py | 221 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/project_plan.py | 88 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/project_score.py | 118 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/project_score.test.py | 74 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/provider_capacity.py | 103 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/routines.py | 238 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/ssh_transport.py | 19 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage_agy.py | 179 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage_claude.py | 137 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage.py | 409 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage.test.sh | 59 | port: north#78 (usage family) | test of usage.py; cases move to cargo test with --refresh/--reservation-json |
| harness/scripts/canary.sh | 55 | stay | 55-line CI glue that runs a family's own check on a planted fault; must run in CI before or beside the build, reads workflow YAML |
| harness/scripts/cargo-tests.sh | 6 | stay | 6-line CI glue that builds test binaries for the offline land check |
| harness/scripts/mutant-tests.sh | 57 | stay | 57-line CI glue that runs the mutant cargo slots |
| harness/scripts/read-isolation-probe.sh | 51 | stay | 51-line manual probe that drives Claude Code tools under a config; no Rust caller, delete if unused for a month |
| harness/skills/machine-capacity/scripts/machine-capacity.test.sh | 578 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | shell tests of the capacity commands |
| harness/skills/private-desktop-development/scripts/private-desktop.sh | 3 | stay | 3-line exec of `north private-desktop`; named by lib/automation-chrome/cli.mjs and the desktop-action guard; delete when those name `north private-desktop` |
| lib/automation-chrome/actions.mjs | 79 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/actions.test.mjs | 54 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/cli.mjs | 96 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/connect.mjs | 27 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/process.mjs | 7 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/service.mjs | 45 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/usage.mjs | 46 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
