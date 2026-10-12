# North

- Model: the weights, text in and text out (Opus, Sonnet, Sol, Haiku).
- Harness: the machinery that turns a model into an agent: loop, tools, context and guardrails (Claude Code and Codex at the base; North adds hooks, skills, routing and policy checks).
- Agent: one model running in a harness, in a loop, toward a goal (a lead, a worker, the proxy).
- Workforce: many agents with roles, a chain of command, staffing, budgets and policy (Socrates; South holds its private policy).
- Use these four terms, one per layer; never call a model an agent. See `docs/terms.md`.

North is a Rust TUI for directing coding work, conversations, explicit delegation, goals and recurring work.
Galileo owns North's state transitions, the Rust host performs terminal, process and transport work, and
Codex app-server owns provider communication.

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so run Cargo through `nix develop`.
Build output stays in the worktree's `target/`.

## Rules

- Put North state transitions in `clause/north.gal`; Rust performs only authorized effects and renders projections.
- Follow [testing](harness/skills/testing/SKILL.md) for test retention, mutation evidence and execution.
- Check with `depbox run -- nix develop -c cargo test --locked --offline` for the host, and
  `depbox run -- bun test` plus `depbox run -- bun run check` inside `agent-machinery/` for its procedures.
- Edit in `worktrees/<slug>`, never `main/`, and land through `safe-push --to main`.
- Run the affected `*.test.sh` or `*.test.py` under `bin/`, `lib/` or `harness/` for a harness change; the `harness.yml` workflow runs them all except its listed machine-bound ones.
- Public and reusable goes to north or firn; Tom-specific or secret goes to south.

## Routers

- `docs/architecture.md` for the owning boundaries.
- `docs/features.md` for commands and the feature surface.
- `docs/feedback-loop.md` for each recorded signal, its writer and the reader that acts on it.
- `agent-machinery/README.md` for the provider-independent procedures and projections.
- `bin/` holds every harness agent command, and `harness/` holds its hooks, skills and policy owners.
