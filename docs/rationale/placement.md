# Placement: one function places every session and heavy run from each host's recorded reading

Plan of 2026-11-11 for north#1036 (the box "One scheduler places every session and heavy run across the laptop,
gex45, cpx62 and cpx62b"). It designs and edits no code. On 2026-11-11 Tom asked that uneven load be solved at
its root, so that nobody watches load or names a host by hand. Six places in north pick a host today, each from
a different signal, and none remembers what the others just placed. The design replaces them with one pure
function over one recorded board, makes each host the keeper of its own budget, and keeps per-host admission as
the enforcer. All times are laptop time.

## What was measured

- 2026-11-11 7:32 AM: gex45 load 24 on 20 threads, cpx62 7 on 16, cpx62b 9 on 16, while launches queued.
- 2026-11-11 7:45 AM: gex45 load 86 with 20 sessions, the laptop 75 on 24 threads with 12, cpx62 27 with 3,
  cpx62b 31 with 5. Load per thread: 4.3, 3.1, 1.7 and 1.9.
- cpx62's watchdog samples for those minutes: at 7:32:07 load 6.93, CPU pressure some avg10 0.5%, 4.6 busy
  cores, one lease; at 7:45:18 load 20.0,
  pressure 54%, 12.4 busy cores, four leases; at 7:45:58 load 26.7, pressure 65%. Three sessions lived there, so
  most of that load was heavy runs other hosts sent it. Sessions are not the unit of load; the runs are.
- The same rows give the line between open and full: pressure passes 40% as load reaches the thread count
  (0.5% at 0.43 load per thread, 54% at 1.25). `offload_job_cpu_psi` in `harness/defaults.toml` records the
  same on 2026-11-08 (68.7% at load 18.3 on 20 threads, 6.7% at 9.6 on 16).

## What exists

Host picks, each with its own signal:

1. Session launch: `machine_for` in `crates/north-pacing/src/pacing/placement/gate.rs` takes the first host in
   `[placement] order` of South's `agents/orchestration.toml` (gex45, cpx62, cpx62b, then the laptop) that
   serves the box and is not refused by `Saturation::refusal` in `crates/north-capacity/src/capacity/saturation.rs`
   (four tickets queued in admission, or load over the threads while unleased work stalls over 10%). The
   decision is recorded and replayed (`crates/north-strategy-types/src/placement.rs`, the `Case`).
   `src/cli/fleet/placement.rs` then keeps the launch here when the chosen host lacks the launch directory.
2. Dispatch: `candidates` in `crates/north-dispatch/src/dispatch/mod.rs` ranks hosts by the run queue past the
   cores (`crates/north-dispatch/src/dispatch/wait.rs`), then by the same order, with its own probe and its own
   count of builds a host's queue takes (`crates/north-dispatch/src/dispatch/slots.rs`). Not recorded.
3. A session's compile: `most_free` in `crates/north-capacity/src/capacity/build_offload.rs` takes the build
   host with the most CPUs its admission has not committed. Only a host that is not a build host offers first
   (`offers_first`); a build host runs its own sessions' compiles itself. Not recorded.
4. A queued run pulled off a full host: `pick` in `crates/north-runtime/src/build_offload/take.rs`, most free
   CPUs less four per run already sent (docs/rationale/pull-hand-over.md). Not recorded.
5. Land checks and pre-queue gate checks: `check_host` in `crates/north-runtime/src/land_offload/room.rs`,
   least busy by slot backlog or committed share, with three standings for the laptop. Not recorded.
6. Runtime builds and the lander's build-ahead: `freest` in `crates/north-runtime/src/runtime/build_host.rs`,
   most free CPUs. Until this morning build-ahead (`crates/north-lander/src/land/serve/ahead.rs`) built on the
   laptop, seven at once. Not recorded.

Beside them:

- Admission on each host (`crates/north-capacity/src/capacity/pressure_target.rs`, `job_admission.rs` beside
  it): a build host starts a heavy run while its whole-host CPU pressure is under `offload_job_cpu_psi` (40%).
  Recorded and replayed. It bounds each host and compares none.
- The launch queue (`crates/north-fleet-supervisor/src/fleet/launch_queue.rs`) holds a launch the spend rule
  refused, and a harness launch on a saturated host; the fleet tick drains it. A Claude launch refused for
  saturation is not queued: its caller gets an error and tries again.
- Capacity leases are a modelled state machine with a `reserved` phase
  (`crates/north-capacity/src/capacity/lease_lifecycle.rs`), and a session holds an agent-class lease.
- The watchdog sampler (`crates/north-capacity-host/src/capacity/watchdog/sampler.rs`) writes load, pressure,
  memory, busy cores and leases every 30 s on every host. Nothing that places reads it.
