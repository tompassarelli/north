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

## Where things live

- [src/](src/): the `north` fleet runtime, terminal screen and authorized host effects.
- [bin/](bin/): every harness command (`agents`, `threads`, `safe-push`, `worker-ledger` and more).
- [harness/](harness/): hooks, skills, defaults and policy owners.
- [agent-machinery/](agent-machinery/): provider-independent doctrine, contracts and skills.
- [docs/](docs/): the documents below.

## Read next

Each document has one job:

- [docs/harness.md](docs/harness.md): install, staffing, policy and landing, end to end.
- [docs/terms.md](docs/terms.md): the four layer terms.
- [docs/architecture.md](docs/architecture.md): the `north` app's owning boundaries.
- [docs/features.md](docs/features.md): what each command does.
- [docs/feedback-loop.md](docs/feedback-loop.md): each recorded signal, its writer and its reader.
- [docs/python-port-order.md](docs/python-port-order.md): the order the remaining Python moves to Rust, from its import cycle.
- [docs/references.md](docs/references.md): outside sources, what they shaped and what came of them.
- [docs/rationale/targets.md](docs/rationale/targets.md): why numeric targets gate or not.
- [AGENTS.md](AGENTS.md): rules for agents working in this repository, including build and check
  commands.
- [agent-machinery/README.md](agent-machinery/README.md): the shared procedures package.
- [agent-machinery/skills/verification/SKILL.md](agent-machinery/skills/verification/SKILL.md): how tests are kept and run.
