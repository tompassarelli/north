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

## north#88 part (a) outcome

- Already gone from `origin/main` before this change: dep-gate, provenance, convo, flow, report (Rust: `north dep-gate`, `provenance`, `convo`, `flow`, `report`), and the old `bin/follows-release` script itself.
- Still in the tree, with the live caller named:
  - `bin/follows-release.lib.sh` and its three `.test.sh` files drive `bin/safe-push`, which calls the Rust `north follows-release`; they leave with the safe-push port (landing-train B18).
  - `bin/agent-instruction-check` has a Rust twin (`instruction_limits.rs`) but `bin/safe-push:437` and `crates/north-core/src/agent_config_check/shared.rs:142` still call the script.
  - `bin/run-bounded` and `bin/job-run` were never ported, despite the north#83 tick. Deployed units name `job-run`, and the session-kill guard rewrites agent commands to `run-bounded`.
  - `bin/agents` is the Firn-stable PATH name; the deployed worker-ledger.service ExecStartPost runs it.

## Inventory

| File | Lines | Disposition | Reason / note |
|---|---|---|---|
| agent-machinery/index.mjs | 42 | stay | entry of the published @tompassarelli/agent-machinery npm package, whose JS API is the product for outside consumers |
| agent-machinery/scripts/contracts.mjs | 29 | stay | exported contract schemas of the published package |
| agent-machinery/scripts/effect-kit.mjs | 336 | stay | drives the target project's own TypeScript compiler and Effect language service (createRequire into the project's node_modules); only JS can load them |
| agent-machinery/scripts/project-exposure-profile.mjs | 128 | stay | exported profile API of the published package (project-exposure-v1) |
| agent-machinery/scripts/refresh-guides.mjs | 63 | port: north#88 box "agent-machinery checks to Rust" (family: tooling, audit 4.3) | repo-internal guide refresher; no JS consumer needs it |
| agent-machinery/scripts/validate.mjs | 126 | port: north#88 box "agent-machinery checks to Rust" (family: tooling, audit 4.3) | repo-internal ajv validation of catalog and contracts; no JS consumer needs it, so `north config check` can own it |
| agent-machinery/tests/capability-contract.test.mjs | 19 | stay | bun tests of the published package modules above |
| agent-machinery/tests/contracts.test.mjs | 37 | stay | bun tests of the published package modules above |
| agent-machinery/tests/package-boundary.test.mjs | 24 | stay | bun tests of the published package modules above |
| bin/agent-capture | 60 | stay | 60-line glue over niri msg and grim; foreign desktop tools, no Rust library for the niri IPC screenshot path; policy names it as the only allowed capture |
| bin/agent-instruction-check | 56 | port: landing-train B18 (north#76) | Rust twin exists (crates/north-landing/src/landing/instruction_limits.rs); live callers: bin/safe-push:437 and crates/north-core/src/agent_config_check/shared.rs:142; delete when both call Rust |
| bin/agent-instruction-check.test.sh | 56 | port: landing-train B18 (north#76) | tests the shell helper that bin/safe-push calls |
| bin/agents | 3 | stay | 3-line exec of `north agents`; Firn keeps this stable PATH name and the deployed worker-ledger.service ExecStartPost names bin/agents (north#82 box); delete only after nixos-config provides an `agents` wrapper |
| bin/agy | 6 | stay | 6-line launcher for the downloaded agy runtime (exports env then execs a vendor binary) |
| bin/agy-runtime-update | 52 | port: north#87 (Codex/runtime/desktop tools) | download and select the agy runtime |
| bin/automation-chrome | 9 | stay | 9-line flock wrapper that execs the Playwright CLI in lib/automation-chrome |
| bin/automation-chrome.test.sh | 9 | stay | test of the Playwright wrapper |
| bin/bun | 14 | stay | PATH shadow that routes bun through `north capacity gate` admission, then execs the real bun; must be an executable named bun (<=80-line glue) |
| bin/capacity-watchdog | 549 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | Bun TS using bun:sqlite (rusqlite replaces it) and importing machine-capacity-logic.ts; no Bun-only API that Rust lacks |
| bin/capacity-watchdog.test.sh | 175 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | test of capacity-watchdog |
| bin/cargo | 14 | stay | same admission shadow for cargo |
| bin/ci-diagnose | 8 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | shim to harness/scripts/ci-diagnose.ts |
| bin/claude-runtime-update | 49 | port: north#87 (Codex/runtime/desktop tools) | downloads and selects the Claude runtime |
| bin/claude-runtime-update.test.sh | 63 | port: north#87 (Codex/runtime/desktop tools) | test of claude-runtime-update |
| bin/code-health | 7 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | 9-line shim to harness/code-health/cli.ts |
| bin/code-health.test.sh | 4 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | 4-line test runner for the code-health TS tests |
| bin/code-health-tick | 5 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | shim to harness/code-health/tick.ts; nixos-config modules/code-health-review/default.nix names it, so the unit change lands first |
| bin/codex-runtime-update | 159 | port: north#87 (Codex/runtime/desktop tools) | 159 lines, over the 80-line glue limit |
| bin/codex-runtime-update.test.sh | 149 | port: north#87 (Codex/runtime/desktop tools) | test of codex-runtime-update |
| bin/depbox | 88 | stay | execs bwrap/unshare sandbox around an arbitrary command (namespace setup before any runtime exists); glue under 80 code lines |
| bin/effect-kit | 4 | stay | 4-line PATH name for agent-machinery/scripts/effect-kit.mjs |
| bin/follows-release-gates.test.sh | 59 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/follows-release.lib.sh | 58 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/follows-release-staffed.test.sh | 40 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/follows-release.test.sh | 33 | port: landing-train B18 (north#76) | drive bin/safe-push against the Rust `north follows-release`; the tool itself is ported and its old script is gone; delete with safe-push.test.sh |
| bin/gh-app-token | 90 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | Bun TS signing an RS256 JWT with node:crypto; Rust has rsa/jsonwebtoken |
| bin/gh-app-token.test.sh | 93 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | test of gh-app-token |
| bin/gh-as-app | 4 | stay | 4-line glue: runs gh with the token printed by gh-app-token |
| bin/iteration-ledger | 458 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | Bun TS with bun:sqlite |
| bin/iteration-ledger-page | 56 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | Bun script rendering a static page |
| bin/job-run | 86 | port: north#88 box "run-bounded and job-run" (family: landing path, north#83) | never ported; deployed systemd units name bin/job-run (nixos-config modules/job-run, delivery-liveness, code-health-review, worker-ledger), so the nixos-config unit change lands first; tests/job_run.rs drives it |
| bin/land-shell | 10 | stay | workflow-free 10-line wrapper that runs a land-check command inside depbox (and nix develop with --dev); used by .safe-push, land-checks.sh and red_check.rs; glue |
| bin/mutant | 3 | stay | 3-line exec of `north mutant`; three skills document the bare `mutant record` command; delete when nixos-config provides the wrapper |
| bin/nix | 14 | stay | same admission shadow for nix |
| bin/north | 15 | stay | launcher: execs the promoted runtime (selected outside the Nix closure); cannot be Rust |
| bin/pace-control | 3 | stay | 3-line exec of `north pace sample`; nixos-config modules/pace-enforcement/default.nix:39 names ${northBin}/pace-control, so the nixos-config change lands before deletion |
| bin/red-main-notify | 40 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | called by `north flow`; state read by calibration/rework.rs |
| bin/red-main-notify.check.sh | 27 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | wrapped by tests/red_main_notify.rs; becomes a cargo test |
| bin/run-bounded | 79 | port: north#88 box "run-bounded and job-run" (family: landing path, north#83) | never ported despite the north#83 tick; PATH name rewritten into agent commands by session-kill guard and named by deployed units; port as `north run-bounded` with the PATH name provided by nixos-config |
| bin/run-bounded.test.sh | 75 | port: north#88 box "run-bounded and job-run" (family: landing path, north#83) | machine-bound test (harness/machine-bound-tests) |
| bin/safe-push | 2452 | port: landing-train B18 (north#76, north#88 land-check loop box) | a planner is decomposing it; do not edit |
| bin/safe-push.test.sh | 2844 | port: landing-train B18 (north#76) | shell tests of the shell landing path; they move to cargo test with the train loop |
| bin/supply-alert | 7 | stay | 7-line glue: one ssh exec to the nexus alert host; callers in Rust (supply_chain.rs, lane-gc) and the nixos-config supply-chain-scan unit |
| bin/test-health | 143 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | Bun TS with test-health-core.ts |
| bin/threads | 789 | port: north#77 (bin/threads) | 789 lines of Python; verbs already move to Rust (RUST_VERBS) |
| bin/threads-resume.test.sh | 80 | port: north#77 (bin/threads) | test of bin/threads |
| bin/threads.test.sh | 684 | port: north#77 (bin/threads) | test of bin/threads |
| bin/tom-warcraft-close | 60 | port: north#87 (Codex/runtime/desktop tools) | closes Tom's Steam launch by walking /proc |
| bin/tom-warcraft-close.test.sh | 63 | port: north#87 (Codex/runtime/desktop tools) | test of tom-warcraft-close |
| bin/verify-north | 9 | stay | launcher: execs the promoted verify-north runtime binary |
| bin/watch-review | 65 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | monthly watch routine named by south agents/watch.toml |
| bin/watch-review.test.sh | 78 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | test of watch-review |
| bin/wc3-login-field | 34 | stay | 34-line foreign-boundary glue: types one SOPS field into the verified private login window through private-desktop.sh |
| harness/code-health/check.ts | 49 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/cli.ts | 87 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/code-health.test.ts | 133 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/config.ts | 130 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/files.ts | 103 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/functions.ts | 180 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/gate.ts | 143 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/graph.ts | 92 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/history.ts | 77 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/imports.ts | 16 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/metrics.test.ts | 20 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/report.ts | 176 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/signal.test.ts | 48 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/signal.ts | 44 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/state.ts | 38 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/tick.test.ts | 73 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/code-health/tick.ts | 154 | port: north#88 box "code-health to Rust" (family: tooling, north#126) | TypeScript on Bun; north#126 ports the import resolver, the box covers the rest |
| harness/hooks/firn-system-policy | 42 | stay | 42-line hook owned by Firn and installed from this tree; belongs in nixos-config, move there rather than port |
| harness/hooks/language-guard.sh | 10 | stay | 10-line hook wrapper that execs `north guard language`; same condition |
| harness/hooks/lib/bench.sh | 100 | port: north#88 box "hook and harness shell to Rust" (family: tests) | 100-line hook benchmark, over the 80-line limit; becomes a `north hook bench` or cargo bench |
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
| harness/lib/routing.test.sh | 83 | port: north#80 (placement, routing, calibration) | allowlisted; test of a routing bug fix pending port |
| harness/lib/ssh_transport.py | 19 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/test-health-core.ts | 284 | port: north#88 box "tokens and ledgers to Rust" (family: tooling, audit C10) | core of bin/test-health |
| harness/lib/tree-north.sh | 18 | port: north#88 box "hook and harness shell to Rust" (family: tests) | sourced library that builds a tree's north binary for shell tests; goes when the shell tests that source it go |
| harness/lib/tree-north.test.sh | 45 | port: north#88 box "hook and harness shell to Rust" (family: tests) | test of tree-north.sh |
| harness/lib/usage_agy.py | 179 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage_claude.py | 137 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage.py | 409 | port: in flight (three workers port harness/lib and hooks/lib Python) | do not touch; calibration.py and goal_eta.test.py are allowlisted under north#88 |
| harness/lib/usage.test.sh | 59 | port: north#78 (usage family) | test of usage.py; cases move to cargo test with --refresh/--reservation-json |
| harness/scripts/canary.sh | 55 | stay | 55-line CI glue that runs a family's own check on a planted fault; must run in CI before or beside the build, reads workflow YAML |
| harness/scripts/cargo-tests.sh | 6 | stay | 6-line CI glue that builds test binaries for the offline land check |
| harness/scripts/ci-diagnose.test.sh | 3 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | 3-line runner for the TS test |
| harness/scripts/ci-diagnose.test.ts | 37 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | Bun test of ci-diagnose.ts |
| harness/scripts/ci-diagnose.ts | 153 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | Bun TS over gh and claude calls |
| harness/scripts/land-checks-binary.test.sh | 35 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests of land-checks.sh selection; they move to cargo test |
| harness/scripts/land-checks-path.test.sh | 26 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests of land-checks.sh selection; they move to cargo test |
| harness/scripts/land-checks-select.test.sh | 36 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests of land-checks.sh selection; they move to cargo test |
| harness/scripts/land-checks.sh | 74 | stay | 74-line bootstrap that pins PATH and builds north before any `north` subcommand can run the land checks |
| harness/scripts/land-checks.test.sh | 83 | port: north#88 box "hook and harness shell to Rust" (family: tests) | tests of land-checks.sh selection; they move to cargo test |
| harness/scripts/main-red.test.sh | 12 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | runner for main-red.test.ts |
| harness/scripts/main-red.test.ts | 15 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | Bun test |
| harness/scripts/main-red.ts | 112 | port: north#88 box "CI watchers to Rust" (family: watchers, north#85) | Bun TS; run by workflow main-red.yml |
| harness/scripts/mutant-tests.sh | 57 | stay | 57-line CI glue that runs the mutant cargo slots |
| harness/scripts/read-isolation-probe.sh | 51 | stay | 51-line manual probe that drives Claude Code tools under a config; no Rust caller, delete if unused for a month |
| harness/scripts/rust-build-timings.sh | 47 | stay | 47-line cargo timing glue |
| harness/scripts/rust-build-timings.test.sh | 28 | port: north#88 box "hook and harness shell to Rust" (family: tests) | test of rust-build-timings.sh |
| harness/skills/machine-capacity/scripts/build-machine-capacity | 28 | stay | producer of the generated bundle (structure.toml); deleted with it |
| harness/skills/machine-capacity/scripts/capacity-clamp.recorded.test.ts | 63 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | recorded and replay tests of the TS logic; cases move to cargo test with fixtures |
| harness/skills/machine-capacity/scripts/capacity-estimate.replay.test.sh | 3 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | shell tests of the capacity commands |
| harness/skills/machine-capacity/scripts/capacity-estimate.replay.test.ts | 75 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | recorded and replay tests of the TS logic; cases move to cargo test with fixtures |
| harness/skills/machine-capacity/scripts/capacity-unseen-pressure.recorded.test.ts | 129 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | recorded and replay tests of the TS logic; cases move to cargo test with fixtures |
| harness/skills/machine-capacity/scripts/charged-resources.property.ts | 39 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | property test of the TS logic |
| harness/skills/machine-capacity/scripts/machine-capacity-host.mjs | 8 | stay | source of the generated bundle above; deleted with it |
| harness/skills/machine-capacity/scripts/machine-capacity-logic.ts | 301 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | policy defaults imported by capacity-watchdog and the recorded tests; Rust capacity module is the owner |
| harness/skills/machine-capacity/scripts/machine-capacity.mjs | 3 | stay | generated 3-line Bun bundle that spawns `north capacity host`; nixos-config dotfiles/bin/machine-update, firn-runtime-update and south hosts/gex45/configuration.nix:32 run it by path, so those callers switch to `north capacity host` first (nixos-config and south changes land first), then delete it and its producer |
| harness/skills/machine-capacity/scripts/machine-capacity.test.sh | 578 | port: north#88 box "capacity TypeScript to Rust" (family: capacity, audit C4) | shell tests of the capacity commands |
| harness/skills/private-desktop-development/scripts/private-desktop-capture.test.sh | 57 | port: north#88 box "shell tests of Rust commands to cargo test" (family: tests) | tests the Rust private-desktop launcher |
| harness/skills/private-desktop-development/scripts/private-desktop.sh | 3 | stay | 3-line exec of `north private-desktop`; named by lib/automation-chrome/cli.mjs, bin/wc3-login-field and the desktop-action guard; delete when those name `north private-desktop` |
| lib/automation-chrome/actions.mjs | 79 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/actions.test.mjs | 54 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/cli.mjs | 96 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/connect.mjs | 27 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/process.mjs | 7 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/service.mjs | 45 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| lib/automation-chrome/usage.mjs | 46 | stay | Playwright (playwright-core 1.63.0) browser automation; no maintained Rust Playwright, and CDP-only crates lack its auto-wait and context model; Bun package |
| tests/dev-shell.test.sh | 28 | port: north#88 box "shell tests of Rust commands to cargo test" (family: tests) | tests the dev shell entry through nix; becomes a cargo integration test |
