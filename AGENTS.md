profile: tooling

# North

North is a Rust TUI for directing coding work, conversations, explicit delegation, goals and recurring work.
Clause owns North's state transitions, the Rust host performs terminal, process and transport work, and
Codex app-server owns provider communication.

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so run Cargo through `nix develop`.
Build output stays in the worktree's `target/`.

## Rules

- Put North state transitions in `clause/north.clause`; Rust performs only authorized effects and renders projections.
- Follow [testing](socrates/skills/testing/SKILL.md) for test retention, mutation evidence and execution.
- Check with `depbox run -- nix develop -c cargo test --locked --offline` for the host, and
  `depbox run -- bun test` plus `depbox run -- bun run check` inside `agent-machinery/` for its procedures.
- Edit in `worktrees/<slug>`, never `main/`, and land through `safe-push --to main`.
- Run the affected `*.test.sh` or `*.test.py` under `bin/`, `lib/` or `socrates/` for a Socrates change; the `socrates.yml` workflow runs them all except its listed machine-bound ones.
- Public and reusable goes to north or firn; Tom-specific or secret goes to south.

## Routers

- `docs/architecture.md` for the owning boundaries.
- `docs/features.md` for commands and the feature surface.
- `agent-machinery/README.md` for the provider-independent procedures and projections.
- `bin/` holds every Socrates agent command, and `socrates/` holds its hooks, skills and policy owners.
