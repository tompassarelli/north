# Learning loops: the registry of every automatic decision north makes

The loop registry, `harness/loops.toml`, lists each decision north makes without a person (the tier a box gets, the ETA and budget its brief states, the cores a lease charges, the deadline a land check gets, which machine a launch runs on) and states for each what it learns from, what updates it and how often. `north loops check` reads the registry against the machines' state and fails when a learning loop has stopped learning. Decided 2026-11-05 under north#112.

## What a loop is

A `[[loop]]` entry names the decision, the source files that make it (`sites`), the outcome it would learn from (`signal`), the program that updates it (`updated_by`), how often (`cadence_hours`), and where its last update shows: the newest tick its writer told the graph (`store = "graph"` and `writer`, below), or the record whose newest write is its last update (`record`, a path under the state root, `XDG_STATE_HOME` or `~/.local/state`). A record that is a directory counts as written when any entry in it is.

## Closed and open

- A **closed** loop has a `signal` and a `record` or graph `writer`: an automatic program measures an outcome and rewrites the decision. `north loops check` fails it when its last update is older than twice its cadence (STALE), when it never updated, or when the graph cannot be read.
- An **open** loop has no `signal`: its decision is a constant, a table or a live reading that nothing yet adjusts from outcomes. It must name an `owner`, the box that closes it. An open loop prints as `open` and does not fail the check, so the registry can list what is still hand-set without raising an alarm every day. An open loop with no owner does not parse.

The rule is that nothing learns silently and nothing is hand-set without a box that will change that. Closing a loop means turning its `owner` into a `signal` and a `record`.

## Where a record is read

State lives on more than one machine: the ledger and the tune cells on the ledger host, the lander's queue and check timings on the lander host. Each closed loop therefore names a `host` role:

- `ledger`: the host in `~/.config/threads/remote`, or this host when that file is absent.
- `lander`: the host `git config -f <harness>/.safe-push --get landing.lander-host` names.
- `here` (the default): the host running the check.

No host reads another: the gex45 and nexus access decisions keep those hosts pull-only, with no keys to reach the laptop, so an ssh read of the lander host from nexus could never succeed (2026-11-05). `north loops check` reads from the local files only the closed loops whose role this host holds, and lists every other closed loop as `checked on ROLE`, which does not fail. An unresolvable lander host means this host does not hold the lander role. `north loops check --role ROLE` checks only the loops of that role, read locally, whatever this host is named.

Each role is checked where its records live. The daily `north tune` on the ledger host checks the ledger and here loops. On the laptop, the lander host, the hourly north-tempo user timer in nixos-config (modules/north-tempo) runs `north loops check --role lander` after recording tempo, so a stale lander loop fails that unit.

## Where a loop's facts live: the graph

A loop that learns tells its learned values to north's graph (`~/.local/state/north/graph.db`, one per host) instead of keeping its own file or SQLite table, and the decision reads them back through a Datalog rule, so every loop shares one store, one history and one query language (north#882, the world-model plan's decision 7). The pattern, first built for tier, eta and budget:

- **Writer.** The program that updates the loop tells as one writer name (`tune` for north tune). Each tick is one transaction through `Graph::apply`: it retracts each value that moved or went, asserts each new one, and moves the writer's tick, `(loop:WRITER :loop/tick-at INSTANT)`. A tick that changes no value still moves the tick, since its age is the proof the loop ran.
- **Learned values.** A value is one triple on a subject named for what it is about, with a predicate in the loop's keyword namespace: north tune's cell `eta_minutes opus feature` is `(cell:feature:opus :cell/eta-minutes 92.0)`, its budget `:cell/budget-usd` and the category's Opus effort `:cell/effort "high"` on the same subject. A value the writer measured itself is asserted under its own name; a worker's claim it read stays quoted until a checker, the lander or git corroborates it.
- **Rules.** Each decision reads through one rule file under `harness/graph-rules/` that names the writer whose assertions count: `eta.json`, `budget.json` and `effort.json` for dispatch's ETA, budget and tier effort, and `ticked.json` for every writer's newest tick. `crates/north-graph-loops` holds the vocabulary, the one-transaction tell and the rule readers; a new loop family adds its measures there.
- **Registry.** The loop's entry says `store = "graph"` and `writer = "WRITER"` in place of `record`, and `north loops check` judges it from the instant of that writer's newest tick in the local graph, on the host its `host` role names.
- **Predicates.** A loop's predicates live in its own namespace (`cell/`, `loop/`, `capacity/`...), because `north graph ingest` retracts every live fact whose predicate a source replaces, whichever writer told it (bare `ticked` belongs to the issues source).

### Every other loop: its store today and its wiring step

Each step below is one lane: add the writer's measures to `crates/north-graph-loops`, tell them each tick from the named updater, read them through the named rule file at the named site, set `store = "graph"` and `writer` on the loop, and delete the old record once nothing reads it.