- The GPU lease, the spend rule (docs/rationale/spend-rule.md), the cloud lane, the disk
  lines and the pull hand-over's sandbox.
- The flow alarm (`crates/north-flow/src/flow/capacity.rs`) tells the proxy when the laptop carries fleet
  builds beside a free build host for five minutes.

Why this left gex45 at four times its threads: pick 1 fills in a fixed order and reads a session in a model
turn as free room; picks 2 to 6 each see another part of the same host and none counts what was placed in the
last minute; and a session's own compiles never leave a build host.

## Designs weighed

- **A. The same greedy rule at each site** (lowest load per thread, no laptop fallback; being built now as
  the first box). It lands today and fixes a 7:32 AM. It keeps six sites and six signals, has no memory of
  what was just placed, so ten launches read one load and go to one host (17 boxes on gex45 on 2026-11-06,
  docs/rationale/dispatch-spread.md), and it fills a host again whenever its sessions are quiet. It is the
  right first box and not the end state.
- **B. One pure function over a recorded board, each host keeping its own budget** (chosen, below).
- **C. A scheduler service with one global queue**, on nexus or the laptop. It is the strongest single
  authority. It adds an always-on service and a round trip on every `cargo` call, puts a single point of
  failure on the two weakest hosts (nexus read 70% pressure on 3 cores this morning; the laptop sleeps), and
  needs every host to call it, where today a host calls only the hosts its `[hosts.NAME] reaches` declares.
- **D. Pull only**: every host under its target takes work from shared queues. Runs cannot herd, and north
  already pulls queued runs. A session still needs a decision at launch (account, needs, directory), and an
  idle host cannot see what waits elsewhere without the same board. B uses pull as its way to move a queued
  run.

B is the one coherent with north. The README's first paragraph makes staffing and routing across machines
the product. docs/rationale/strategy-kernel.md asks that a decision be a pure function of a recorded case,
checked by replay; B extends the placement case already recorded rather than adding a scheduler beside it.
The README keeps each fact in one place: B has one function, one target number and one ledger per host.
The standing order to fix at the highest level that works puts this in the structure, and B deletes five
pickers and four numbers where C adds a service.

## The design

**Work** is a session (a resident agent that lives for one box and is busy in bursts) or a heavy run (a
leased build or test, whoever starts it: a session, the lander, a gate check, a runtime build, build-ahead,
the slot warmer).

**A reading** is what one host says about itself at one instant: its threads, one-minute load, whole-host CPU
pressure (some avg10), memory total and available, root disk free and size, pushed-token age, live sessions,
the cores of running leases, of tickets queued in its admission and of reservations not yet arrived, whether
a GPU-exclusive lease is held, and on the laptop the protected-slice pressure and whether Tom's seat is
active. It is the saturation probe of today with pressure, the three core sums and its own timestamp added.

**The board** is every host's newest reading plus the work waiting unplaced. A decider reads it with the one
ssh round trip per host it already makes, records it in the case, and decides from it alone.

**Demand per thread** of host h:

- for a heavy run: (load + queued cores + reserved cores) / threads, what is busy now;
- for a session: (the larger of load and live sessions x `placement_session_cores`, + queued + reserved) /
  threads. A resident session holds its expected share while it is quiet in a model turn, which is what the
  fixed order never counted. A run may borrow cores a quiet session is not using; a new session may not.

**Full** has one meaning everywhere: CPU pressure at or over `offload_job_cpu_psi`, or demand per thread at
or over 1. Pressure is the fast, measured side and is what admission already enforces; demand per thread is
the same line seen before the work presses, and unlike pressure it ranks idle hosts, which all read 0%.

**The rule.** `place(board, ask)`:

1. Drop hosts that cannot take the ask: needs, reach, push, sign-in, `launches`, memory room, disk line,
   token age, a held GPU-exclusive lease for a heavy run, and a reading older than `placement_reading_seconds`
   (30, one sampler period), which makes the host dark. These are today's filters unchanged.
2. Drop the laptop unless the ask carries a local reason from a closed list: a need only the laptop serves,
   `--local` or `--pin` with its text, a build whose output the laptop runs, or no build host answering.
3. Among the rest that are not full, take the one whose demand per thread is lowest once the ask's charge is
   added (a session's `placement_session_cores`, a run's sized cores), so a 20-thread host takes
   proportionally more than a 16-thread one. A tie goes to the host name that sorts first. Nothing in the
   function names a host or reads an order.
4. A heavy run a session starts prefers its own host while that host is not full, because its lane's target
   directory is warm there; otherwise step 3.
5. When every remaining host is full, the answer is to wait, with the reason: a session in the launch queue,
   a run in the admission queue of the host it started on.

