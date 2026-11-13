# Architecture layers: the map, the target and the boxes that close the gap

Every north crate sits in one of seven layers with one responsibility each, dependencies point down, and each place the code departs from that is a box on north#1276. Decided 2026-11-12 under north#1276, whose direction is to make the design legible layer by layer: rewrite where the design is wrong, split crates and modules harder, and keep the result true by checks at landing and not by review. This note is the plan; it changes no code. [ARCHITECTURE.md](../../ARCHITECTURE.md) at the repository root is the two-page form a reader starts from.

All figures are read at commit 15f0a364a: 65 packages (the root package and 64 crates), 283,366 lines of product Rust and 58,833 lines of test Rust in 2,651 files, joined by 263 normal, 15 dev and 1 build dependency between workspace members.

## The map as it is

Each crate is placed in the lowest layer that lets every one of its dependencies point down. "Uses / used by" counts direct normal dependencies on workspace members. Churn is lines added plus deleted in the seven days before the reading. The public surface is a text estimate (see the last section). A crate marked (and) has a purpose that cannot be said without an "and".

### Layer 1: platform

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-foundation |  | 0 / 1 | 1,161 | 51 | 72 / 75 | The error type, the clock port and time values, and identifiers. |
| north-base (and) |  | 3 / 49 | 11,912 | 15,685 | 411 / 548 | Adapters to the outside world (git, GitHub, processes, SQLite, ssh, Nix), the shell lexer, the lifecycle store and the verdict cache, and records that belong to layers above it. |
| north-strategy-types |  | 0 / 10 | 740 | 802 | 43 / 51 | The decision records the kernel writes and strategies read: one Case and one Decision per decision kind. |
| north-shared-config |  | 0 / 2 | 216 | 664 | 0 / 17 | The shared configuration a host reads, fetched into the state directory with its freshness bound. |
| north-publish |  | 0 / 2 | 28 | 28 | 1 / 1 | Create a file whole in one step, so no reader sees part of it. |
| north-registries |  | 0 / 3 | 114 | 114 | 4 / 4 | Read a generated registry's per-unit source tree and render the registry. |
| north-cli-reference |  | 0 / 3 | 59 | 7 | 3 / 4 | The command reference text and the calls it names. |
| north-git-locks |  | 0 / 4 | 157 | 599 | 3 / 5 | Stale git lock files: the one removal an agent may make in a primary checkout's git directory. |
| north-runtime-follow |  | 0 / 7 | 120 | 148 | 6 / 7 | How a long-lived process moves onto the runtime its link promotes. |
| north-ssh-spare |  | 0 / 2 | 269 | 365 | 2 / 5 | A spare ssh connection held ready and the framed run over it. |
| north-build-system |  | 1 / 5 | 549 | 484 | 15 / 17 | The one interface north builds, lists and runs Rust tests through, with its Cargo and Buck2 sides. |
| north-buck |  | 0 / 1 | 1,077 | 1,077 | 6 / 28 | The Buck2 targets of the workspace crates, written from cargo metadata. |
| north-test-cases |  | 0 / 0 | 172 | 172 | 1 / 8 | Run one libtest binary's cases one process each and print a status line per case. |
| north-broker |  | 2 / 1 | 1,163 | 2,284 | 4 / 29 | The credential broker: a policy ceiling per user and scoped tokens swapped into allowlisted calls. |
| north-machine-model |  | 1 / 1 | 874 | 2,208 | 5 / 12 | Model checking of lifecycle machines against the real lifecycle store. |

### Layer 2: records

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-threads |  | 3 / 12 | 7,795 | 14,316 | 108 / 222 | The threads ledger (items, holders, runs, messages, jobs) and the board read from GitHub. |
| north-fleet-store |  | 3 / 2 | 3,092 | 1,911 | 89 / 126 | The fleet's stored rows: alarms, gates, held messages, pages and the session store. |
| north-graph |  | 0 / 6 | 4,339 | 5,508 | 24 / 95 | The fact graph: terms, assertions with their writers, the five verbs and the Datalog engine. |
| north-graph-loops |  | 2 / 10 | 775 | 1,142 | 13 / 65 | The learning loops' facts in the graph and the readers every decision site uses. |
| north-graph-ground |  | 2 / 5 | 895 | 1,158 | 1 / 36 | Read the fact graph around the issues and files a message names. |
| north-graph-change |  | 3 / 1 | 2,248 | 2,655 | 15 / 66 | The change queries the lander asks of the graph: affected tests, shared tests, blame, splits. |
| north-github-mirror |  | 1 / 8 | 634 | 1,288 | 34 / 60 | The local copy of GitHub issues, CI runs and check results that pollers read. |
| north-coverage |  | 2 / 5 | 1,774 | 3,729 | 23 / 74 | The map from each Rust test to the functions it executes and the tests that reach a change. |

### Layer 3: rules

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-pace (and) |  | 1 / 13 | 7,387 | 16,258 | 134 / 240 | Account usage readings, the router's tier, price and brief vocabulary, and the spend and credit books. |
| north-calibration (and) |  | 3 / 10 | 3,213 | 687 | 25 / 93 | Calibration of estimates against run records, and a graft that re-exports the threads ledger under its own name. |
| north-capacity |  | 2 / 4 | 5,908 | 5,614 | 140 / 256 | Machine capacity leases and admission. |
| north-code-health |  | 4 / 3 | 4,510 | 2,150 | 5 / 12 | The code-health report, its gates, the layers check and the Rust habit checks. |
| north-implicit-state |  | 1 / 1 | 785 | 936 | 3 / 10 | The implicit-state shapes: rules that find state kept outside a typed lifecycle. |
| north-diff-gates |  | 1 / 4 | 3,503 | 1,869 | 21 / 33 | The gates that judge a diff: dependencies, language, provenance, build cache, stubs. |
| north-landing-gates |  | 4 / 5 | 6,700 | 3,294 | 74 / 141 | The gates that judge an issue or a lane: box lint, follows-release, abandon, fails-first. |
| north-land-scans |  | 6 / 3 | 5,533 | 3,834 | 38 / 97 | The scans a landing runs over a tree: docs, generated files, policy, test admission. |
| north-check-environment |  | 1 / 4 | 118 | 339 | 6 / 7 | Which check environment a landing runs in and the credential it may hold. |
| north-concerns |  | 1 / 3 | 832 | 288 | 23 / 47 | Each project's concerns as named checks with a bound judged on each measured value. |
| north-mutants |  | 1 / 3 | 1,114 | 304 | 20 / 51 | Mutation testing of a repository's tests: recorded mutants and the runs that verify them. |
| north-guard |  | 12 / 1 | 20,443 | 24,808 | 44 / 246 | Tool-call policy: every guard and the hook that dispatches a tool call to them. |
| north-context-pack |  | 3 / 1 | 356 | 408 | 1 / 8 | The context pack a brief may carry for each file a box names. |

### Layer 4: mechanisms

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-claude-stream | sessions | 0 / 3 | 344 | 4 | 8 / 20 | Claude Code's stream-json wire format: encoders and the event decoder. |
| north-session-host | sessions | 2 / 3 | 1,536 | 2,628 | 29 / 42 | The process that hosts one session: its pipes, sent log and frames. |
| north-resume | sessions | 1 / 2 | 116 | 128 | 11 / 11 | The messages a resumed session opens with and whether it may resume. |
| north-fleet | sessions | 7 / 5 | 8,020 | 16,045 | 249 / 308 | Headless agent sessions: the session model, the delivery journal, turns, tickets and the watch. |
| north-fleet-supervisor | sessions | 6 / 7 | 7,964 | 6,354 | 60 / 110 | The session registry and supervisor: start, hand-in, handover. |
| north-providers (and) | sessions | 7 / 8 | 9,606 | 10,526 | 82 / 231 | The Claude, Codex, OpenCode, pi and agy adapters, and the run ledger. |
| north-lanes | workspace | 3 / 4 | 7,415 | 5,258 | 3 / 27 | Worktree and pin lifecycle: new, reap, rescue, gc, layout checks. |
| north-land-sandbox | landing | 1 / 1 | 1,087 | 1,640 | 5 / 46 | The sandbox a land check runs in: fence, recorder, supervisor. |
| north-land-checks | landing | 5 / 1 | 6,890 | 5,897 | 70 / 170 | The check slots of a landing: select, run, cache. |
| north-landing (and) | landing | 11 / 4 | 7,912 | 50,737 | 155 / 321 | The landing queue and the lander's state, the steps after a landing, and the sheriff. |
| north-lander | landing | 9 / 3 | 6,222 | 11,773 | 36 / 153 | The lander service: enqueue, serve batches, promote, and the hand-in road. |
| north-safe-push | landing | 3 / 1 | 4,085 | 5,735 | 19 / 130 | The safe-push entry: request, gates, publication, restore. |
| north-capacity-host (and) | capacity | 7 / 5 | 9,087 | 7,023 | 16 / 264 | Host leases, the dependency box, the watchdog and the PATH gate. |