- **capacity-estimate** (`agents/machine-capacity/capacity.db`, SQLite, here). Writer `capacity`, told by the lease finisher in `crates/north-capacity-host/src/capacity/host/mod.rs` once per finished lease: `(signature:HASH :capacity/cores N)`, `:capacity/memory-mb N`, `:capacity/wall-s N` from the newest runs of that signature. Rule `capacity-estimate.json`, read by `estimate.rs` in place of its SQLite sample query. The per-lease samples stay in capacity.db as raw observations; the graph holds the estimate each signature learned.
- **check-deadline** (`safe-push/check-timings/passes`, a directory of files, lander). Writer `land`, told by `crates/north-landing-gates/src/landing/check_timings.rs::record` after each passing run: `(check:COMMAND :check/deadline-s N)` from the doubled p90 wall, plus `:check/timed-out true` after a timeout on lane and base. Rule `check-deadline.json`, read by `crates/north-land-checks/src/land/slots/runner.rs`.
- **corrections** (`north/grounding/ledger.jsonl`, lander's grounding store, here). Writer `distill`, told by `crates/north-core/src/grounding/distill.rs` each run: `(correction-class:CLASS :corrections/week-count N)` per ISO week subject `(week:YYYY-Www)`, and `:corrections/guarded true` once a guard box exists. Rule `corrections.json`, read by `crates/north-flow/src/detectors/correction.rs` for the twice-seen and recurring verdicts. The ledger lines stay as quoted observations of Tom's prompts.
- **scoreboard** (`north/scoreboard/week.json`, here). Writer `scoreboard`, told by `src/cli/scoreboard/weekly.rs::record`: `(number:NAME :scoreboard/week-figure F)` and `:scoreboard/observations N` on subject `number:NAME:YYYY-Www`. Rule `scoreboard.json`, read by the same file's week-on-week comparison.
- **code-health-review** (`code-health/north/loop.json`, lander). Writer `code-health`, told by `crates/north-code-health/src/code_health/signal.rs::record_loop`: `(review:north :code-health/hand-files-median F)`, `:code-health/revert-share F` and one `:code-health/reason TEXT` per fired reason. Rule `code-health-review.json`, read by `tick.rs`.
- **slow-test** (`agents/flow/detectors.json`, shared with every flow detector, lander). Writer `flow`, told by `crates/north-flow/src/detectors/slow_test.rs`: `(test:BINARY::NAME :test/pass-s F)` for the newest run of each binary, `:test/slow true` when over its bound. Rule `slow-test.json`. detectors.json keeps the other detectors until their own lane.
- **lander-doctor** (`north/land-doctor/book.json`, lander). Writer `land-doctor`, told by `crates/north-lander-doctor/src/book.rs::save`: `(bound:NAME :doctor/state :ok|:over|:unknown)` and `(incident:CLASS :doctor/count N)`. Rule `lander-doctor.json`, read by `bounds.rs` and `hands.rs`.
- **fleet-retire** (`north/fleet/.reap.json`, here). Writer `fleet`, told by `crates/north-fleet-supervisor/src/fleet/reaper.rs` each tick: `(session:NAME :fleet/verdict :idle|:parked|:stale)` and `(fleet:retire :fleet/dead-letters-24h N)`. Rule `fleet-retire.json`, read by `registry/retiring.rs`.

### Named exceptions

- **remote-branches** and **credentials** stay on their record files: each records that a sweep or an expiry check ran, not a value learned from an outcome, so the graph would hold only a tick. They move if they start learning (a sweep age or a renewal lead time set from outcomes).
- **effort**, **pressure-band**, **placement**, **worker-concurrency** and **skill-routing** are set by hand (orchestration.toml, defaults.toml, `resident_cap`, skill frontmatter) and learn from no outcome; **context-window**, **landing-batch** and **policy-climber** are open loops whose owners have not closed them. Each moves to the graph in the lane that gives it a signal, telling its first learned value as above.

## The tune tick fails on a stale loop

`north tune` runs the check after its own work and appends the registry's lines to its output. Its exit code is 1 when any closed loop it holds is stale or never updated, or when the registry cannot be parsed, which fires the nexus unit's failure alert. The same code that fails `north tune` fails `north loops check`.

On the lander host the check also reads the watch's cargo.yml run history (`north/watch/runs-north.json`) and fails when main stayed red over 10 minutes with neither a sheriff revert (`Sheriff-Reverts:` on origin/main after the red commit) nor a fleet message naming the commit, recorded in `north/watch/named.jsonl` by `north ops watch`; reds from before the record began are not judged. Added 2026-11-08 under north#634 after Tom twice had to be the alarm for a red main (class tom-as-monitor in `harness/corrections.toml`).

## Completeness

The registry is kept complete by a test in `crates/north-tune/src/loops/inventory_tests.rs`, not by comments at the decision sites. It scans the dispatch, fleet-launch, placement, capacity-admission and lander source directories (leaving out test files and the text after `#[cfg(test)]`) for numeric `const` items and for the keys of `default(...)`-style reads of `harness/defaults.toml`. Each must be named, as `path::NAME` or as the key, in some loop's `constants` or `defaults`, or in the `[plumbing]` lists at the end of the registry, which hold values that do not decide anything (protocol limits, buffer sizes, wait intervals). A name the scan no longer finds also fails, so the lists cannot go stale.

A new constant under those directories therefore stops the build until someone says which decision it belongs to or that it is plumbing. Placing it on a loop makes it visible as a hand-set input of that loop. Rejected: marker comments at the sites (the repository allows no code comments), a count ratchet (it hides which constant was added), and a hand-kept function list alone (a new constant would pass unseen).

## Adding a loop

1. Add a `[[loop]]` with `name`, `decision`, `sites` (files that exist), `updated_by` and `cadence_hours`.
2. If the decision already learns, add `signal`, `store = "graph"` with its `writer`, and the `host` whose graph it tells, and make the updating program tell its tick on every run, including a run that changes nothing, since the tick's age is the proof the loop ran. If it does not, add `owner` naming the box that will close it.
3. Name its constants and defaults keys in `constants` and `defaults`.
4. Run `cargo test --locked --offline -p north-dispatch loops`, then `north loops check`.