**No herding.** The decision ends by reserving the charge on the chosen host: a lease in its `reserved` phase
with a deadline (`placement_reserve_seconds`, 120). The host counts it in every reading from then on, whoever
asks. It becomes the session's or the run's own lease when the work arrives and is reclaimed at its deadline
when it does not. A host that already took someone else's reservation and is now full refuses, and the
decider takes its next choice. Within one dispatch run or one drain of the launch queue the decider adds each
placement to its own copy of the board first, which needs no ssh. Staleness is therefore safe: a decider may
rank on a reading up to 30 s old, but the host it picks checks its live ledger.

**One budget per host.** The host's ledger holds every charge against it: live sessions, running leases,
queued tickets, reservations. Placement reads the ledger; admission enforces the same pressure target on it.
Each piece of work is on exactly one host's ledger from reservation to release.

**Deciders and reach.** The function runs wherever a decision is made: the laptop and nexus for launches and
dispatch, the lander for its checks and builds, each host for its own sessions' runs. A decider places only on
the hosts its `[hosts.NAME] reaches` declares. A host that declares none places its own runs at home or
leaves them queued, and the taker on a host that declares the others moves them
(docs/rationale/pull-hand-over.md). The design changes no `reaches` line. With
every reaching host asleep, queued runs stay where they are and admission still bounds each host.

## Invariants

1. **Spread.** No work is placed on a full host while a host that could take it is open, and the open host
   chosen has the lowest demand per thread. From level hosts the difference stays within one charge; from
   uneven hosts the fuller one receives nothing until the others reach it. A queued run on a full host is
   offered again within 30 s. Check: `no_work_goes_to_a_fuller_host_while_an_open_one_could_take_it` (a
   property test over arbitrary boards and arrivals) and `the_2026_11_11_morning_is_placed_within_the_spread_bound`.
2. **The laptop.** Every placement on the laptop records one of the four local reasons, and the laptop's own
   enforcer (attended band, batch freeze on protected-slice pressure, game reserve, playtest hold, battery)
   still yields to Tom's foreground. Check: `the_laptop_takes_fleet_work_only_with_a_recorded_local_reason`,
   and the replay invariant `laptop-reason` over recorded decisions.
3. **Pure.** The same board and ask decide the same way, twice, after a JSON round trip and with the hosts
   listed in any order. Check: `the_same_readings_and_queue_place_the_same_way`.
4. **Dark hosts.** A host whose newest reading is over 30 s old takes nothing; its reservations lapse at
   their deadline and its queued launches go back to the launch queue on the next tick. Check:
   `a_host_that_stops_answering_loses_its_queue_to_the_others`.
5. **One budget.** A session and the runs it starts at home are charged to that host's ledger, and no charge
   is on two ledgers or none. Check: the model check below, and
   `a_session_and_the_builds_it_starts_charge_one_host_budget`.