### Layer 5: orchestration

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-pacing (and) |  | 8 / 1 | 4,120 | 2,331 | 62 / 114 | Placement of work on hosts and accounts, laptop state and the resident cap. |
| north-dispatch |  | 13 / 6 | 7,971 | 25,984 | 41 / 363 | The dispatch decision: which box runs next, on which tier, with which brief. |
| north-runtime (and) |  | 5 / 1 | 17,584 | 37,110 | 53 / 184 | Runtime update, canary, order and pin, and the worker sandbox, the offload, the timers and the privacy judge. |

### Layer 6: operations

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-tune |  | 6 / 1 | 1,543 | 465 | 9 / 70 | Tuned values and the learning-loop checks. |
| north-replay |  | 5 / 1 | 732 | 1,292 | 2 / 23 | Strategy replay: logged decision cases decided again by the linked code. |
| north-flow (and) |  | 10 / 1 | 9,960 | 3,436 | 48 / 321 | The flow report, and agents, detectors, experiments, tempo and tool mining. |
| north-ops (and) |  | 14 / 1 | 16,767 | 24,825 | 83 / 374 | Thirty-eight unrelated operations verb families, from docs indexes to desktop upkeep. |
| north-health |  | 4 / 3 | 1,519 | 1,668 | 16 / 19 | Health facts: failed starts, flows, issues, loops, red main, rework. |
| north-convo (and) |  | 3 / 1 | 7,338 | 1,509 | 12 / 92 | Transcript search, and policy replay, and the context audit. |
| north-lander-doctor |  | 4 / 1 | 2,195 | 3,410 | 9 / 94 | The lander doctor: each health bound read live and the remedy for each stuck state. |
| north-land-watch |  | 3 / 1 | 880 | 352 | 3 / 37 | The watch on landings and their GitHub checks. |
| north-graph-ingest |  | 6 / 1 | 1,594 | 2,484 | 6 / 33 | Load what north already records into the fact graph. |
| north-core (and) |  | 11 / 1 | 14,949 | 24,233 | 31 / 220 | The agent catalog and policy contract, and grounding, the crate-split tool, build time, Nix upkeep, the contract probe and proxy calls. |

### Layer 7: command

| Crate | Family | Uses / used by | Product lines | 7-day churn | Public items used outside / declared | What it is for |
|---|---|---|---|---|---|---|
| north-tui |  | 2 / 1 | 2,845 | 2,595 | 6 / 52 | The fleet terminal screen, input, transcript and digest views. |
| north-monitor |  | 0 / 0 | 2,016 | 6,034 | 6 / 83 | One live terminal screen of fleet, landing queue, runtime versions and health. |
| north (and) |  | 44 / 0 | 20,504 | 83,703 | 22 / 281 | Command routing and the composition root, and the dash report, launch admission, the eval runner and verify-north. |

| Layer | Crates | Product lines | 7-day churn |
|---|---|---|---|
| 1 platform | 15 | 18,611 | 24,688 |
| 2 records | 8 | 21,552 | 31,707 |
| 3 rules | 13 | 60,402 | 60,789 |
| 4 mechanisms | 13 | 70,284 | 123,748 |
| 5 orchestration | 3 | 29,675 | 65,425 |
| 6 operations | 10 | 57,477 | 63,674 |
| 7 command | 3 | 25,365 | 92,332 |

Thirteen crates need an "and": north-base, north-pace, north-calibration, north-providers, north-landing, north-capacity-host, north-pacing, north-runtime, north-flow, north-ops, north-convo, north-core and the root package. Mechanisms hold a quarter of the code and the most churn of any layer; the command layer is 9% of the code and a fifth of the churn.

## The target design

Seven layers, bottom to top. A crate may depend on any crate in a lower layer, and on a crate in its own layer only inside its own family. Layer 4 has four families; every other layer is one family. Dev and build dependencies count the same as normal ones.

| Layer | One responsibility | What the layer above may call | May depend on |
|---|---|---|---|
| 1 platform | Touch the outside world and hold base values. It names no north noun from a layer above. | Typed adapters that return a value or an error (git, GitHub, processes, SQLite, ssh, Nix, files), the clock port, identifiers, the shell lexer, the lifecycle store, the build interface, the decision record types. | platform |
| 2 records | Remember. One crate per durable store: its schema, its one writer, its typed readers. It never decides. | Open a store, append one typed row, read typed rows or run one query. | platform, records |
| 3 rules | Decide. A verdict from records, a diff, a tree or a tool call, with no effect beyond reading. | A function from typed input to a typed verdict (allow or refuse with its reason, an estimate, a score), and the check commands a landing runs. | platform, records, rules |
| 4 mechanisms | Act. One family per kind of long-lived machinery: sessions, workspace, landing, capacity. Families never depend on each other; they meet through a record one writes and another reads, or are wired together above. | Start, advance or stop one thing (a session, a lane, a queue entry, a lease), and the ports a family needs filled from outside. | lower layers, own family |
| 5 orchestration | Choose what runs next, where, and on which build. The only layer where one function may call two mechanism families. | One decision or one order at a time: dispatch, place, order a runtime. | lower layers, orchestration |
| 6 operations | Watch, measure, tune, report and repair, on a timer or by hand. Nothing below depends on it, so removing an operations crate never breaks a task's path. | Verbs only. | lower layers, operations |
| 7 command | Parse arguments, wire ports to their implementations, print. No decision lives here. | The `north` command line and the terminal screens. | everything |

To find where a behaviour lives, ask what it does. It talks to something outside north: platform. It stores or reads back rows: records, in the crate named after the store. It says yes, no or how much: rules. It runs a session, a lane, a landing or a lease: the mechanism family of that name. It picks between tasks, hosts or builds: orchestration. It reports on or repairs any of the above: operations. It reads `argv` or draws a screen: command.

### One task through the layers

1. **Brief.** `north work brief` composes the text in `crates/north-dispatch/src/dispatch/brief.rs` (orchestration) from the vocabulary in `crates/north-pace/src/router/brief.rs` and the context pack in north-context-pack (rules), over the board in north-threads (records).
2. **Launch.** `north fleet launch` is parsed in `src/cli/fleet/launch.rs` (command). Admission and placement are an orchestration decision in north-pacing. The session family then acts: the registry in north-fleet-supervisor records the start, north-providers builds the provider's command, north-session-host holds the process.
3. **Lane.** north-lanes creates the worktree (`crates/north-lanes/src/lanes/new.rs`, workspace family) and the worker sandbox confines the session to it.
4. **Tool calls.** Each tool call the session makes is judged by north-guard through `crates/north-guard/src/hook.rs` (rules) before it runs.
5. **Finish call.** The finish tool reaches `src/cli/remote_land.rs` (command), which wires the road into `crates/north-lander/src/land/hand_in.rs` (landing family) and from there north-safe-push.
6. **Land check.** north-safe-push runs the rules over the lane: north-land-scans, north-diff-gates, north-landing-gates and north-code-health. The check slots of north-land-checks run inside north-land-sandbox, in the environment north-check-environment names.
7. **Lander.** `crates/north-lander/src/land/enqueue.rs` queues the lane, `crates/north-lander/src/land/serve.rs` lands batches and `crates/north-lander/src/land/promote.rs` promotes; the queue and the steps after a landing are north-landing (`crates/north-landing/src/land/queue.rs`, `crates/north-landing/src/land/after/`).
8. **Runtime order.** `crates/north-runtime/src/runtime/order.rs` (orchestration) builds the landed commit, runs the canary, writes the pin and goes live; long-lived processes move onto it through north-runtime-follow (platform).
9. **Records.** The run's ledger row, its thread and its facts are written by the records layer and read back by operations (north-flow, north-health, north-tune) and by the next dispatch.

