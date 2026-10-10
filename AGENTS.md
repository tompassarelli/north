profile: tooling

# North

North is a Rust TUI for directing coding work, conversations, explicit delegation, goals and recurring work.
Clause owns North's state transitions, the Rust host performs terminal, process and transport work, and
Codex app-server owns provider communication.

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so run Cargo through `nix develop`.
Build output stays in the worktree's `target/`.

## Rules

- Put North state transitions in `clause/north.clause`; Rust performs only authorized effects and renders projections.
- Check with `nix develop -c cargo test --offline` for the host, and `bun test` plus `bun run check` inside
  `agent-machinery/` for its procedures.
- Edit in `worktrees/<slug>`, never `main/`, and land through `safe-push --to main`.

## Routers

- `docs/architecture.md` for the owning boundaries.
- `docs/features.md` for commands and the feature surface.
- `agent-machinery/README.md` for the provider-independent procedures and projections.
