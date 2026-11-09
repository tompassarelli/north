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

A loop that learns tells its learned values to north's graph (`~/.local/state/north/graph.db`, one per host) instead of keeping its own file or SQLite table, and the decision reads them back through a Datalog rule, so every loop shares one store, one history and one query language (north#882, the world-model plan's decision 7). The pattern, first built for tier, eta and budget and followed by scoreboard and code-health-review:

- **Writer.** The program that updates the loop tells as one writer name (`tune` for north tune). Each tick is one transaction through `Graph::apply`: it retracts each value that moved or went, asserts each new one, and moves the writer's tick, `(loop:WRITER :loop/tick-at INSTANT)`. A tick that changes no value still moves the tick, since its age is the proof the loop ran.
- **Learned values.** A value is one triple on a subject named for what it is about, with a predicate in the loop's keyword namespace: north tune's cell `eta_minutes opus feature` is `(cell:feature:opus :cell/eta-minutes 92.0)`, its budget `:cell/budget-usd` and the category's Opus effort `:cell/effort "high"` on the same subject. A value the writer measured itself is asserted under its own name; a worker's claim it read stays quoted until a checker, the lander or git corroborates it.
- **Rules.** Each decision reads through one rule file under `harness/graph-rules/` that names the writer whose assertions count: `eta.json`, `budget.json` and `effort.json` for dispatch's ETA, budget and tier effort, `scoreboard.json` for the weekly comparison, `code-health-review.json` for the review brief, and `ticked.json` for every writer's newest tick. `crates/north-graph-loops` holds the vocabulary, the one-transaction tell and the rule readers; a new loop family adds its measures there.
- **Registry.** The loop's entry says `store = "graph"` and `writer = "WRITER"` in place of `record`, and `north loops check` judges it from the instant of that writer's newest tick in the local graph, on the host its `host` role names.
- **Predicates.** A loop's predicates live in its own namespace (`cell/`, `loop/`, `capacity/`...), because `north graph ingest` retracts every live fact whose predicate a source replaces, whichever writer told it (bare `ticked` belongs to the issues source).

### Loops on the lander host

- **check-deadline.** Writer `land`, told by `crates/north-landing-gates/src/landing/check_timings.rs` (`Timings::passed` and `Timings::exceeded_on_base_too`, each followed by one `Graph::apply`): `(check:CLASS:COMMAND :check/deadline-s N)` with `:check/passes`, `:check/p90-s` once five runs passed, and `:check/exceeded-s` with `:check/timed-out` after a timeout that lane and base both ran past. Rule `check-deadline.json`, read by `Timings::deadline`, which the runner calls through `Timings::standard()` in `crates/north-land-checks/src/land/slots/runner/judge.rs`. The passes files under `safe-push/check-timings/passes` stay as the raw wall observations the deadline is computed from, and a check with no graph value yet reads its deadline from them. The write waits at most 300 ms for the graph's write lock and is dropped past that, so an ingest never slows a land check; the next pass tells again.
- **lander-doctor.** Writer `land-doctor`, told by `crates/north-lander-doctor/src/book.rs::save` once per run, changed or not: per bound `(bound:KEY :doctor/state :ok|:over|:unknown)` and, while it is held, `:doctor/since`, `:doctor/remedy "AT CLASS"` and `:doctor/paged`; `(lander:BUILD :doctor/good RANK)` for each build that landed green; `(incident:CLASS :doctor/count N)`. Rule `lander-doctor.json`, read by `Book::load` into the book `bounds.rs` and `hands.rs` decide from. The first run after this change reads `north/land-doctor/book.json` once, tells it, and deletes the file.
- **slow-test.** Writer `flow`: each flow detector run tells, from the nextest logs under `land-queue/*/logs`, every test over its line in each of its newest runs: `(test:BINARY NAME :test/slow true)` and `:test/pass-s F` for its newest run. Rule `slow-test.json` is read by `crates/north-flow/src/detectors/slow_test.rs` to pick the tests to profile and file. A test under its line has no facts, so the graph holds the decision's candidates and not every test's time. `agents/flow/detectors.json` keeps the boxes already filed for every flow detector until their own lane.
- **code-health-review.** Writer `code-health`: `signal.rs::record_loop`, run by `north code-health signal --record`, tells each repository's reading as one transaction: `(review:REPO :code-health/hand-files-median F)`, `:code-health/revert-share F` and one `:code-health/reason TEXT` per reason that fired, replacing that repository's earlier reading, and moves `(loop:code-health :loop/tick-at INSTANT)`. `tick.rs` reads the repository's reading through `code-health-review.json` into the review brief's `{{measured}}` line, so the reviewer sees the cost the loop measured. `loop.json` is gone; `loop.txt` stays as the score line.

### Loops on every host

- **capacity-estimate.** Writer `capacity`, told by `crates/north-capacity-host/src/capacity/host/learned.rs::tell` once per finished lease: `(signature:HASH :capacity/MEASURE value)` for the runs behind the estimate, the p90 and p50 of cores, memory, I/O and disk-busy share, and the p75 wall seconds over the signature's newest runs on this host. Rule `capacity-estimate.json`, read by sizing through `learned.rs::read`. The per-lease samples stay in `agents/machine-capacity/capacity.db` as the raw observations the estimate is computed from.