## Where today's code departs

Ranked by how much each hides the design from a reader, hotspots first.

### 1. Dependencies that point up or sideways

Seven manifest edges break the rule; the other 272 already obey it.

| Edge | Kind | What crosses it | Box |
|---|---|---|---|
| north-threads -> north-pace | up | `CATEGORIES` and `FACETS`, the brief vocabulary, in two files | L12 |
| north-fleet-store -> north-pace | up | one type, the credit `Snapshot` | L12 |
| north-guard -> north-fleet-supervisor | up | 26 imports of session identity in 13 files: the session environment names, the session record and its reader | L13, L14, L15 |
| north-guard -> north-lanes | up | one call, the lane layout exemption | L16 |
| north-capacity-host -> north-fleet | sideways | two call sites: a session environment name and the record of a detached run | L15 |
| north-providers -> north-runtime (dev) | up | one test that needs the worker sandbox; it closes a cycle through north-dispatch and north-pacing | L18 |

The declared map does not see these. `north code-health layers` checks the references of changed files against `structure.toml`; it does not read manifests, so a dev dependency or a crate reached through a re-export is invisible to it (L02, L03). Five workspace members have no declared entry at all (the root package, north-health, north-monitor, north-registries, north-test-cases), and several entries still claim another crate's files by a glob written before that crate was split out (L01).

### 2. Hotspots

The ten files with the highest commits times complexity, from `north code-health report`:

| File | Score | What is wrong | Box |
|---|---|---|---|
| `crates/north-dispatch/src/dispatch/mod.rs` | 8,349 | a module root with 57 `mod` lines that also holds `decide` (76 lines, complexity 51) | L06 |
| `src/cli/scoreboard/ready.rs` | 4,653 | 590 lines; three subjects, four functions over the limit | L08 |
| `crates/north-runtime/src/runtime/update.rs` | 4,374 | no function over a limit; its score is churn from a crate with five jobs | L18 to L22 |
| `src/cli/fleet/router.rs` | 4,225 | fan-out 40, the tree's one allowed hotspot | L44 |
| `src/cli/tools.rs` | 3,920 | one of five routing schemes | L42 |
| `src/cli/fleet/placement.rs` | 3,885 | launch admission and placement decided in the command layer | L07 |
| `crates/north-dispatch/src/dispatch/survey.rs` | 3,792 | `survey` is 109 lines | L09 |
| `crates/north-guard/src/hook.rs` | 3,224 | fan-out 60; the guard list is code, not data | L10 |
| `src/cli/mod.rs` | 3,162 | `dispatch` is 81 lines at complexity 65 | L42 |
| `crates/north-threads/src/threads/client.rs` | 2,712 | the proxy transport sits in the client | L11 |

### 3. Crates with more than one responsibility

| Crate | What it holds | Verdict |
|---|---|---|
| north-runtime (17,584 lines) | runtime update and order (6,924), the worker sandbox (2,305), the offload (4,003), the timers and watches (3,208), the privacy judge (595) | split: L18 to L22 leave it with the runtime alone |
| north-core (14,949) | agent catalog and policy contract, grounding, the crate-split tool, build time, Nix upkeep, a contract probe, the proxy-call classifier; no module names another group's | dissolve: L23 to L27 |
| north-ops (16,767) | 38 verb families in one module | split the docs indexes (L28); the rest stays, see below |
| north-providers (9,606) | five provider adapters and the run ledger (2,546), which names nothing in the adapters | split: L29 |
| north-landing (7,912) | the queue and lander state, the steps after a landing, the sheriff, joined by a 27-file cycle | L30, L31 |
| north-base (11,912) | adapters, and the landing records of layers above (`land_ticket`, the land queue and notice, the verdict cache) | split: L48 |
| north-calibration (3,213) | calibration, and a graft of the threads ledger that ten crates depend on it for | L47 |
| root package (20,504) | routing and wiring, and the dash report (4,159), launch placement, the eval runner (2,131), verify-north | L07, L42 to L46 |
| north-pace (7,387) | usage readings, the router, spend, credit | keep whole: `usage` reads `router` and `credit`, so the split would point up; the value types others reach up for leave with L12 and the ladder is made one with L33 |
| north-capacity-host (9,087), north-pacing (4,120) | leases, dependency box, watchdog, PATH gate; placement, laptop state, resident cap | keep: each list answers one question (may this host take it; where may this run) |
| north-flow (9,960), north-convo (7,338) | a report and its satellites; transcript search, policy replay, context audit | keep for now: only the root depends on them, and every satellite of north-flow imports its `flow` module |

north-ops, north-flow and north-convo are operations crates that nothing below depends on, so their mixing costs no reader of a task's path. They are read again on the review date of the lines below.

### 4. Sizes and standing allowances

The report counts 25 files over 500 lines, 77 functions over 60 lines and 593 functions over complexity 15 (670 functions over either limit), 46 module cycles, 38 layer findings and 2 unmapped files. The 38 layer findings are all a module naming its own crate root to reach a re-export; they go with L49.

Files over 500 lines:

| File | Lines | Box |
|---|---|---|
| `crates/north-diff-gates/src/dep_gate.rs` | 1,237 | L40 |
| `crates/north-base/src/platform/pyjson.rs` | 1,003 | in flight: the file is being removed |
| `crates/north-core/src/grounding/distill.rs` | 996 | L25 |
| `crates/north-diff-gates/src/language_gate.rs` | 908 | L40 |
| `crates/north-diff-gates/src/provenance.rs` | 853 | L40 |
| `crates/north-pacing/src/pacing/placement/gate.rs` | 690 | L39 |
| `crates/north-core/src/grounding/score.rs` | 686 | L25 |
| `crates/north-tui/src/tui/fleet/screen.rs` | 637 | L39 |
| `crates/north-session-host/src/session_host.rs` | 630 | L37 |
| `crates/north-fleet/src/fleet/model.rs` | 628 | L14 |
| `src/cli/scoreboard/ready.rs` | 590 | L08 |
| `src/cli/cloud.rs` | 574 | L39 |
| `crates/north-lanes/src/lanes/gc/local.rs` | 574 | L39 |
| `crates/north-fleet-store/src/gates.rs` | 569 | L39 |
| `crates/north-dispatch/src/dispatch/deepseek/review.rs` | 567 | L39 |
| `crates/north-fleet-supervisor/src/fleet/registry/mod.rs` | 556 | L39 |
| `crates/north-pacing/src/pacing/placement/hosts.rs` | 549 | L39 |
| `crates/north-capacity/src/capacity/admission.rs` | 548 | L39 |
| `src/cli/fleet/placement.rs` | 545 | L07 |
| `crates/north-fleet/src/fleet/watch.rs` | 540 | L39 |
| `crates/north-core/src/grounding/sources/policy.rs` | 537 | L25 |
| `crates/north-threads/src/threads/snapshot.rs` | 533 | L39 |
| `crates/north-dispatch/src/dispatch/survey.rs` | 527 | L09 |
| `crates/north-core/src/grounding/store.rs` | 520 | L25 |
| `crates/north-capacity-host/src/capacity/host/run.rs` | 510 | L39 |

Functions. The guard crate holds 98 of the 670, north-core 62, the root 43, north-ops 35, north-base 32. The highest complexities are `Policy.scan_write_commands` at 208 (`crates/north-guard/src/guard/launch_critical/writes.rs`, L41), the pyjson decoder at 155 (in flight), the grounding functions at 108 to 127 (L25), `shell_substitutions` at 117 (`crates/north-base/src/shell/substitution.rs`), the three diff-gate parsers at 69 to 107 (L40) and `dispatch` at 65 (L42). `shell_substitutions` is kept until L41 has shown that the policy replay catches a changed verdict, because every guard reads through it. The rest are held by the existing gate, which refuses a lane that adds or grows one, and fall as their files are split.

