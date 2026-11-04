# North

North is the harness that Socrates, Tom's agent workforce, runs on. Claude Code and Codex are the
base harnesses; North adds what a workforce needs on top: staffing and routing of workers, a safe
landing path to main, hooks that guard each tool call, shared skills and the policy checks that keep
them honest. It also ships `north`, a terminal fleet screen for directing sessions and decisions.

It is built for Tom and the agents in Socrates. Anyone who directs coding agents can read it, try
the `north` app, or borrow a command, hook or skill. The full harness expects Tom's machines and his
private policy repository, South, so it does not install elsewhere as is.

The words follow [docs/terms.md](docs/terms.md): a **model** is the weights, the **harness** turns a
model into an **agent**, and a **workforce** is many agents with roles, staffing and policy. A
model is never called an agent.

## Try it

You need Nix with flakes to build North. Install and sign in to a provider CLI to launch sessions
for that provider.

```sh
git clone https://github.com/tompassarelli/north && cd north
nix develop -c cargo build --locked
target/debug/north
```

How the harness is installed on Tom's machines, how a worker is staffed, where policy lives and how
a change lands: [docs/harness.md](docs/harness.md).

## Finding a fact

Read in three rings and stop at the first one that answers:

- **First:** this file for people, [AGENTS.md](AGENTS.md) for agents. AGENTS.md holds the rules for
  working here and names the document to read before touching each area.
- **When you touch an area:** the concept document for it, below. Concept documents explain how
  the parts fit and link to the owner of each detail; they do not restate flags or numbers.
- **For the exact fact:** its owner. `north --help` and each command's own `--help` for commands
  and flags, [harness/defaults.toml](harness/defaults.toml) for every tuned number with its unit and
  source, [.safe-push](.safe-push) for the checks a landing runs,
  [structure.toml](structure.toml) for module boundaries, a skill's SKILL.md for a procedure, and
  [docs/rationale/](docs/rationale/) for why a choice was made.

## Where things live

- [src/](src/) and [crates/](crates/): the `north` fleet runtime, terminal screen, landing gates and
  authorized host effects; [docs/architecture.md](docs/architecture.md) maps the owning boundaries.
- [bin/](bin/): every harness command (`agents`, `threads`, `safe-push` and more);
  [docs/features.md](docs/features.md) says what each one is for.
- [harness/](harness/): hooks, skills, defaults and policy owners;
  [docs/harness.md](docs/harness.md) follows them end to end.
- [agent-machinery/](agent-machinery/): provider-independent doctrine, contracts and skills;
  [agent-machinery/README.md](agent-machinery/README.md) introduces the package.
- [docs/](docs/): the concept documents, the feature entries and the rationale notes.

## Concept documents

- [docs/harness.md](docs/harness.md): install, staffing, policy and landing, end to end.
- [docs/terms.md](docs/terms.md): the four layer terms.
- [docs/architecture.md](docs/architecture.md): the `north` app's owning boundaries.
- [docs/features.md](docs/features.md): what each command does.
- [docs/feedback-loop.md](docs/feedback-loop.md): each recorded signal, its writer and its reader.
- [docs/python-port-order.md](docs/python-port-order.md): the order the remaining Python moves to Rust, from its import cycle.
- [docs/references.md](docs/references.md): outside sources, what they shaped and what came of them.
- [docs/rationale/review-triggers.md](docs/rationale/review-triggers.md): why review is triggered by risk, not a landing trailer.
- [agent-machinery/skills/verification/SKILL.md](agent-machinery/skills/verification/SKILL.md): how tests are kept and run.

## Keeping the docs true

Each fact lives in one place and the docs link to it. `north citation-check` refuses a landing whose
docs, skills or hooks name a `north` subcommand the CLI does not define, a repository path that no
longer exists or a relative link that does not resolve. Rationale notes are dated records and keep
the paths of their day. The layout and the work still open are in
[docs/rationale/docs-architecture.md](docs/rationale/docs-architecture.md).
