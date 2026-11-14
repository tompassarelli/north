# Architecture

north is one command line over a set of crates in seven layers. Each layer has one responsibility, and a crate depends only on layers below it. This page is the map, the life of a task and the rules. The reasons, the measured figures and the open work to reach the target are in [docs/rationale/architecture-layers.md](docs/rationale/architecture-layers.md).

## The layers

| Layer | It does one thing | The layer above may call |
|---|---|---|
| 1 platform | Touches the outside world and holds base values. It names no north noun from above. | Typed adapters (git, GitHub, processes, SQLite, ssh, Nix), the clock, identifiers, the shell lexer, the lifecycle store, the build interface. |
| 2 records | Remembers. One crate per durable store: its schema, its one writer, its typed readers. | Open a store, append a typed row, read typed rows. |
| 3 rules | Decides. A verdict from records, a diff, a tree or a tool call, with no effect beyond reading. | A function from typed input to a typed verdict; the check commands a landing runs. |
| 4 mechanisms | Acts. Four families that never depend on each other: sessions, workspace, landing, capacity. | Start, advance or stop one session, lane, queue entry or lease; ports to fill from outside. |
| 5 orchestration | Chooses what runs next, where, and on which build. Only here may one function call two families. | One decision or one order at a time. |
| 6 operations | Watches, measures, tunes, reports and repairs. Nothing below depends on it. | Verbs only. |
| 7 command | Parses arguments, wires ports to implementations, prints. No decision lives here. | The `north` command line and the terminal screens. |

To find a behaviour, ask what it does: talks to something outside north (platform), stores or reads back rows (records), says yes, no or how much (rules), runs a session, lane, landing or lease (that mechanism family), picks between tasks, hosts or builds (orchestration), reports on or repairs the rest (operations), reads arguments or draws a screen (command).

## The crates