The 17 standing `[[rust_habits.allow]]` entries:

| Entry | Check | Verdict |
|---|---|---|
| `crates/north-lander/src/land/serve.rs` (`Serve`), `crates/north-lander/src/land/serve/batch.rs` (`Batching`), `crates/north-lander/src/land/serve/round.rs` (`Pushing`) | single-impl-trait | fix (L32): the traits exist only because the `Lander` type is defined in the crate below |
| `crates/north-session-host/src/session_host.rs` (`SentLog`, `FrameWatch`) | single-impl-trait | fix (L37): make both concrete |
| `src/cli/fleet/window_move/steps.rs` (`Mover`) | single-impl-trait | fix: north#1238 removes it |
| `crates/north-flow/src/flow/jobs.rs`, `crates/north-flow/src/flow/measure.rs` | shared-mutability | fix (L38): take `&mut self` |
| `crates/north-fleet/src/fleet/books.rs` (`WatchBooks`, `FleetBooks`) | single-impl-trait | keep: ports with test doubles |
| `crates/north-landing-gates/src/land_abandon/scratch.rs` (`Witness`), `crates/north-concerns/src/history.rs` (`Store`) | single-impl-trait | keep: ports the layer above implements, which is the rule's own way to call upward |
| `crates/north-fleet-supervisor/src/fleet/test_support.rs` (`Internals`), `crates/north-fleet-supervisor/src/fleet/registry/fixtures.rs` | single-impl-trait, shared-mutability | keep: test seams |
| `crates/north-session-host/src/session_host.rs`, `crates/north-fleet-supervisor/src/fleet/registry/hand_in.rs`, `crates/north-fleet-supervisor/src/fleet/registry/start_lock.rs` | shared-mutability | keep: state shared between threads by design |
| `crates/north-threads/src/threads/board/issues.rs`, `src/bin/verify_north/notes.rs` | shared-mutability | keep: one SQLite connection, one stdin pipe |

Seventeen fall to ten. The 21 `[[implicit_state.allow]]` entries are counted with them by L04.

### 5. One concept in more than one crate

- The model ladder is written five times: `Tier` in `crates/north-pace/src/router/tier.rs`, two `Rung` types in `crates/north-dispatch/src/dispatch/tier.rs` and `crates/north-dispatch/src/dispatch/nearest/pick.rs`, `TIER_ORDER` and `RUN_TIERS` in `crates/north-calibration/src/threads/calibration/mod.rs`, `FAMILIES` in `crates/north-tune/src/tune/cells.rs` and again in `crates/north-pace/src/usage/meter.rs`; `family_of` is written twice (`crates/north-fleet-supervisor/src/fleet/handover.rs`, `src/cli/fleet/placement.rs`). L33, L34.
- Helpers that drive git inside tests are defined in 23 packages: the root 32 times, north-lander 11, north-land-scans 10, north-landing-gates 9, north-code-health 8. L35, L36.
- The JSON escapers are in flight under their own boxes of north#1276.

### 6. Interfaces that leak a lower layer upward

- Thirty-four crate roots hold 90 `pub use north_*` lines, so `crate::platform` or `crate::threads` resolves in a crate that does not own it and a reader cannot tell from a path which crate is meant. L49.
- Three grafts go further and merge another crate's module into their own: north-calibration's `threads`, the glob at the top of `crates/north-capacity-host/src/capacity/mod.rs` and the glob at the top of `crates/north-fleet-supervisor/src/fleet/mod.rs`. Ten crates depend on north-calibration, and two of them (north-guard, north-pacing) use nothing of it but the graft. L47.
- The platform holds records of the landing layer, which 19 packages then import as if they were adapters. L48.
- The `Lander` type is defined in north-landing while its behaviour is in north-lander, which forces three extension traits. L32.

### 7. The command layer

`src/cli/mod.rs` and `src/cli/tools.rs` route one `argv` through five schemes: an enum of 27 commands with its parser, a table of 15 forwarded verbs, an enum of 12 tools with tables of 51 uniform and 15 direct verbs and two more for fleet and operations, the session rule in `src/cli/session.rs`, and the refusals in `src/cli/moved.rs`. The root is also the composition root: `src/cli/remote_land.rs` and `src/cli/tools.rs` implement ports of the landing crates, and `src/main.rs` installs two process-wide hooks. About 290 functions across 40 crates take `&[String]` and return an exit code, each with its own usage text, so every layer parses arguments; that half is the argument-library box already in flight. L42 to L46.

## The boxes

Forty-nine boxes, about 73 worker-hours, in rank order: the checks that hold the rule, the hotspots, the seven edges, the crates with two jobs, then the rest. Each box stands alone. Tests of the crates a box touches are run on a build host through `north capacity offload run`. "Behavior: unchanged" means no output, exit code, record or refusal changes.

Two boxes may run at the same time when their Files share no file and neither is listed in the other's After. A lane that adds, moves or removes a crate reruns both generators (`structure.toml` and the Buck2 targets) on its last rebase and never merges a generated file by hand. A new crate lands with its entry in `docs/rationale/experiments.toml`, from the lines in the next section.

Four boxes cannot be 60 minutes or less, and each says why: L14, L31, L41, L48.

### The rule as a check

**L01 Check: every crate declares its layer.** 1.0 h. After: nothing.
- Files: the crate-dag parts under `structure/` (60 today, one per crate, such as `structure/0035-crate-dag-crate-north-safe-push.toml`); generated `structure.toml`; `crates/north-code-health/src/code_health/layers/mod.rs`.
- Change: each part gains `layer` and, in layer 4, `family`, from the map above. Five parts are added (root package, north-health, north-monitor, north-registries, north-test-cases). Globs that span crates are narrowed to the crate's own directory. The two unmapped files (`crates/north-base/src/json_text.rs`, `crates/north-base/src/store_pins.rs`) get a module.
- Behavior: `north code-health layers` also refuses a workspace member with no entry or no layer.
- Check: `rg -c '^layer = ' structure.toml` prints 65 and `north code-health report --json | jq '.unmapped | length'` prints 0.

**L02 Check: the report lists manifest edges that break the rule.** 0.75 h. After: L01.
- Files: a new file in `crates/north-code-health/src/code_health/layers/` and the report writer beside it.
- Change: read each workspace member's `Cargo.toml` (normal, dev and build dependencies on workspace members; no cargo call, no model call) and list each edge whose target is in a higher layer, or in the same layer and another family.
- Behavior: `north code-health report --json` gains `manifest_edges`; nothing is refused.
- Check: `north code-health report --json | jq '.manifest_edges | length'` prints 7 on the commit this note landed on.

**L03 Check: a lane that adds such an edge is refused (new gate).** 0.5 h. After: L02.
- Files: `crates/north-code-health/src/code_health/layers/`; `.safe-push` (the layers line also runs for a lane that touches `Cargo.toml`); `docs/rationale/experiments.toml`.
- Behavior: `north code-health layers BASE HEAD` exits non-zero and names each offending manifest edge at HEAD that BASE did not have. Edges already on main pass until their box lands, so there is no allowlist to grow.
- Check: in a scratch lane, adding north-pace to the dependencies of `crates/north-base/Cargo.toml` makes `north code-health layers origin/main HEAD` exit non-zero and print `north-base -> north-pace`; on main it exits 0.

**L04 Check: allowances only fall (new gate).** 0.5 h. After: nothing.
- Files: `crates/north-code-health/src/code_health/habits.rs` (`grown`), `crates/north-code-health/src/code_health/habits_tests.rs`, `docs/rationale/experiments.toml`.
- Behavior: the habits check a landing already runs with `--new-since` (`.safe-push` line 44; `--new-since` reaches `grown` through `findings_new`, `habits.rs` line 377) refuses a head whose `structure.toml` holds more `[[rust_habits.allow]]` or more `[[implicit_state.allow]]` entries than the base, printing both counts. `--base` alone does not reach it.
- Check: a scratch lane that adds one `[[rust_habits.allow]]` part is refused with `17 -> 18`; main passes.

