# North

North is the harness the fleet runs on: the agent commands in `bin/`, the hooks, skills and policy in
`harness/`, and the `north` TUI, whose state transitions are typed Rust. `README.md` says what it is
and how to try it; this file holds the rules for working in it.

The toolchain is pinned by `flake.nix` and `rust-toolchain.toml`, so run Cargo through `nix develop`.
Build output stays in the worktree's `target/`.

## Rules

- Use the four layer terms of `docs/terms.md`, one per layer: model, harness, agent, fleet. North is the harness and the fleet is the agents it runs (South holds their private policy); never call a model an agent.
- Keep fleet screen state and rendering in `crates/north-tui/src/tui/fleet`; the Rust host performs authorized effects and renders projections.

- Change Rust build settings (profiles, linker, caches, crate splits) only after
  measuring them per [compile-times](harness/skills/rust-development/references/compile-times.md).
- Follow [verification](harness/skills/verification/SKILL.md) for test retention, mutation evidence and execution.
- Before landing, check only the crates you touched, `depbox run -- nix develop -c cargo clippy -p CRATE --all-targets --locked --offline -- -D warnings` (the land check refuses any warning), and the tests that exercise the change by name filter, plus
  `north check harness tree-policy --tree .` for its procedures. Never run the workspace suite or workspace clippy first: the lander runs them once on the merged tree, so a pre-run doubles the work ([tiers and landing](harness/skills/verification/references/tiers-and-landing.md)). The exception is a change to the landing machinery, which runs its own tests first.
- Run `verify-north` in your worktree before claiming a north change works; it builds north, walks the spine (a brief, a worker, a landed commit, a ledger row) on a scratch repository, and cleans up.
- Edit in `worktrees/<slug>`, never `main/`, and land through `safe-push --to main`.
- Run the affected `*.test.sh` or `*.test.py` under `bin/`, `lib/` or `harness/` for a harness change; the `harness.yml` workflow runs them all except its listed machine-bound ones.
- Public and reusable goes to north or firn; Tom-specific or secret goes to south.
- A harness lane's commit message names its `Expected effect:` on finished boxes per unit of usage; `north dash --weekly` counts the week's lanes that do and lists those that caused an incident within 48 h (north#706).
- Keep the concerns in `concerns.toml` true: a new concern is one `[[concern]]` with its source line, oracle and class, and `north check concerns` runs them all; a red concern reaches the proxy through the fleet tick.

## Routers

- Before changing `crates/north-safe-push`, `.safe-push` or `crates/north-landing/src/landing/mod.rs`, read [docs/harness.md](docs/harness.md#landing-a-change) for how a lane reaches main.
- Before changing `crates/north-fleet/src/fleet/mod.rs` or `crates/north-tui/src/tui/fleet/mod.rs`, read [docs/architecture.md](docs/architecture.md#owning-boundary) and the [fleet sessions](docs/features/03-fleet-sessions/) features.
- Before changing `crates/north-threads/src/threads/staffing/mod.rs` or `crates/north-pace/src/router.rs`, read [docs/harness.md](docs/harness.md#staffing-a-worker) and the [staffing and routing](docs/features/02-staffing-and-routing/) features.
- Before changing `crates/north-capacity/src/capacity/admission.rs` or a tuned number in `harness/defaults.toml`, read the [machine-capacity skill](harness/skills/machine-capacity/SKILL.md) and the [capacity](docs/features/08-machines-capacity-and-scheduled-jobs/) features.
- Before changing `harness/policy-owners.toml`, the hook dispatcher `crates/north-guard/src/hook.rs` or a skill per `harness/SKILL-ARCHITECTURE.md`, read [docs/harness.md](docs/harness.md#where-policy-lives) and the [agent-policy skill](harness/skills/agent-policy/SKILL.md).
- Before changing the policy gate in `crates/north-core/src/agent_config_check/gate.rs` or `agents sync`, read [docs/harness.md](docs/harness.md#install) for how policy is published.
- Before changing `crates/north-core/src/grounding/sources/repos.rs` or `crates/north-core/src/grounding/prompt_hook.rs`, read the [policy measurement](docs/features/06-policy-measurement/) features, since docs lines feed that index.
- Before changing what `crates/north-threads/src/threads/runs/record.rs` or `crates/north-providers/src/ledger/mod.rs` records, read [docs/feedback-loop.md](docs/feedback-loop.md) for each signal and the reader that acts on it.
- Before adding or changing a command, add its entry under `docs/features/`, its kind under `src/cli/verbs/`, its reference line under `src/cli/reference/` and, when `north --help` lists it, its file under `src/cli/help/`; any table of `structure.toml` goes under `structure/`. The features index is not tracked and `src/cli/help.txt`, `src/cli/reference.txt`, `src/cli/verbs.rs` and `structure.toml` are written from those trees: the build writes the compile-time registries before the crate compiles, `north docs` and the lander write the rest, and `north land scan generated-registry` refuses a lane that commits one.
- Before changing provider-independent doctrine, contracts or procedures, read [harness/SKILL-ARCHITECTURE.md](harness/SKILL-ARCHITECTURE.md); [docs/terms.md](docs/terms.md) names the layers.