| Layer | Crate | What it is for |
|---|---|---|
| 1 platform | north-foundation | The error type, the clock port and time values, and identifiers. |
| 1 platform | north-base | Adapters to the outside world (git, GitHub, processes, SQLite, ssh, Nix), the shell lexer, the lifecycle store and the verdict cache, and records that belong to layers above it. |
| 1 platform | north-strategy-types | The decision records the kernel writes and strategies read: one Case and one Decision per decision kind. |
| 1 platform | north-shared-config | The shared configuration a host reads, fetched into the state directory with its freshness bound. |
| 1 platform | north-publish | Create a file whole in one step, so no reader sees part of it. |
| 1 platform | north-registries | Read a generated registry's per-unit source tree and render the registry. |
| 1 platform | north-cli-reference | The command reference text and the calls it names. |
| 1 platform | north-git-locks | Stale git lock files: the one removal an agent may make in a primary checkout's git directory. |
| 1 platform | north-runtime-follow | How a long-lived process moves onto the runtime its link promotes. |
| 1 platform | north-agent-starts | Local agent starts: a start request the fleet tick admitted, read as fixed fields and handed to the agent user only through the host shell its polkit rule admits. |
| 1 platform | north-ssh-spare | A spare ssh connection held ready and the framed run over it. |
| 1 platform | north-build-system | The one interface north builds, lists and runs Rust tests through, with its Cargo and Buck2 sides. |
| 1 platform | north-buck | The Buck2 targets of the workspace crates, written from cargo metadata. |
| 1 platform | north-test-cases | Run one libtest binary's cases one process each and print a status line per case. |
| 1 platform | north-broker | The credential broker: a policy ceiling per user and scoped tokens swapped into allowlisted calls. |
| 1 platform | north-machine-model | Model checking of lifecycle machines against the real lifecycle store. |
| 2 records | north-threads | The threads ledger (items, holders, runs, messages, jobs) and the board read from GitHub. |
| 2 records | north-fleet-store | The fleet's stored rows: alarms, gates, held messages, pages and the session store. |
| 2 records | north-graph | The fact graph: terms, assertions with their writers, the five verbs and the Datalog engine. |
| 2 records | north-graph-loops | The learning loops' facts in the graph and the readers every decision site uses. |
| 2 records | north-graph-ground | Read the fact graph around the issues and files a message names. |
| 2 records | north-graph-change | The change queries the lander asks of the graph: affected tests, shared tests, blame, splits. |
| 2 records | north-github-mirror | The local copy of GitHub issues, CI runs and check results that pollers read. |
| 2 records | north-coverage | The map from each Rust test to the functions it executes and the tests that reach a change. |
| 3 rules | north-pace | Account usage readings, the router's tier, price and brief vocabulary, and the spend and credit books. |
| 3 rules | north-calibration | Calibration of estimates against run records, and a graft that re-exports the threads ledger under its own name. |
| 3 rules | north-capacity | Machine capacity leases and admission. |
| 3 rules | north-code-health | The code-health report, its gates, the layers check and the Rust habit checks. |
| 3 rules | north-implicit-state | The implicit-state shapes: rules that find state kept outside a typed lifecycle. |
| 3 rules | north-diff-gates | The gates that judge a diff: dependencies, language, provenance, build cache, stubs. |
| 3 rules | north-landing-gates | The gates that judge an issue or a lane: box lint, follows-release, abandon, fails-first. |
| 3 rules | north-land-scans | The scans a landing runs over a tree: docs, generated files, policy, test admission. |
| 3 rules | north-check-environment | Which check environment a landing runs in and the credential it may hold. |
| 3 rules | north-concerns | Each project's concerns as named checks with a bound judged on each measured value. |
| 3 rules | north-mutants | Mutation testing of a repository's tests: recorded mutants and the runs that verify them. |
| 3 rules | north-guard | Tool-call policy: every guard and the hook that dispatches a tool call to them. |
| 3 rules | north-context-pack | The context pack a brief may carry for each file a box names. |
| 4 sessions | north-claude-stream | Claude Code's stream-json wire format: encoders and the event decoder. |
| 4 sessions | north-session-host | The process that hosts one session: its pipes, sent log and frames. |
| 4 sessions | north-resume | The messages a resumed session opens with and whether it may resume. |
| 4 sessions | north-fleet | Headless agent sessions: the session model, the delivery journal, turns, tickets and the watch. |
| 4 sessions | north-fleet-supervisor | The session registry and supervisor: start, hand-in, handover. |
| 4 sessions | north-providers | The Claude, Codex, OpenCode, pi and agy adapters, and the run ledger. |
| 4 workspace | north-lanes | Worktree and pin lifecycle: new, reap, rescue, gc, layout checks. |
| 4 landing | north-land-sandbox | The sandbox a land check runs in: fence, recorder, supervisor. |
| 4 landing | north-land-checks | The check slots of a landing: select, run, cache. |
| 4 landing | north-landing | The landing queue and the lander's state, the steps after a landing, and the sheriff. |
| 4 landing | north-lander | The lander service: enqueue, serve batches, promote, and the hand-in road. |
| 4 landing | north-safe-push | The safe-push entry: request, gates, publication, restore. |
| 4 capacity | north-capacity-host | Host leases, the dependency box, the watchdog and the PATH gate. |
| 5 orchestration | north-pacing | Placement of work on hosts and accounts, laptop state and the resident cap. |
| 5 orchestration | north-dispatch | The dispatch decision: which box runs next, on which tier, with which brief. |
| 5 orchestration | north-runtime | Runtime update, canary, order and pin, and the worker sandbox. |
| 5 orchestration | north-offload | Running a land check, a session's build and the compiler cache on another host: which host has room, the slot a run holds and the script that ships and starts it. |
| 6 operations | north-timers | The timers and watches, the outside, job and usage-web readers, and the privacy judge. |
| 6 operations | north-tune | Tuned values and the learning-loop checks. |
| 6 operations | north-replay | Strategy replay: logged decision cases decided again by the linked code. |
| 6 operations | north-flow | The flow report, and agents, detectors, experiments, tempo and tool mining. |
| 6 operations | north-docs | The north docs command: each generated index written from its per-unit source tree. |
| 6 operations | north-ops | Thirty unrelated operations verb families, from agent capture to desktop upkeep. |
| 6 operations | north-issue-boxes | An issue's checkboxes as work: converted into child issues, and ticked when their check passes. |
| 6 operations | north-claims | Claims a written text makes about the tree, checked against it: cited commands, flags, paths and keys, and a design review's facts. |
| 6 operations | north-expectations | Declared expectations judged on a timer, each alerting once per breach and once per clear. |
| 6 operations | north-reclaim | Disk room on the fleet's hosts: what may be deleted or archived, under which budget, and when. |
| 6 operations | north-health | Health facts: failed starts, flows, issues, loops, red main, rework. |
| 6 operations | north-convo | Transcript search, and policy replay, and the context audit. |
| 6 operations | north-lander-doctor | The lander doctor: each health bound read live and the remedy for each stuck state. |
| 6 operations | north-land-watch | The watch on landings and their GitHub checks. |
| 6 operations | north-graph-ingest | Load what north already records into the fact graph. |
| 6 operations | north-core | The agent catalog and policy contract, and grounding, the crate-split tool, build time, Nix upkeep, the contract probe and proxy calls. |
| 7 command | north-tui | The fleet terminal screen, input, transcript and digest views. |
| 7 command | north-monitor | One live terminal screen of fleet, landing queue, runtime versions and health. |
| 7 command | north | Command routing and the composition root, and the dash report, launch admission, the eval runner and verify-north. |