**L05 Check: ARCHITECTURE.md equals the workspace (new gate).** 0.5 h. After: L03.
- Files: `crates/north-code-health/src/code_health/layers/`, `.safe-push`, `ARCHITECTURE.md`, `docs/rationale/experiments.toml`.
- Behavior: `north code-health layers BASE HEAD` refuses when the crate table of `ARCHITECTURE.md` and the workspace differ: a member with no row, a row that names no member, or a row whose layer is not the declared one.
- Check: deleting one row of the table in a scratch lane makes the command exit non-zero and name the crate; on main it exits 0.

### Hotspots

**L06 Split: `decide` leaves the dispatch module root.** 1.0 h. After: nothing.
- Files: `crates/north-dispatch/src/dispatch/mod.rs`; a new file for the decision beside it.
- Change: `decide` and `revive` become one function per stage of the decision in the new file; `mod.rs` keeps the module list, the usage text and argument routing.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '[.files["crates/north-dispatch/src/dispatch/mod.rs"].functions[] | select(.lines > 60 or .complexity > 15)] | length'` prints 0, and north-dispatch's tests pass.

**L07 Move: launch admission and placement leave the command layer.** 1.0 h. After: nothing.
- Files: `src/cli/fleet/placement.rs`; `crates/north-pacing/src/pacing/placement.rs` and a new file under `crates/north-pacing/src/pacing/placement/`.
- Change: `admit`, `place` and their helpers move into north-pacing; the command layer keeps argument parsing and printing.
- Behavior: unchanged.
- Check: `! rg -q 'fn (admit|place)\b' src/cli/fleet/placement.rs`, the file is under 150 lines, and the launch tests of the root and north-pacing's tests pass.

**L08 Split: the ready report by subject.** 0.75 h. After: nothing.
- Files: `src/cli/scoreboard/ready.rs` and three new files beside it (idle sessions, records, the bar).
- Behavior: unchanged; `north dash` prints the same text.
- Check: `north code-health report --json | jq '[.files | to_entries[] | select(.key | startswith("src/cli/scoreboard/ready")) | .value | select(.lines > 500 or any(.functions[]; .complexity > 15))] | length'` prints 0.

**L09 Split: `survey` into its gather steps.** 0.75 h. After: nothing (another file of the crate L06 edits).
- Files: `crates/north-dispatch/src/dispatch/survey.rs`.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '.files["crates/north-dispatch/src/dispatch/survey.rs"] | (.lines <= 500) and all(.functions[]; .lines <= 60)'` prints true.

**L10 REWRITE: the guard hook dispatches from one table.** 1.0 h. After: L17 (same file).
- Files: `crates/north-guard/src/hook.rs`.
- New shape:

  ```rust
  struct Guard { name: &'static str, kinds: &'static [HookKind], tools: Matcher, run: fn(&HookInput) -> Verdict }
  static GUARDS: &[Guard] = &[/* one row per guard, in today's order */];
  fn evaluate(input: &HookInput) -> Verdict {
      GUARDS.iter().filter(|guard| guard.applies(input)).map(|guard| (guard.run)(input)).find(Verdict::is_final).unwrap_or(Verdict::Allow)
  }
  ```

- What the old tests pin, and must stay: the order guards run in (the first refusal wins), which hook kinds and tools each guard sees, the text of each denial, and the exit code and JSON the hook prints. The hook tests of north-guard and `crates/north-land-checks/src/land/slots/guard_suites_tests.rs` pass unchanged.
- Behavior: unchanged.
- Check: the same `jq` query as L06 on `crates/north-guard/src/hook.rs` prints 0.

**L11 Split: the proxy transport leaves the threads client.** 0.5 h. After: nothing.
- Files: `crates/north-threads/src/threads/client.rs` and a new child module.
- Behavior: unchanged.
- Check: the same `jq` query as L06 on that file prints 0.

### The seven edges

**L12 Move: the brief vocabulary and the credit snapshot go below the stores.** 0.5 h. After: nothing. Removes two edges.
- Files: `crates/north-pace/src/router/brief.rs`, north-pace's `credit` module, north-strategy-types (which receives `CATEGORIES`, `FACETS` and `Snapshot`); `crates/north-threads/src/lib.rs`, `crates/north-threads/src/threads/runs/record.rs`, `crates/north-threads/src/threads/runs/brief.rs`, `crates/north-threads/Cargo.toml`; `crates/north-fleet-store/src/lib.rs`, `crates/north-fleet-store/src/store/remote.rs`, `crates/north-fleet-store/Cargo.toml`.
- Change: north-pace re-exports the three names so its other callers do not change.
- Behavior: unchanged.
- Check: `! rg -q 'north-pace' crates/north-threads/Cargo.toml crates/north-fleet-store/Cargo.toml`, and both crates' tests pass.

**L13 Move: session identity names go to the fleet store.** 0.5 h. After: nothing.
- Files: `crates/north-fleet/src/fleet/runtime.rs` (`SESSION_ENV`, `ROLE_ENV`, `Binding`, `binding`), `crates/north-fleet/src/fleet/tickets.rs` (`FINISH_TOOL`), `crates/north-fleet/src/fleet/claude_only.rs` (`DEEPSEEK_PROVIDERS`), `crates/north-fleet-store/src/runtime.rs` (which already holds `PROXY`).
- Change: the names move down; north-fleet re-exports them at their old paths.
- Behavior: unchanged.
- Check: `rg -c 'pub const (SESSION_ENV|ROLE_ENV)' crates/north-fleet-store/src/runtime.rs` prints 2.

**L14 Move: the session record and its reader go to the fleet store.** 1.5 h. After: L13.
- Files: `crates/north-fleet/src/fleet/model.rs`, `crates/north-fleet/src/fleet/records.rs`, `crates/north-fleet-supervisor/src/fleet/registry/home.rs`, a new `session` module in north-fleet-store.
- Change: `SessionRecord`, `Role`, the types their fields name, `read_record` and `root_here` move down in files under 500 lines; north-fleet re-exports the old paths.
- Behavior: unchanged.
- Check: `rg -l 'pub struct SessionRecord' crates` prints one path, in north-fleet-store, and `north code-health report --json | jq '.over.files'` is one lower than at the lane's base.
- Cannot be smaller: the record leaves with every type its fields name, and the file sits in north-fleet's 12-module cycle, so a partial move does not compile.

**L15 Move: the guard and the capacity host read sessions from the store.** 1.0 h. After: L14, L17. Removes two edges.
- Files: the 13 files of north-guard that import `crate::fleet`, `crates/north-guard/src/lib.rs`, `crates/north-guard/Cargo.toml`; `crates/north-capacity-host/src/lib.rs`, `crates/north-capacity-host/src/capacity/watchdog/owners.rs`, `crates/north-capacity-host/src/capacity/host/run.rs`, `crates/north-capacity-host/Cargo.toml`; `crates/north-fleet/src/fleet/waits.rs` (`Waits`), `crates/north-fleet/src/fleet/turns.rs` (`TurnExit`), `crates/north-fleet/src/fleet/detached.rs` (`Prior`), whose three records move down with L14's.
- Behavior: unchanged.
- Check: `! rg -q 'north-fleet' crates/north-guard/Cargo.toml` and `! rg -q 'north-fleet = ' crates/north-capacity-host/Cargo.toml`.

**L16 Move: the lane layout exemption goes to the platform.** 0.3 h. After: nothing. Removes one edge.
- Files: `crates/north-lanes/src/lanes/layout_check.rs` (`exempt`), `crates/north-lanes/src/lanes/mod.rs`, north-base's `platform`, `crates/north-guard/src/guard/launch_critical/git.rs`, `crates/north-guard/Cargo.toml`.
- Behavior: unchanged.
- Check: `! rg -q 'north-lanes' crates/north-guard/Cargo.toml`.

**L17 Move: the guard hands its evidence row to its caller.** 0.5 h. After: L16. Removes one edge.
- Files: `crates/north-guard/src/guard/native_evidence.rs` (it opens the run ledger; its `decide_with` already takes the write as a closure), `crates/north-guard/src/hook.rs`, `crates/north-guard/src/hook_input.rs`, `src/main.rs`, `crates/north-guard/Cargo.toml`.
- Change: the verdict carries the row and the hook's entry in the command layer writes it through `crates/north-providers/src/ledger/mod.rs`.
- Behavior: unchanged; the same `native-solo` event is written for the same command.
- Check: `! rg -q 'north-providers' crates/north-guard/Cargo.toml`, and north-guard's tests pass.