### Loops on the laptop

- **corrections.** Writer `distill`: `crates/north-core/src/grounding/distill.rs` tells each run every correction in the ledger as `(correction-class:CLASS :corrections/seen-at INSTANT)`, one fact per correction, in one transaction with the writer's tick. Rule `corrections.json` is read by the repeated-correction detector in `crates/north-flow/src/detectors/correction.rs` (twice seen) and by the fleet tick's recurrence check (a guarded class seen again within a week of its guard). The first graph-backed run tells the whole ledger, so no history is seeded by hand. The ledger lines stay as the quoted observations of Tom's prompts, and `corrections-week.txt` stays the printed score. Decided 2026-11-08 under north#882: an instant per correction in place of a weekly count per class, because the recurrence verdict needs the correction's time against the guard's.
- **fleet-retire.** Writer `fleet`, told by `crates/north-fleet-supervisor/src/fleet/reaper.rs` on each applied tick: `(session:NAME :fleet/verdict :idle|:parked|:stale)` and `(fleet:retire :fleet/dead-letters-24h N)`. Rule `fleet-retire.json`, read by `registry/retiring.rs`; the reaper no longer writes `.reap.json`.
- **scoreboard.** Writer `scoreboard`: `src/cli/scoreboard/weekly.rs::record` tells each recording as one transaction: `(number:NAME:YYYY-Www :scoreboard/week-figure F)`, `:scoreboard/observations N` and `:scoreboard/higher-is-better BOOL` for the week ending at the recording and the week before it, replacing the readings of earlier recordings, and moves `(loop:scoreboard :loop/tick-at INSTANT)`. `weekly::tick` reads them back through `scoreboard.json` and compares the two weeks of each number; `week.json` is gone, and `week.txt` (the score line `north loops check` prints) and `told.txt` (which recording the proxy last heard) stay as files, since neither is a learned value.

### Named exceptions

- **remote-branches** and **credentials** stay on their record files: each records that a sweep or an expiry check ran, not a value learned from an outcome, so the graph would hold only a tick. They move if they start learning (a sweep age or a renewal lead time set from outcomes).
- **effort**, **pressure-band**, **placement**, **worker-concurrency** and **skill-routing** are set by hand (orchestration.toml, defaults.toml, `resident_cap`, skill frontmatter) and learn from no outcome; **context-window**, **landing-batch** and **policy-climber** are open loops whose owners have not closed them. Each moves to the graph in the lane that gives it a signal, telling its first learned value as above.

## The tune tick fails on a stale loop

`north tune` runs the check after its own work and appends the registry's lines to its output. Its exit code is 1 when any closed loop it holds is stale or never updated, or when the registry cannot be parsed, which fires the nexus unit's failure alert. The same code that fails `north tune` fails `north loops check`.

On the lander host the check also reads the watch's cargo.yml run history (`north/watch/runs-north.json`) and fails when main stayed red over 10 minutes with neither a sheriff revert (`Sheriff-Reverts:` on origin/main after the red commit) nor a fleet message naming the commit, recorded in `north/watch/named.jsonl` by `north ops watch`. A main-is-red alert for north recorded while the red interval was open also covers it, so a red whose first run has left the bounded run list stays covered (north#987); reds from before the record began are not judged. Added 2026-11-08 under north#634 after Tom twice had to be the alarm for a red main (class tom-as-monitor in `harness/corrections.toml`).

## Completeness

The run-time-data check (`validate` in `crates/north-tune/src/loops/mod.rs`, with the test `the_repo_loops_registry_validates_so_north_loops_and_the_tempo_unit_can_read_it`) checks the registry's shape: every `[[loop]]` parses with its required keys and a `[plumbing]` table exists. Nothing yet scans the dispatch, fleet-launch, placement, capacity-admission and lander source for numeric `const` items or `harness/defaults.toml` keys that no loop's `constants` or `defaults` and no `[plumbing]` list names, so a new constant does not stop the build. The lists in `constants`, `defaults` and `[plumbing]` are kept by hand, and a value that decides something belongs on a loop, where it shows as a hand-set input, while one that decides nothing (protocol limits, buffer sizes, wait intervals) belongs in `[plumbing]`. Rejected for the scan when it is built: marker comments at the sites (the repository allows no code comments) and a count ratchet (it hides which constant was added).

## Adding a loop

1. Add a `[[loop]]` with `name`, `decision`, `sites` (files that exist), `updated_by` and `cadence_hours`.
2. If the decision already learns, add `signal`, `store = "graph"` with its `writer`, and the `host` whose graph it tells, and make the updating program tell its tick on every run, including a run that changes nothing, since the tick's age is the proof the loop ran. If it does not, add `owner` naming the box that will close it.
3. Name its constants and defaults keys in `constants` and `defaults`.
4. Run `cargo test --locked --offline -p north-tune loops`, then `north loops check`.