6. **No host is named** (the invariant that ends the flipping). Build placement moved four times in two
   weeks (gex45 first, laptop first on Oct 31, bounded laptop first on Nov 7, Hetzner first on Nov 10); each
   was a preference for a named host set by the last incident. Hosts now differ only by what `[hosts.NAME]`
   declares they serve and reach and by their readings. Check: `placement_is_blind_to_host_names_and_their_order`
   (swapping two hosts' names and declarations swaps their placements), and no host list is left in
   `[placement] order`.
7. **No herding.** Ten launches decided against one reading spread by threads. Check:
   `ten_launches_against_one_reading_spread_by_threads`, on the 2026-11-06 dispatch run's host facts.

## Proof without watching

- **Fixture.** The board at 7:32 and 7:45 AM on 2026-11-11 and the launches between them, taken from each
  host's watchdog rows and the launching host's placement log. The watchdog keeps two daily files, so the
  rows are gone after the rotation that follows the next one (about 4:35 AM on 2026-11-13): the capture box is
  first. Expected: at 7:32 the queued launches go to cpx62 and cpx62b in turn until their demand per thread
  reaches 1, gex45 (1.2) and the laptop take none, and the rest wait; at 7:45, with every host full, no
  launch is placed, none goes to the laptop, and each waits with its reason.
- **Per-decision replay.** `north ops strategy replay placement --invariants` already runs recorded cases;
  it gains the `spread` and `laptop-reason` lines. Run placements become recorded cases of the same kind, so
  the five unrecorded pickers become replayable as they move onto the function.
- **Closed loop.** Per-decision replay does not simulate what later readings would have been
  (docs/rationale/strategy-kernel.md). The property tests step a board forward with a stated model (a placed
  charge joins the host's demand, a finished one leaves), which proves the rule's arithmetic and not the
  hosts' physics. The live reading in the last box closes that gap once.
- **Model check (north#1042).** The reservation is the state machine worth checking: the `reserved` phase of
  `capacity-lease` together with `offload-offer`, which is registered in `src/cli/machines.rs` with no model.
  Scope: two deciders reserving the last room on one host, a reservation whose work never arrives, work that
  arrives after its reservation lapsed, and the host going dark while it holds reservations. Promises: the
  last room is given once; a lapsed reservation's charge leaves the ledger; late work is admitted through
  admission and counted once; every reservation ends as a lease or as reclaimed.

## Work already placed badly

- A live session does not move. Its conversation, lane and warm target directory are on its host, moving
  costs a cold re-read of its context and a cold build, and a revive already stays where its conversation is.
- A launch still waiting is bound to no host and is placed afresh on every tick.
- Runs move. A session on a full host sends its next heavy run to an open host (step 4 of the rule, through
  the taker where the host declares no `reaches`), and a ticket queued on a full host is pulled to an open one. A
  run that has started is left to finish, since giving it back runs it again from the start.
- The rest drains by attrition: a full host receives no session, and its sessions end with their boxes.
- The flow alarm becomes the live oracle of invariant 1 (a host full for five minutes beside an open one
  with work queued) rather than a laptop-only watch.

## Kept, changed, removed

Kept: the recorded case, decision log and replay; admission on each host and its classes; the laptop's
bands, freeze, game reserve, playtest hold and battery rule; the eligibility filters; account ranking, the
spend rule; the cloud lane; the GPU lease (its exclusive class stays its own box, and the
reading only carries whether one is held); disk lines and budgets; the pull hand-over and its sandbox; the
watchdog's overload caps.

Changed: picks 1 to 6 call the one function; the saturation probe becomes the reading; a Claude launch
held for room waits in the launch queue like any other; a build host offers its sessions' runs when it is
full; the flow alarm reads invariant 1.

Removed: the host names in `[placement] order`; `QUEUED_LIMIT` and `STALL_LIMIT` with the load rule in
`Saturation::refusal`; `resident_cap` (200 on every host since 2026-11-10) and the sizing rule for it in
docs/rationale/dispatch-spread.md; dispatch's own probe, wait estimate and build count; the laptop's three
standings for land checks; `most_free`, `freest` and the taker's `pick`.

Left alone on purpose: accounts and spend; nexus, which takes no session or build; every `reaches` line; and
placement by predicted finish time (docs/rationale/capacity-scheduler.md), which waits until replay shows
this rule leaves work waiting beside an open host.

New numbers in `harness/defaults.toml`, three: `placement_session_cores` (prior 2; at 7:45 AM the four hosts
carried load 219 with 40 sessions, a peak that included land checks; a box measures it),
`placement_reading_seconds` (30) and `placement_reserve_seconds` (120). The target stays `offload_job_cpu_psi`.

## Price

Inferred, not measured. Cost: fourteen boxes of about 30 agent-minutes, 420 agent-minutes, and no operator
time. Saving: heavy runs queued 140.8 h in a week against 273 h running (South's throughput strategy review
of 2026-11-10, section 7 item 5); a tenth of that is two agent-hours a day, and Tom stops watching load. That
pays back inside three days.

## Assumptions, each with what would break it

- A lease in the `reserved` phase can exist before its process does and is reclaimed at a deadline. The
  reservation box reads `lease_lifecycle.rs` first; if not, the reservation is a sibling record in the same
  store, and the model check covers it the same way.
- Load per thread near 1 is where pressure passes the target on all four hosts. Shown above for cpx62 and
  gex45; the reading box compares the two over a day on the laptop and cpx62b and reports any host where
  they part.
- A session's expected share is a stable number per host. If the measuring box finds it varies more than
  twofold by role or provider, the charge is keyed by provider, and nothing else changes.

## Boxes

Written onto north#1036 in this order; each lands usable alone. Box 0 is the greedy box already there
(lowest load per thread, no laptop fallback), built by another worker.

1. Capture the morning: the two boards and the launches between, as a fixture. Needs the laptop.
2. The function, the four named tests on it, and session launch deciding through it.
3. In-run reservations: one dispatch run or queue drain adds each placement to its board.
4. Dispatch decides through the function; its ranking, wait estimate and build count go.
5. The reading: pressure, the three core sums and a timestamp in the probe; the dark rule.
6. Reservations on the host: a reserved lease at placement, counted in the reading, refused when full.
7. A session's runs: build offload and the taker decide through the function, recorded; a full build host
   offers its sessions' runs.
8. The harness's own runs: land checks, gate checks, runtime builds and build-ahead decide through it.
9. The laptop rule: the closed list of local reasons, recorded; a launch held for room queues.
10. A dark host's queued launches return to the queue; its reservations lapse.
11. Replay invariants `spread` and `laptop-reason`.
12. Model the reservation under north#1042.
13. Measure `placement_session_cores` from a day of watchdog rows and set it with its source.
14. Remove what the list above names, re-point the flow alarm, and take one live reading.