**L18 Split: the worker sandbox leaves north-runtime (new crate north-worker-sandbox, layer 4, workspace family).** 0.75 h. After: nothing. Removes the last edge and the cycle.
- Files: `crates/north-runtime/src/worker_sandbox/` (2,305 lines; no other module of the crate names it), `crates/north-runtime/src/lib.rs`, `crates/north-providers/tests/opencode_lane.rs`, `crates/north-providers/Cargo.toml`, `src/cli/remote_land.rs`, `src/main.rs`, a new crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Change: `north code-health split move-module --from north-runtime --crate north-worker-sandbox worker_sandbox`.
- Behavior: unchanged.
- Check: `! rg -q 'north-runtime' crates/north-providers/Cargo.toml` and `test -d crates/north-worker-sandbox`.

### Crates with two jobs

**L19 Move: the runtime tree's layout becomes a leaf.** 0.75 h. After: L18.
- Files: `crates/north-runtime/src/runtime/layout.rs`, `crates/north-runtime/src/runtime/update/collect.rs`, `crates/north-runtime/src/runtime/mod.rs` (`own_runtime`, `stage_on`), north-runtime-follow.
- Change: the layout, `collect` and `own_runtime` move to north-runtime-follow; `stage_on` is passed to the offload by its caller.
- Behavior: unchanged.
- Check: `! rg -q 'crate::runtime::' crates/north-runtime/src/land_offload crates/north-runtime/src/build_offload crates/north-runtime/src/offload_cache`.

**L20 Split: the offload leaves north-runtime (new crate north-offload, layer 5).** 0.75 h. After: L19, and the Buck2 remote-execution work in flight.
- Files: `crates/north-offload/src/land_offload/`, `crates/north-offload/src/build_offload/`, `crates/north-offload/src/offload_cache/` (4,003 lines), `crates/north-runtime/src/lib.rs`, `crates/north-runtime/src/runtime/mod.rs`, a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `test ! -e crates/north-runtime/src/land_offload && test -d crates/north-offload`.

**L21 Split: the timers leave north-runtime (new crate north-timers, layer 6).** 0.75 h. After: L20.
- Files: `crates/north-timers/src/timers/`, `crates/north-timers/src/outside_watch/`, `crates/north-timers/src/job_watch/`, `crates/north-timers/src/usage_web_read/`, `crates/north-timers/src/runtime_access.rs`, `crates/north-runtime/src/lib.rs`, a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `test ! -e crates/north-timers/src/timers && test -d crates/north-timers`.

**L22 Move: the privacy judge becomes a land scan with its model call passed in.** 1.0 h. After: L21.
- Files: `crates/north-timers/src/privacy/` (595 lines), north-land-scans, `src/cli/tools.rs`, `crates/north-land-checks/src/land/slots/guard_suites_tests.rs`.
- Change: the judge's one provider call becomes a parameter the command layer supplies, so the judge is a rule.
- Behavior: unchanged.
- Check: `test ! -e crates/north-timers/src/privacy`, and north-runtime's product lines are under 8,000.

**L23 Move: the crate-split tool and the build-time reading join code health.** 0.5 h. After: nothing.
- Files: north-core's `split` and `build_time` modules, `crates/north-core/src/lib.rs`, `crates/north-code-health/src/lib.rs`, `crates/north-code-health/Cargo.toml`.
- Behavior: unchanged.
- Check: `! rg -q 'pub mod (split|build_time)' crates/north-core/src/lib.rs`.

**L24 Split: agent policy leaves north-core (new crate north-agent-policy, layer 3).** 0.75 h. After: L23, and the JSON escaper boxes in flight.
- Files: north-core's `agent_catalog`, `agent_policy_contract` and `agent_config_check` modules (about 6,500 lines), `crates/north-core/src/lib.rs`, a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `! rg -q 'pub mod agent_' crates/north-core/src/lib.rs && test -d crates/north-agent-policy`.

**L25 Merge: brief grounding has one home (new crate north-grounding, layer 3, replacing north-context-pack).** 1.0 h for the move, then four units of 0.5 h at the same time. After: L24, and the brief-grounding step in flight.
- Files: `crates/north-core/src/grounding/` (about 5,300 lines), `crates/north-context-pack/`, a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Change: both move into one crate; then `distill.rs`, `score.rs`, `sources/policy.rs` and `store.rs` are each cut under 500 lines, one unit per file.
- Behavior: unchanged.
- Check: `test ! -e crates/north-context-pack && test ! -e crates/north-core/src/grounding`, and the report lists no file of the new crate over 500 lines.

**L26 Move: the last modules of north-core go to their users.** 0.5 h. After: L24.
- Files: north-core's `nix_gc` and `doctor_nix` (to north-ops), `crates/north-core/src/contract_probe.rs` (to north-providers, beside the Claude adapter), `crates/north-core/src/proxy/` (to north-guard, whose delegation guard it serves).
- Behavior: unchanged.
- Check: `! rg -q '^pub mod' crates/north-core/src/lib.rs` once L25 has landed.

**L27 Merge: north-core is removed.** 0.3 h. After: L23 to L26.
- Files: `crates/north-core/`, every manifest that names it, its crate-dag part, the generated files.
- Check: `test ! -e crates/north-core`.

**L28 Split: the docs indexes leave north-ops (new crate north-docs, layer 6).** 0.75 h. After: nothing.
- Files: `crates/north-ops/src/ops/docs.rs`, `crates/north-ops/src/ops/rationale_index/` and the `registry`, `features_index`, `help_index`, `reference_index`, `structure_index` and `verbs_index` modules beside them (one 7-module cycle that moves whole); `.safe-push`, whose docs lines name two of these paths; a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `north docs rationale-index --check` exits 0 and `test ! -e crates/north-ops/src/ops/rationale_index`.

**L29 Split: the run ledger leaves north-providers (new crate north-run-ledger, layer 4, sessions family).** 0.75 h. After: L18.
- Files: `crates/north-providers/src/ledger/` (2,546 lines; it names the board, the fleet, the router and the graph and nothing in the adapters), `crates/north-providers/src/lib.rs`, `crates/north-providers/Cargo.toml`, a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `test ! -e crates/north-providers/src/ledger && test -d crates/north-run-ledger`.

### Landing

**L30 Move: the issue body becomes a leaf of north-landing.** 1.0 h. After: north#1195's split of `crates/north-landing/src/land/lander.rs`.
- Files: `crates/north-landing/src/issue_body.rs` and the files of `crates/north-landing/src/land/after/` that import it or that it imports.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '[.cycles[] | select(index("crates/north-landing/src/issue_body.rs"))] | length'` prints 0.

**L31 Split: the steps after a landing stop forming one cycle.** 2.0 h. After: L30.
- Files: `crates/north-landing/src/land/after/` (26 files).
- Change: `mod.rs` keeps the order of the steps; gates, clean-up and tick become three modules that take their inputs from it and do not import each other.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '[.cycles[] | select(any(.[]; startswith("crates/north-landing/src/land/after/"))) | length] | max // 0'` prints at most 5.
- Cannot be smaller: the 26 files are one strongly connected set, so the report does not move until the last back-reference is cut.

**L32 Merge: the lander's three extension traits become one wrapper type.** 0.75 h. After: nothing (north#1195 edits another file of the crate).
- Files: `crates/north-lander/src/land/serve.rs`, `crates/north-lander/src/land/serve/batch.rs`, `crates/north-lander/src/land/serve/round.rs`, the three allow parts under `structure/`, generated `structure.toml`.
- Change: a type in north-lander that holds the `Lander` takes the three behaviours as its own methods. The `Lander` type stays in north-landing, which uses it in 15 files.
- Behavior: unchanged.
- Check: `! rg -q 'trait (Serve|Batching|Pushing)\b' crates/north-lander/src`.

### One concept, one place

