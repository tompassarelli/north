# Rationale notes

Why north is built the way it is, one note per decision. `north docs rationale-index` writes this list from each note's title and first sentence; edit the notes, not this file.

- [Account usage: one reserve, placement that manages work in flight, and a spend breaker](account-capacity.md): Plan of 2026-11-04 (laptop time), revised the same day to Tom's design of 2026-11-05.
- [Build cost: where north's compiles burn cores, and what to cut first](build-cost.md): Plan of 2026-11-04 (laptop time, +07), revised the same day after review.
- [Capacity scheduler: elastic, learned, fleet-wide placement by predicted finish time](capacity-scheduler.md): Owner: north#259
- [Conflict-free landing: no lane starves, no author round-trip for a mechanical conflict](conflict-free-landing.md): On 2026-11-05 Tom asked for a fix by construction after the lane that puts every repository on the lander (s18-integrated) lost the race to main four times in 20 minutes while its author rebased by hand.
- [Crate split: why a leaf edit rebuilt 141k lines](crate-split.md): Measured 2026-10-31 on the laptop, heavy class (6 CPUs, 8 GiB), warm cache: appending a blank line to `src/provenance.rs` cost 10.5 s for `cargo build` and 15.6 s for `cargo test --no-run`.
- [Docs architecture](docs-architecture.md): Owner: north#523
- [Failing-first rationale](failing-first.md): Cold-path reasoning behind the rule that a bug fix lands with a test seen failing on the code before the fix, and a refactor declares `Behavior: unchanged` with its changed tests passing on the old code.
- [Hooks against the runtime](hooks-and-runtime.md): Audited 2026-11-04 against Claude Code 2.1.317, for the 18 hook entries installed in the `claude-d` profile (`jq '.hooks' ~/.local/state/north/profiles/claude-d/settings.json`).
- [Landing latency: where a worker's commit waits before main](landing-latency.md): Measured on Mak'gora, 2026-10-28, while about 15 workers waited on their landings during a live playtest.
- [Landing train: main never red, finished work never waits on someone else's break](landing-train.md): Owner: north#360
- [Learned routing and goal ETA measurements](learned-routing-and-goal-eta.md): Dated measurements behind two routing decisions: the learned tier-routing backtest that kept the cell-rate rule, and the replay that set how the goal ETA reads box durations.
- [Measured no gain: what to retire](measured-no-gain.md): Audited 2026-11-04 on north origin/main `7c2a76350`.
- [Current Nix practice, November 2026, against how Tom's repositories use Nix](nix-current-practice.md): Research of 2026-11-04 for north#553.
- [Nix and dev-shell footguns: every failure class and the guard that closes it](nix-footguns.md): Plan of 2026-11-05 (laptop time, +07) for north#553.
- [Non-Rust inventory](non-rust-inventory.md): Every non-Rust code file in North, written 2026-11-04 for north#88: shell, Python, TypeScript and JavaScript files, and shebang scripts in `bin/` and `harness/`.
- [One landing per change](one-landing-per-change.md): A change lands on main as one commit.
- [The proxy's role: route and judge, delegate the rest, and measure it](proxy-role.md): Tom asked on 2026-11-05 for the orchestrating session to act as one: do brief cross-cutting work itself and delegate everything that takes several steps, with a deterministic mechanism behind each rule wherever one can carry it.
- [pstack integration rationale](pstack.md): Cold-path reasoning for which pstack practices North adopted, and which it did not (decided 2026-10-15).
- [Worker read isolation](read-isolation.md): Measured 2026-10-30 for north#59 step 1 (north#361): which Claude Code settings stop a worker session from reading session credentials (`~/.claude/sessions`) and reaching peer session sockets (`/run/user/1000/cc-socks`), whatever command it runs.
- [Review triggers](review-triggers.md): Why north has no review-trailer landing gate, and when a fresh reviewer is staffed instead.
- [Rust build times: a measured trigger for crate splits](rust-build-times.md): Tom asked on 2026-11-04 that Rust compile times stay fast as Wisp grows.
- [Targets rationale](targets.md): Cold-path reasoning behind numeric performance, time and size targets.
- [Tier routing rationale](tier-routing.md): Cold-path reasoning behind which model tier a box starts on.
- [Wake paths: what wakes an agent turn, and what to cut](wake-paths.md): Measured 2026-11-04 over the 7 days 2026-10-28 to 2026-11-04 (UTC).