## Owning boundary

A keystroke in the fleet screen reaches a session by one path: terminal input, fleet screen state, an authorized fleet operation, the session runtime, transcript and usage events, and back to the screen as a projection. `crates/north-fleet/src/fleet/` owns sessions, delivery, decisions and the runtime boundary. `crates/north-tui/src/tui/fleet/` owns the terminal loop, transcript, launch form and fleet digest. `crates/north-providers/src/providers/` adapts provider protocols behind the fleet runtime. `src/cli/` wires commands and provider implementations. The run ledger records session outcomes and task verdicts; it is not a second session authority. A bare `north` follows the same path as `north fleet`.

## The life of a task

1. **Brief.** `north work brief` composes the text in north-dispatch from the vocabulary in north-pace and the pack in north-context-pack, over the board in north-threads.
2. **Launch.** `north fleet launch` is parsed in `src/cli/fleet/launch.rs`. north-pacing admits and places it. north-fleet-supervisor records the start, north-providers builds the provider's command and north-session-host holds the process.
3. **Lane.** north-lanes creates the worktree and the worker sandbox confines the session to it.
4. **Tool calls.** north-guard judges each tool call before it runs (`crates/north-guard/src/hook.rs`).
5. **Finish call.** The finish tool reaches `src/cli/remote_land.rs`, which wires the road into north-lander's hand-in (`crates/north-lander/src/land/hand_in.rs`) and north-safe-push.
6. **Land check.** north-safe-push runs the rules over the lane (north-land-scans, north-diff-gates, north-landing-gates, north-code-health); north-land-checks runs the check slots inside north-land-sandbox.
7. **Lander.** north-lander queues the lane, lands batches and promotes; north-landing holds the queue and the steps after a landing.
8. **Runtime order.** north-runtime builds the landed commit, runs the canary, writes the pin and goes live (`crates/north-runtime/src/runtime/order.rs`); long-lived processes follow through north-runtime-follow.
9. **Records.** The run's ledger row, its thread and its facts are written by the records layer and read by operations and by the next dispatch.

## Session scopes

Each fleet session owns a `north-session-<uuid>.scope` in the user systemd manager, including its provider host and process descendants. Codex sessions have separate app-servers so stopping one cannot terminate another. The host records its PID separately from the scope runner; its directory retains the scope name in `host.scope` after the host exits. Stop and forget synchronously stop that scope before completing; revive clears the previous scope first.

Launchers that create additional systemd units use `north session-unit` with the usual `systemd-run` arguments. This Rust adapter binds the unit to the inherited `NORTH_FLEET_SCOPE` with `BindsTo` and `After`, so capacity jobs and private desktop clients end with their session even after reparenting. `north capacity bounded` applies the same dependency to its bounded scope.

## The rules

- A crate depends only on crates in lower layers, or on crates of its own family in its own layer. Dev and build dependencies count.
- Mechanism families meet through a record one writes and another reads, or are wired together in orchestration or the command layer. A lower layer that needs something from above declares a port and the layer above implements it.
- One crate, one responsibility: its row in the table needs no "and". A crate that grows a second one is split.
- One concept lives in one crate. A second copy is a move, not a new type.
- A file stays under 500 lines and a function under 60 lines and complexity 15. Standing allowances are a count that only falls.
- A new crate, verb or gate lands with its metric, its bar and its review date in `docs/rationale/experiments.toml`.
- The table above lists every workspace member with its declared layer. `north code-health layers` and `north code-health habits` check these rules at landing; the checks that are not yet built are boxes L01 to L05 of the rationale note.