**L33 Merge: one model ladder.** 1.0 h. After: L07, and the estimator switch in flight.
- Files: `crates/north-pace/src/router/tier.rs`, `crates/north-pace/src/usage/meter.rs`, `crates/north-calibration/src/threads/calibration/mod.rs`, `crates/north-tune/src/tune/cells.rs`, `crates/north-fleet-supervisor/src/fleet/handover.rs`, and the `family_of` L07 moved.
- Change: `Tier` becomes the one ordered ladder with its family, rank and provider; the other tables and both `family_of` functions read it. Where two tables disagree today the call site keeps its present result and the commit message lists the difference.
- Behavior: unchanged.
- Check: `! rg -q 'const (TIER_ORDER|RUN_TIERS|FAMILIES)\b' crates/north-calibration crates/north-tune` and `! rg -q 'fn family_of' crates src`.

**L34 Merge: one rung type in dispatch.** 0.75 h. After: L33.
- Files: `crates/north-dispatch/src/dispatch/tier.rs`, `crates/north-dispatch/src/dispatch/nearest/pick.rs`.
- Behavior: unchanged.
- Check: `rg '(enum|struct) Rung\b' crates/north-dispatch/src | wc -l` prints 1.

**L35 Merge: one git fixture for tests.** 0.75 h. After: L16.
- Files: `crates/north-lanes/src/lanes/test_git.rs`, `crates/north-lanes/src/lanes/mod.rs`, north-foundation's `test_inputs`, north-base's test files that define a git helper.
- Change: the helper becomes the one fixture (a scratch repository, a commit, a branch) in north-foundation.
- Behavior: unchanged; tests only.
- Check: `rg -l 'fn (git|init_repo)\(' -g '*tests.rs' -g '**/tests/**' crates/north-base | wc -l` prints 0.

**L36 Merge: each package's tests use the fixture.** One lane per package, 0.3 to 0.5 h each, all at the same time. After: L35.
- Files: the test files of one package that define a git helper: the root (32 definitions), north-lander (11), north-land-scans (10), north-landing-gates (9), north-code-health (8) and 17 more.
- Behavior: unchanged; tests only.
- Check: `rg -l 'fn git\(' -g '*tests.rs' -g '**/tests/**' PACKAGE_DIR | wc -l` prints 0 for the lane's package.

### Allowances

**L37 Fix: the session host's two traits become concrete.** 1.0 h. After: nothing.
- Files: `crates/north-session-host/src/session_host.rs`, its two allow parts under `structure/`, generated `structure.toml`.
- Change: `SentLog` and `FrameWatch` become types; the file is cut under 500 lines in the same lane.
- Behavior: unchanged.
- Check: `! rg -q 'trait (SentLog|FrameWatch)\b' crates/north-session-host/src`.

**L38 Fix: two shared cells in north-flow become `&mut self`.** 0.5 h. After: nothing.
- Files: `crates/north-flow/src/flow/jobs.rs`, `crates/north-flow/src/flow/measure.rs`, their two allow parts, generated `structure.toml`.
- Behavior: unchanged.
- Check: `! rg -q 'crates/north-flow/src/flow/(jobs|measure)\.rs' structure.toml`.

### Files and functions

**L39 Split: twelve files over 500 lines.** One lane per file, 0.5 h each, all at the same time. After: L07 for the two files of north-pacing.
- Files, one per lane: `crates/north-pacing/src/pacing/placement/gate.rs`, `crates/north-pacing/src/pacing/placement/hosts.rs`, `crates/north-tui/src/tui/fleet/screen.rs`, `src/cli/cloud.rs`, `crates/north-lanes/src/lanes/gc/local.rs`, `crates/north-fleet-store/src/gates.rs`, `crates/north-dispatch/src/dispatch/deepseek/review.rs`, `crates/north-fleet-supervisor/src/fleet/registry/mod.rs`, `crates/north-capacity/src/capacity/admission.rs`, `crates/north-fleet/src/fleet/watch.rs`, `crates/north-threads/src/threads/snapshot.rs`, `crates/north-capacity-host/src/capacity/host/run.rs`.
- Change: cut by subject into files under 500 lines.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '.files["PATH"].lines'` prints at most 500 for the lane's file and `.over.files` is one lower than at the lane's base.

**L40 Split: the three diff-gate files.** One lane per file, 1.0 h each, all at the same time. After: nothing.
- Files: `crates/north-diff-gates/src/dep_gate.rs`, `crates/north-diff-gates/src/language_gate.rs`, `crates/north-diff-gates/src/provenance.rs`.
- Change: each parser (`dependencies`, `shell_targets`, `new_rules`) is cut into its steps, then the file by subject.
- Behavior: unchanged; every refusal text is pinned by the crate's tests.
- Check: as L39.

**L41 REWRITE: `scan_write_commands`.** 1.5 h. After: nothing.
- Files: `crates/north-guard/src/guard/launch_critical/writes.rs`.
- New shape:

  ```rust
  struct Writer { programs: &'static [&'static str], targets: fn(&Invocation) -> Vec<Target> }
  static WRITERS: &[Writer] = &[/* one row per way a command writes: redirect, tee, copy, in-place edit, git */];
  fn scan_write_commands(&self, segments: &[Segment]) -> Vec<Finding> {
      segments.iter()
          .flat_map(|segment| WRITERS.iter().filter(|writer| writer.matches(segment)).flat_map(move |writer| (writer.targets)(segment.invocation())))
          .filter_map(|target| self.judge(&target))
          .collect()
  }
  ```

- What the old tests pin, and must stay: every case of the launch-critical tests (each form of write refused or allowed, with the same denial text), and the same verdicts from `north policy-replay` over its recorded scenarios.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '[.files["crates/north-guard/src/guard/launch_critical/writes.rs"].functions[] | select(.complexity > 15)] | length'` prints 0.
- Cannot be smaller: one function of complexity 208 is the unit; half a table does not compile.

### The command layer

**L42 REWRITE: one verb table.** Three stages on the same files, in order: 1.0 h, 1.0 h, 0.75 h. After: the argument-library box of north#1276 has landed its first converted crate.
- Files: `src/cli/mod.rs`, `src/cli/tools.rs`, `src/cli/tools/subverbs.rs`, `src/cli/session.rs`, `src/cli/moved.rs`.
- New shape:

  ```rust
  struct Verb { path: &'static [&'static str], run: fn(&[String]) -> u8, session: Session, moved_to: Option<&'static str> }
  static VERBS: &[Verb] = &[/* one row per verb, generated from the per-verb files that already feed src/cli/verbs.rs */];
  fn dispatch(arguments: &[String]) -> u8 {
      match longest_prefix(VERBS, arguments) {
          Some((verb, _)) if verb.moved_to.is_some() => moved::refusal(verb),
          Some((verb, rest)) => verb.session.enter().map_or_else(report, |_| (verb.run)(rest)),
          None => usage(arguments),
      }
  }
  ```

- Stages: (a) the table type and the 27 commands; (b) the tool tables folded in; (c) the session rule and the moved refusals become columns and the four old schemes are deleted.
- What the old tests pin, and must stay: the text of `north --help` and `north --help-all`, every verb's usage text and exit code, each moved refusal, and the two entry points chosen by the program name.
- Behavior: unchanged.
- Check: `north --help-all` from the lane's build is byte-equal to the base's, and the `jq` query of L06 on `src/cli/mod.rs` prints 0.
- Cannot be one 60-minute box: five schemes read the same arguments, and the help text must be whole after each stage.

**L43 Move: the composition root is one module.** 1.0 h. After: L42.
- Files: `src/main.rs`, `src/cli/remote_land.rs`, `src/cli/tools.rs`, a new wiring module.
- Change: every implementation of a lower crate's port (the hand-in road, the issue-body ledger, the abandon witness) and the two process-wide installs move into it and nothing else lives there.
- Behavior: unchanged.
- Check: `rg -l '^impl .*(Road|Ledger|Witness) for ' src` prints one path.

**L44 Move: the fleet router becomes rows of the table.** 0.75 h. After: L42.
- Files: `src/cli/fleet/router.rs`.
- Behavior: unchanged.
- Check: `north code-health report --json | jq '[.allowed_debt[] | select(tostring | test("fleet/router.rs"))] | length'` prints 0.

