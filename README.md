# North

- **Model**: the weights, text in and text out (Opus, Sonnet, Sol, Haiku).
- **Harness**: the machinery that turns a model into an agent: loop, tools, context and guardrails
  (Claude Code and Codex at the base; North adds hooks, skills, routing and policy checks).
- **Agent**: one model running in a harness, in a loop, toward a goal (a lead, a worker, the proxy).
- **Workforce**: many agents with roles, a chain of command, staffing, budgets and policy (Socrates;
  South holds its private policy).

An all-in-one agent product is a packaged harness with defaults and a persona, not a separate layer.
Socrates spans many repositories, public and private; its member repositories carry the GitHub topic
`socrates`. More: [docs/terms.md](docs/terms.md)

North is a Rust TUI for directing coding work, conversations, delegation, goals and recurring work.
Galileo owns its state transitions; the Rust host performs terminal, process and transport work.

North's additions to the harness are the agent commands under [bin/](bin/) and the hooks, skills
and policy under [harness/](harness/).

Both are for people who direct coding agents from one terminal, and for the agents that work in
this repository.

## Build and run

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so enter the dev shell first:

```sh
nix develop
```

Build, run and check commands for the host, for [agent-machinery/](agent-machinery/) and for
harness scripts are listed in [AGENTS.md](AGENTS.md); see AGENTS.md rather than copying them here.

## Where things live

- [src/](src/): the Rust host.
- [galileo/](galileo/): North's state transitions in Galileo.
- [agent-machinery/](agent-machinery/): provider-independent procedures and projections.
- [bin/](bin/): every harness agent command.
- [harness/](harness/): harness hooks, skills and policy owners.
- [docs/](docs/): design, features and feedback.

## Read next

- [AGENTS.md](AGENTS.md): rules for working in this repository.
- [docs/architecture.md](docs/architecture.md): the owning boundaries.
- [docs/features.md](docs/features.md): commands and the feature surface.
- [docs/feedback-loop.md](docs/feedback-loop.md): each recorded signal, its writer and its reader.
- [docs/references.md](docs/references.md): outside sources, what they shaped and what came of them.
- [agent-machinery/README.md](agent-machinery/README.md): the shared procedures package.
- [harness/skills/testing/SKILL.md](harness/skills/testing/SKILL.md): how tests are kept and run.
