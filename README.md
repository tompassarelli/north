# North

North is a Rust TUI for directing coding work, conversations, delegation, goals and recurring work.
Clause owns its state transitions; the Rust host performs terminal, process and transport work.

Socrates is the agent machinery that runs alongside it: the agent commands under [bin/](bin/) and
the hooks, skills and policy under [socrates/](socrates/).

Both are for people who direct coding agents from one terminal, and for the agents that work in
this repository.

## Build and run

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so enter the dev shell first:

```sh
nix develop
```

Build, run and check commands for the host, for [agent-machinery/](agent-machinery/) and for
Socrates scripts are listed in [AGENTS.md](AGENTS.md); see AGENTS.md rather than copying them here.

## Where things live

- [src/](src/): the Rust host.
- [clause/](clause/): North's state transitions in Clause.
- [agent-machinery/](agent-machinery/): provider-independent procedures and projections.
- [bin/](bin/): every Socrates agent command.
- [socrates/](socrates/): Socrates hooks, skills and policy owners.
- [docs/](docs/): design, features and feedback.

## Read next

- [AGENTS.md](AGENTS.md): rules for working in this repository.
- [docs/architecture.md](docs/architecture.md): the owning boundaries.
- [docs/features.md](docs/features.md): commands and the feature surface.
- [docs/feedback-loop.md](docs/feedback-loop.md): each recorded signal, its writer and its reader.
- [docs/references.md](docs/references.md): outside sources, what they shaped and what came of them.
- [agent-machinery/README.md](agent-machinery/README.md): the shared procedures package.
- [socrates/skills/testing/SKILL.md](socrates/skills/testing/SKILL.md): how tests are kept and run.