**L45 Split: the dash report leaves the root (new crate north-dash, layer 6).** 1.0 h. After: L08, L42.
- Files: `src/cli/scoreboard/`, `src/cli/dash/`, `src/cli/views/` (4,159 lines), a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged; `north dash` and `north dash --weekly` print the same text.
- Check: `test ! -e src/cli/scoreboard && test -d crates/north-dash`.

**L46 Split: the eval runner leaves the root package (new crate north-evals, layer 7).** 0.5 h. After: nothing.
- Files: `evals/` (2,131 lines), the root `Cargo.toml`, a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `cargo metadata --format-version 1 --offline --no-deps | jq '[.packages[] | select(.name == "north") | .targets[].src_path | select(test("/evals/"))] | length'` prints 0.

### Leaks, last

These touch many crates at once, so each lands after the boxes above that move files in the crates it edits.

**L47 Move: the three grafts go.** Three units in order, 1.0 h each. After: L15.
- (a) `crates/north-calibration/src/lib.rs`: callers import ledger items from north-threads; north-guard and north-pacing drop north-calibration from their manifests. Check: `! rg -q 'north-calibration' crates/north-guard/Cargo.toml crates/north-pacing/Cargo.toml`.
- (b) `crates/north-capacity-host/src/capacity/mod.rs`: callers name the crate that owns each item. Check: `! rg -q 'pub use north_capacity::capacity::\*' crates/north-capacity-host/src`.
- (c) `crates/north-fleet-supervisor/src/fleet/mod.rs` and the re-export of north-pacing's modules in `crates/north-dispatch/src/lib.rs`. Check: `! rg -q 'pub use north_fleet::fleet::\*' crates/north-fleet-supervisor/src`.
- Behavior: unchanged.

**L48 Split: the landing records leave the platform (new crate north-land-records, layer 2).** 1.5 h. After: L30 to L32.
- Files: north-base's `land_ticket`, `autoland_status` and `verdict_cache` modules, `crates/north-base/src/platform/land_queue.rs`, `crates/north-base/src/platform/land_notice.rs`; the imports of the 19 packages that use them; a crate-dag part, the generated files, `docs/rationale/experiments.toml`.
- Behavior: unchanged.
- Check: `test ! -e crates/north-base/src/platform/land_queue.rs && test -d crates/north-land-records`.
- Cannot be smaller: the platform cannot re-export a crate above it, so all 19 packages change their imports in one landing.

**L49 Move: the plain re-exports go, one crate per lane.** 1.0 h for the first lane, 0.3 h for each of the other 33, all at the same time. After: every box above that moves files in the lane's crate.
- Files: one crate's root file, its manifest and its own source files.
- Change: a scratch script that is not landed rewrites the crate's paths to name the owning crate (`north_base::platform` for `crate::platform`) and adds the direct dependency where one was only reached through a re-export.
- Behavior: unchanged.
- Check: `! rg -q '^pub use north_' crates/CRATE/src/lib.rs` for the lane's crate; after the last lane, `north code-health report --json | jq '.layer_violations | length'` prints 0.

### What can start at once

No box in this list waits on another: L01, L04, L06, L07, L08, L09, L11, L12, L13, L16, L18, L23, L28, L32, L37, L38, L40, L41, L46 and ten of L39's twelve lanes. The chains behind them are L01 to L02 to L03 to L05; L13 to L14 to L15; L16 to L17 to L10 and L15; L18 to L19 to L20 to L21 to L22; L23 to L24 to L26 to L27; L35 to L36; L07 to L33 to L34. L42 and the boxes after it wait for the argument library; L25 waits for the brief-grounding step; L30 waits for north#1195.

## How the design stays true

A reviewer is not the control. Each rule is a check a landing runs, read from manifests and `structure.toml`, with no model call.

| Rule | Check | State |
|---|---|---|
| A changed file's references stay inside what its module and crate may use | `north code-health layers BASE HEAD`, run by `.safe-push` | exists |
| A new crate has a declared entry; the declared crate graph has no cycle | the same check | exists |
| Every workspace member has an entry with a layer | the same check | new: L01 |
| No manifest edge points up or sideways, dev and build edges included | the same check | new: L02, L03 |
| The crate table of ARCHITECTURE.md equals the workspace members and their layers | the same check | new: L05 |
| A crate does not grow past its line cap | `north code-health crate-lines` | exists |
| No new file or function over a limit, no new habit finding | `north code-health gate` and `north code-health habits` | exists |
| The three ambient test allowlists only shrink; a stale allowance is reported | `north code-health habits` | exists |
| The count of habit and implicit-state allowances only falls | `north code-health habits --new-since BASE` | new: L04 |

The lines for what this plan adds, as `docs/rationale/experiments.toml` asks of every new gate and crate. Each lands with the box that adds it, and a crate that misses its bar on its review date is merged back.

| Added | Metric | Bar | Review |
|---|---|---|---|
| The manifest layer rule (L03) | manifest edges on main that point up or sideways | 7 on 2026-11-12; never higher at a landing; 0 at the review | 2026-11-26 |
| Allowances only fall (L04) | standing habit and implicit-state allowances | 38 on 2026-11-13 (17 and 21); never higher; habit allowances at most 10 at the review | 2026-11-26 |
| The ARCHITECTURE.md check (L05) | table rows that disagree with the workspace | 0 at every landing | 2026-11-26 |
| north-worker-sandbox (L18) | crates that depend on north-runtime to reach the sandbox | 0, and no cycle through a dev dependency | 2026-11-26 |
| north-offload (L20), north-timers (L21) | product lines of north-runtime | under 8,000 once L22 lands (17,584 on 2026-11-12) | 2026-11-26 |
| north-agent-policy (L24) | product lines of north-core | 0: the crate is gone | 2026-12-03 |
| north-grounding (L25) | crates that hold brief grounding | 1 (2 on 2026-11-12) | 2026-12-03 |
| north-docs (L28) | top-level modules of north-ops | at most 30 (38 on 2026-11-12) | 2026-11-26 |
| north-run-ledger (L29) | crates that depend on north-providers and name only its ledger | 0 | 2026-12-03 |
| north-dash (L45), north-evals (L46) | product lines of the root package | under 14,000 (20,504 on 2026-11-12) | 2026-12-03 |
| north-land-records (L48) | modules of north-base that hold a landing record | 0 (5 on 2026-11-12) | 2026-12-03 |
| Every split above | crates whose purpose needs an "and" | at most 6 (13 on 2026-11-12), with the product line count no more than 60 lines per new crate above its reading before the split | 2026-12-03 |

## What this plan leaves alone

Work in flight on 2026-11-12 is planned around and not repeated: the argument and error library across the command layer, the JSON encoder and escapers, Buck2 remote execution and the build interface in north-build-system (north#1115), the land scans for clean-up, the brief-grounding step, and the estimator switch in the learned-values store. Boxes that touch their files say so in After.

Open issues that already name a split keep it: north#1195 (the verdict path out of the lander, the enqueue function), north#1238 (the window mover), north#1246 (dependencies by what each brings). No lane is refused for size (north#729), tests and docs do not count against the line budget, and Codex is not removed.

## How the figures were taken

Read-only, with `cargo metadata`, `rg`, `git log` and `north code-health report --json`; nothing was built.

- Dependencies: `cargo metadata --format-version 1 --offline`, workspace members only, normal, dev and build edges kept apart. A crate's depth is its longest path down.
- Size: lines of `.rs` files per package; a file counts as a test when its path matches `tests.rs`, `*_test.rs`, `*_tests.rs` or a `tests/` directory, the path rule of north#1276.
- Churn: `git log --since='7 days ago' --numstat` over `.rs` files, added plus deleted, attributed to the package that owns the path today. The window held 2,737 commits and includes five crates since removed.
- Public surface: top-level `pub` functions, types, traits, constants and statics in non-test files, counted as used outside when another package names the item, by its bare name for a type declared `pub` in one crate only and by `module::name` otherwise. It is a text estimate without the compiler: 2,569 of 6,989 items are named outside their crate.
- Limits, cycles, hotspots and allowances: `north code-health report --json` and `structure.toml` at the same commit.
