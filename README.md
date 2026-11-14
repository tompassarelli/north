<p align="center"><img src="docs/assets/north-logo.jpeg" alt="North" width="720"></p>

North turns an issue with a runnable check into a change landed on main, with no person in the loop.
You write what you want and the command that proves it is done; north writes a brief, staffs a coding
agent on a model and account that has room, guards each of its tool calls, checks the change it hands
in, lands it on main, and reverts it if main breaks. It drives the command-line clients you already
use (Claude Code, Codex, OpenCode and Pi), so it adds the staffing, the guards and the landing, not
another model. You direct the fleet from `north`, one terminal screen for talking to the coordinator,
starting work, answering decisions and seeing what landed.

## Measured result

On small tasks north lands a change in 12 s against T3 Code's 16 s at one worker and 13 s against 21 s at five at once, at about 0.3 of the cost (15 pairs per cell; method and every row on [the comparison page](docs/rationale/plain-runner-comparison.md)).

## Quickstart

You need x86_64 Linux with Nix and flakes, `git`, and Claude Code installed and signed in. The command
clones and builds north, makes an account profile from the Claude Code login, has one agent land one
change on a scratch repository and exits 0 when it is on main ([the onboarding page](docs/onboarding.md)).

<!-- north docs readme: quickstart -->
```sh
nix run github:tompassarelli/north#setup
```
<!-- end quickstart -->

Then put `~/code/north/main/target/debug` on your `PATH` and run `north` to open the fleet screen.

## How a change lands

1. **Brief.** An issue's box carries a `Done when:` line with a command. North writes the brief and
   picks the tier, the account and the machine from the live usage of each ([docs/briefing.md](docs/briefing.md)).
2. **Work.** A worker session runs in its own worktree. Every tool call passes north's guards first, which
   keep it out of the main checkout, other lanes and secrets.
3. **Hand-in.** The worker calls the finish tool. North runs the box's check, commits only the paths
   named and queues the lane.
4. **Landing.** The lander rebases the lane on main, runs the checks the change touches and pushes it,
   then runs the full suite once per batch and reverts a lane that breaks main ([docs/landing.md](docs/landing.md)).
5. **Record.** Each run's tokens, time and verdict go to a ledger, and north tunes its estimates and
   routing from it ([docs/feedback-loop.md](docs/feedback-loop.md)).

The words follow [docs/terms.md](docs/terms.md): a **model** is the weights, the **harness** turns a
model into an **agent**, and a **fleet** is many agents with roles, staffing and policy.

## Where things live

- [src/](src/): the `north` command, one file per command
- [crates/](crates/): the libraries behind it, one crate per responsibility in seven layers ([ARCHITECTURE.md](ARCHITECTURE.md))
- [harness/](harness/): the hooks, skills, policy and tuned defaults north installs into each agent
- [docs/](docs/): concept pages, one record per feature, and the rationale for each choice
- [evals/](evals/): the runners that measure north against other harnesses
- [examples/](examples/): a working operator repository with synthetic values

## Finding a fact

- [AGENTS.md](AGENTS.md): the rules for working in this repository
- [docs/README.md](docs/README.md): every page, in reading order
- [docs/features/](docs/features/): what each command is for
- [docs/rationale/](docs/rationale/): why it is built this way

## License

MIT or Apache-2.0, at your choice: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE).
