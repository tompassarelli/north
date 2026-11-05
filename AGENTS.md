# North

North is the harness Socrates runs on: the agent commands in `bin/`, the hooks, skills and policy in
`harness/`, and the `north` TUI, whose state transitions are typed Rust. `README.md` says what it is
and how to try it; this file holds the rules for working in it.

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so run Cargo through `nix develop`.
Build output stays in the worktree's `target/`.

## Rules

- Use the four layer terms of `docs/terms.md`, one per layer: model, harness, agent, workforce. North is the harness and Socrates the workforce (South holds its private policy); never call a model an agent.
- Keep fleet screen state and rendering in `crates/north-tui/src/tui/fleet`; the Rust host performs authorized effects and renders projections.

- Change Rust build settings (profiles, linker, caches, crate splits) only after
  measuring them per [compile-times](agent-machinery/skills/rust-development/references/compile-times.md).
- Follow [verification](agent-machinery/skills/verification/SKILL.md) for test retention, mutation evidence and execution.
- Check with `depbox run -- nix develop -c cargo test --locked --offline` and `depbox run -- nix develop -c cargo clippy --all-targets --locked --offline -- -D warnings` (the land check refuses any warning) for the host, and
  `depbox run -- bun test` inside `agent-machinery/` plus `north config check` for its procedures.
- Run `verify-north` in your worktree before claiming a north change works; it builds north, drives the fleet TUI and CLI end to end, and cleans up.
- Edit in `worktrees/<slug>`, never `main/`, and land through `safe-push --to main`.
- Run the affected `*.test.sh` or `*.test.py` under `bin/`, `lib/` or `harness/` for a harness change; the `harness.yml` workflow runs them all except its listed machine-bound ones.
- Public and reusable goes to north or firn; Tom-specific or secret goes to south.

## Routers

- Before changing `bin/safe-push`, `.safe-push` or `crates/north-landing/src/landing/mod.rs`, read [docs/harness.md](docs/harness.md#landing-a-change) for how a lane reaches main.
- Before changing `crates/north-fleet/src/fleet/mod.rs` or `crates/north-tui/src/tui/fleet/mod.rs`, read [docs/architecture.md](docs/architecture.md#owning-boundary) and the [fleet sessions](docs/features.md#fleet-sessions) features.
- Before changing `crates/north-threads/src/threads/staffing/mod.rs` or `crates/north-pace/src/router.rs`, read [docs/harness.md](docs/harness.md#staffing-a-worker) and the [staffing and routing](docs/features.md#staffing-and-routing) features.
- Before changing `crates/north-core/src/capacity/admission.rs` or a tuned number in `harness/defaults.toml`, read the [machine-capacity skill](harness/skills/machine-capacity/SKILL.md) and the [capacity](docs/features.md#machines-capacity-and-scheduled-jobs) features.
- Before changing `harness/policy-owners.toml`, the hook `harness/hooks/north-hook.sh` or a skill per `harness/SKILL-ARCHITECTURE.md`, read [docs/harness.md](docs/harness.md#where-policy-lives) and the [agent-policy skill](harness/skills/agent-policy/SKILL.md).
- Before changing the policy gate in `crates/north-core/src/agent_config_check/gate.rs` or `agents sync`, read [docs/harness.md](docs/harness.md#install) for how policy is published.
- Before changing `crates/north-core/src/grounding/sources/repos.rs` or `harness/hooks/prompt-grounding.sh`, read the [policy measurement](docs/features.md#policy-measurement) features, since docs lines feed that index.
- Before changing what `crates/north-threads/src/threads/runs/record.rs` or `crates/north-core/src/ledger/mod.rs` records, read [docs/feedback-loop.md](docs/feedback-loop.md) for each signal and the reader that acts on it.
- Before adding or changing a command, add its entry under `docs/features/` and run `north docs features-index`, because [docs/features.md](docs/features.md) is generated and never edited by hand.
- Before changing provider-independent doctrine, contracts or procedures, read [agent-machinery/README.md](agent-machinery/README.md); [docs/terms.md](docs/terms.md) names the layers.
