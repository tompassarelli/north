# North

North is the harness Socrates runs on: the agent commands in `bin/`, the hooks, skills and policy in
`harness/`, and the `north` TUI, whose state transitions are typed Rust. `README.md` says what it is
and how to try it; this file holds the rules for working in it.

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so run Cargo through `nix develop`.
Build output stays in the worktree's `target/`.

## Rules

- Use the four layer terms of `docs/terms.md`, one per layer: model, harness, agent, workforce. North is the harness and Socrates the workforce (South holds its private policy); never call a model an agent.
- Keep fleet screen state and rendering in `src/tui/fleet`; the Rust host performs authorized effects and renders projections.

- Change Rust build settings (profiles, linker, caches, crate splits) only after
  measuring them per [compile-times](agent-machinery/skills/rust-development/references/compile-times.md).
- Follow [verification](agent-machinery/skills/verification/SKILL.md) for test retention, mutation evidence and execution.
- Check with `depbox run -- nix develop -c cargo test --locked --offline` and `depbox run -- nix develop -c cargo clippy --all-targets --locked --offline -- -D warnings` (the land check refuses any warning) for the host, and
  `depbox run -- bun test` plus `depbox run -- bun run check` inside `agent-machinery/` for its procedures.
- Run `verify-north` in your worktree before claiming a north change works; it builds north, drives the fleet TUI and CLI end to end, and cleans up.
- Edit in `worktrees/<slug>`, never `main/`, and land through `safe-push --to main`.
- Run the affected `*.test.sh` or `*.test.py` under `bin/`, `lib/` or `harness/` for a harness change; the `harness.yml` workflow runs them all except its listed machine-bound ones.
- Public and reusable goes to north or firn; Tom-specific or secret goes to south.

## Routers

- `docs/harness.md` for install, where policy lives, staffing and landing.
- `docs/architecture.md` for the owning boundaries.
- `docs/features.md` for commands and the feature surface.
- `docs/feedback-loop.md` for each recorded signal, its writer and the reader that acts on it.
- `agent-machinery/README.md` for the provider-independent doctrine, contracts and procedures.
- `bin/` holds every harness agent command, and `harness/` holds its hooks, skills and policy owners.
