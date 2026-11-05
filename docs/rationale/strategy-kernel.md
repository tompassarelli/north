# Strategy kernel: north's decisions as pure functions over recorded inputs, checked by replay

Design of 2026-11-06 for north#643 box 1: the decision record (case file) for placement, admission and
the lander, the decision log that writes it, the replay command, the line between kernel and strategy,
the `no_std` landing gate, and the migration order. Tom asked for the most ambitious, best-scaling way to keep north's fast-moving decision logic fast
and safe to change (2026-11-06). The design stands or falls on one property: replay tells the truth. A
decision function may read only its input record, so the record has to hold every input today's code
reads, and replay has to show when it does not.

## Why

North's decision logic changes constantly. Of the 973 commits on main from 10:00 PM on 2026-10-30 to
`89e7fc553` (5:21 AM on 2026-11-06, about 7 days; laptop time, UTC+7), 550 touched decision code. Sorted into areas by file
path (the script is in [Method](#method)):

- landing gate verdicts: 326 commits, 56,443 lines
- hook and guard verdicts: 125 commits, 36,926 lines
- pace: 70 commits, 14,959 lines
- dispatch ranking: 70 commits, 10,239 lines
- tier choice and tuning: 63 commits, 2,552 lines
- placement: 45 commits, 3,014 lines
- admission: 39 commits, 2,915 lines

Each change is checked today by hand-written tests over fixtures, and those fixtures reach the decision
through side doors in the decision code itself: `PLACEMENT_NOW`, `PLACEMENT_HOSTS`,
`SPAWN_CAPACITY_STATUS`, `AGENT_CAPACITY_SETTLE_SECONDS`, `AGENT_CAPACITY_DISK_FREE_GIB`,
`AGENT_CAPACITY_LOADAVG`. Nothing checks a change against what north actually decided last week, because
the decision functions read files, ssh, sysfs, environment variables and two clocks while they decide, so
no past call can be run again.

## The boundary

The kernel is Rust, as today: processes, git, the ledger, capacity leases, ssh, files, hooks, and every
other effect. Strategy is a function `decide(record) -> decision` with no IO, no clock, no environment
and no randomness. One call works like this:

1. The kernel gathers every input into one record and stamps one clock reading, `at`.
2. It calls the strategy.
3. It appends the record, the decision and the strategy's version (the runtime sha) to the decision log.
4. It carries out the effects the decision names.

Five rules keep the line clean:

- **Inputs are read before the call, never during it.** A strategy cannot ask for more mid-call. Where
  today's code reads only on some branches, the kernel either always reads the input when it is cheap,
  or makes a second call whose record names the first (`follows: <id>`). Placement's `has_directory`, an
  ssh `test -d` for the machine placement picked, joins the per-machine probe that already makes one ssh
  round trip to every machine, so it costs no extra trip. Admission's reclaim-then-decide-again becomes a
  second call.
- **Effects come out as data.** Admission's `note_reason` ticket write, the disk reclaim, the
  launch-refused message to the parent and the stderr verdict lines become fields of the decision, and
  the kernel acts on them.
- **Parameters belong to the strategy, configuration belongs to the record.** Values compiled in from
  `harness/defaults.toml` (`admit_settle_seconds`, `placement_drain_margin`, `bar1_*`,
  `build_disk_free_gib`, ...) and `harness/model-catalog.toml` plan multipliers ship with the strategy and
  are covered by its runtime sha. South's `agents/orchestration.toml` changes on its own schedule, so the parts of
  it a decision reads go into the record.
- **One clock per call.** Placement reads two clocks today (`storage::now` for usage and the credit,
  `capacity::mode::now` for the battery and playtest hold). Both become the record's `at`.
- **Fixture environment variables go.** The record replaces every test seam listed under [Why](#why). A
  test of a strategy is a record, so the decision code has no reason to read the environment.

The `World` traits that `guard::spawn_capacity` and `capacity::effects::Host` already use are this same
pattern, applied by hand to one function each. Here it becomes the one shape every decision has.

### Crates, calls and versions

- `north-strategy-types`: `#![no_std]` with `alloc` and serde. One `Case` and one `Verdict` struct per
  decision kind. The kernel fills the `Case` and the log writes it with serde. Replay reads it back into
  the same type, and the strategy reads the same type, so the kernel and the strategy share one definition.
- `north-strategy`: a leaf crate linked into north. `#![no_std]` with `alloc`, `#![forbid(unsafe_code)]`,
  and its only dependency is `north-strategy-types`. Its interface is `decide_placement(&placement::Case) -> placement::Verdict`
  and the same for each kind. Without `std` it has no file, environment, clock, process, thread or
  network API, so a strategy edit that tries to read one fails to compile. A landing gate refuses
  `extern crate std` in the crate and any dependency not on its allowlist (`north-strategy-types`,
  `serde`, `libm`).
- The call passes native structs: no virtual machine, no serialization on the decision path. Serde runs
  only when the kernel appends the log line.
- Versions: a recorded call names `live:<runtime sha>`. North keeps each runtime tree under
  `~/.local/state/north/runtimes/SHA`, and replay's truth check runs a record's own version through
  that tree's `north strategy decide --batch`. So runtime garbage collection keeps every tree that a record in
  the 14-day log names. Replay of old against new runs the old tree's binary for old and the lane's
  build for new.
- Faults: a panic in a strategy is caught at the call (`catch_unwind`), recorded, and fails closed:
  placement refuses with the strategy error, and admission defers with `INVALID`. A fault never admits
  work. A linked call cannot be preempted, so there is no fuel. Termination is checked before landing
  instead: replay times every call, and a strategy whose slowest call over the 7-day replay passes 10 ms
  is refused. North#643's purity-gate box needs this restated for linked code. Its "empty import section and fuel"
  becomes the replay time limit and the gate under [Loading](#loading).
- What the seal leaves open, and what catches each: `static` atomics or `core::cell` statics hold state
  between calls, caught by the forbid on `static mut` and atomics in the landing gate, and by replay
  running each case twice. `core::hash` with a fixed seed is deterministic. `alloc` has no hash maps,
  so iteration order is always a `BTreeMap`'s. Floating point is IEEE on one target; `libm`, never
  `std` float functions, so results do not change with the platform's math library.

## Placement record

Placement runs once per `north fleet launch` or `revive` of a Claude box (`src/cli/fleet/placement.rs`
`placed` → `crates/north-dispatch/src/pacing/placement/gate.rs` `live` → `decide`). Every input it reads
today, the record field that holds it, and where it comes from:

- `at`: one clock reading. Today `PLACEMENT_NOW` or `storage::now()`, and separately
  `capacity::mode::now()` for the battery and the playtest hold.
- `launch`: `name`; `account` (named, or empty for placement's pick); `fresh` (true when a `cwd` is given,
  false on revive); `cwd`; `named_machine` (`--cwd HOST:PATH` or the revived record); `overrides` (`local`,
  `pin`, `duplicate`, `tier`, including those recovered from a revived note); `provider`;
  `verification`. All from `LaunchRequest` and `Overrides`.
- `brief`: the text placement parses: the system-prompt file followed by the prompt, anchored with cwd
  and task (`brief_text`, `gate::anchored`). This is the largest field, 1.5 KB for this box's brief.
- `config`: the parts of `orchestration.toml` it reads: `[placement] order, public_repos, private_repos,
  cloud_credit`; for each `[hosts.NAME]`: `ssh`, `serves`, `pushes`, `reaches`, `resident_cap`; `[accounts.claude]
  plans`. Plus the sha256 of the whole file.
- `credit`: the `costs.toml` service that `cloud_credit` names, its expiry, and the spend readout the
  credit line prints (`hosts.rs` `credit`, `credit_service`, `credit_expiry`).
- `usage`: for each `claude:*` control in `usage-gate.json`: `windows[]` with `window`, `used_pct`,
  `resets_at`, `reset_h`, `burn_pct_h`. Plus the file's sha256 and its age at `at`, so a stale sample shows
  up as stale.
- `pace`: for each account, `accounts.NAME.target.by` from `pace-control.json`. Placement reads only that
  field (`room.rs` `Basis::spending`), and the file is 925 KB, so the record holds the field, not the file.
- `roster`: the standing of each account: listed, duplicate (and why), foreign (owners) or unknown
  (`Roster::standing`).
- `machines[]`: for each host in placement order: `live` resident CLIs, null when unreachable (ssh
  `resident-cap status`, or the local count); `accounts` signed in (ssh `north fleet accounts`);
  `saturation` (`memory_total_mib`, `memory_available_mib`, `disk_free_gib`, `backlog` load, queued and
  threads) and its age, since it can be up to 60 s old from the cache; `hosted`, the fleet sessions this
  machine runs there (`machine_load`); and `has_cwd`, whether the launch directory exists there.
- `here`: the host name; `serves`; `gaming` (`agent_load::gaming_declared`); `battery`, the settled
  percent and discharging-since from `/sys/class/power_supply`; `playtest_hold_until` from the capacity
  mode state; `saturation` (`here_saturation`); and `spawn_cpu`, the spawn gate's inputs (`provisioned`,
  `limit`, `protected` pressure, `local_workers`) that `here_cpu_refusal` → `guard::spawn_capacity::launch_refusal`
  reads through its probe.

The decision: `target` (cloud lane, account, held or unserved, with why), `machine`, `account`, the
override `note`, the refusal kind (saturated, exhausted, final), `need`, and the verdict lines `--report`
prints. The admission wait in `src/cli/fleet/admission.rs` stays in the kernel: it is a loop over
placement calls, each with its own record, and its 60 s poll and 2-minute limit are kernel parameters.

Size: the record for this box, built from today's real `usage-gate.json` and the saturation cache, is
3,816 bytes as compact JSON. Volume: a few hundred launches a day, so under 2 MB a day before
compression. Recording it costs nothing a launch would notice.

## Admission record

Admission already has the right shape. `crates/north-capacity/src/capacity/admission.rs` `decide(&Facts)`
is nearly pure, and `Facts` arrives as JSON. The inputs it does not see in `Facts` are the ones
`enrich.rs` `admit_enriched` and three leaf functions read during the call:

- `game_reserve_agent_cpus`: `mode::confined_agent_cpus` reads the agent slice.
- `gaming`: the gaming marker file under the capacity root.
- `run_queue_load`: `/proc/loadavg`, or `AGENT_CAPACITY_LOADAVG`.
- `disk_free_gib`: `statvfs("/")`, or `AGENT_CAPACITY_DISK_FREE_GIB`.
- `host_warcraft_clients`, `host_bar1_margin_mib`: `[hosts.HERE]` in `orchestration.toml`.
- `bar1_free_mib`, `vram_free_mib`, `bar1_timed_out`: `aperture::memory()` (nvidia-smi), read only when the
  request starts GPU work. It stays conditional: the kernel knows from the request whether it will start
  GPU work before it calls, so the read happens before the call either way.
- `pair_leases` beyond the request: the Wisp LAN pair queue files.
- `need`: `AGENT_CAPACITY_NEED`.
- `within_exclusive`: `AGENT_CAPACITY_LEASE` checked against the exclusive leases.
- the settle window: `AGENT_CAPACITY_SETTLE_SECONDS`, read in `pressure_target::starting`. It becomes a
  strategy parameter and the variable goes.
- `serving(needs)`: the offload host, a lookup in `[hosts]`. The `serves` table goes into the record, and
  the strategy picks the host itself.
- `reclaim_disk`: an effect followed by a second decision. It becomes a decision field
  (`reclaim: true`) and a second call that `follows` the first.

`Facts` itself is built in `crates/north-capacity-host/src/capacity/host/decision.rs` `facts()` from the
signals (`/proc/pressure/cpu`, meminfo, `cpu.max`), GPU busy and clients, the lease and queue directories,
the land-check and native-waiting markers, the Warcraft cgroups, the requester lease (environment or
cgroup), the profile and the mode. All of that is kernel work, and all of it is already in `Facts`.
`now` is in `Facts` in milliseconds, so admission already has its one clock.

Volume is what makes admission hard. A waiting ticket decides again every second (`QUEUE_POLL_MILLISECONDS`),
or every 10 s when it is held behind another ticket. gex45 logged 905,102 queued seconds on 2026-10-31 and
1.77 million over 2026-10-30 to 2026-11-05 (`machine-capacity-usage.jsonl`, `queuedSeconds` summed by
day), so a heavy day there makes up to about 900,000 admission calls. The kernel records them in two
tables:

- `snapshot`: the host part of `Facts` (signals, GPU, leases, queue, markers, enrichment), written once
  per distinct reading and keyed by its hash. Every ticket deciding in the same instant shares one
  snapshot. That is at most one a second per host, about 86,400 a day.
- `call`: `at`, the request part (class, CPUs, memory, owner, pair, clients, ticket), the snapshot hash,
  the decision, and the strategy version. About 100 bytes each.

Consecutive snapshots differ in a few numbers, so hourly zstd segments should shrink them about 10 to 30
times. That ratio is an estimate: box 3 measures it on real streams and stops if a host passes
100 MB a day.

Recording must not slow the decision. Admission decides under the queue lock, so the kernel writes
each call as one `write(2)` to an `O_APPEND` file, with no fsync and no compression on the path. A
background step compresses closed segments.

## Lander records

The lander's choices (`crates/north-landing/src/land/serve.rs` `batch`, `bisect.rs`, `culprit.rs`,
`sheriff.rs`) interleave git effects with decisions. A cherry-pick's outcome is an input to the next
choice. So each choice is logged as one step: the observations so far, and the step chosen.

- `land.pick`, one per ticket per candidate (`candidate::pick`). Case: `base`; the ticket's `branch`,
  `head`, `red_fix`; the branches of earlier queued tickets; the observations `readable`, `unlanded`
  (`git cherry` shows a `+`), `picked_clean`, `conflict_files`, `bare_main`, `with_main`, `queued_ahead`,
  `dropped` (commit counts differ, or the cherry-pick dropped a commit), `generator_moved`, `stale_reverts`
  and `lane_gate` (its refusal or none). Decision: the `Pick` (included, decided, refused, undecided or
  deferred) and its text.
- `land.batch`, one per candidate round. Case: `base`; tickets in queue order (`branch`, `head`,
  `red_fix`); the picks; `limit`; `red_only`; `pause` (the held reason or none); the strike streaks
  keyed as `Strikes::key`; `spares`; `rebuilds`; `bisect_depth`. Decision: the lanes included, and whether
  to halve (`bisect::half`) instead of checking.
- `land.judge`, one per checked candidate. Case: the verdict (green, refused, red-main, transient,
  broken-base or deferred, plus detail), `included`, `base_moved`, `streak`, and whether this base was
  already searched. Decision: the `Judgement` after `bisect::escalate`.
- `land.wave`, one per culprit-search wave (`culprit::wave_size`). Case: `wanted`, and the free and total
  offload slots. Decision: the wave size.
- `land.sheriff`, one per bisection step (`sheriff::bisect`). Case: the red request (`head`, `failing`), the
  lanes, and every probe result so far. Decision: the next lane to probe, or the culprit.

The rule for steps: a new strategy that would have probed a different lane, or included a different set,
needs an observation that never happened. From that step on, replay counts that run's later steps as
`unreplayable: unobserved probe` and does not guess. The first step of every run always replays.

## Decision log: the brief for the box that writes it now

North#643's decision-log box writes this schema before any strategy module exists, so that a week of
real history is waiting when the first module lands. Everything below is enough to staff it.

**Envelope**: one JSON object per line. Every decision kind writes the same envelope:

```
{"v":1,"kind":"placement","id":"whiterabbit-1793917506.374-41822-0","at":1793917506.374,
 "host":"whiterabbit","strategy":"live:89e7fc553","follows":null,
 "case":{...},"decision":{...},"decide_us":412}
```

- `kind`: `placement`, `admission`, `land.pick`, `land.batch`, `land.judge`, `land.wave`, or `land.sheriff`.
- `id`: host, `at`, pid and a per-process sequence number, so ids are unique without coordination.
- `strategy`: `live:<runtime sha>`, the runtime that decided. Before a strategy is migrated this is the live code. After, it is the linked `north-strategy` of that runtime.
- `follows`: the id of the call this one continues, as with admission's reclaim-then-decide-again or a
  later lander step in the same round.
- `case`: the record defined in this doc for that kind. For admission, `case` holds the request part and
  `snapshot`, the hash of a line in the snapshot file.
- `decide_us`: the time the decision took, so that recording's own cost stays visible.

**Types**: create `crates/north-strategy-types` (`no_std` plus `alloc` and serde) holding one `Case`
and one `Decision` struct per kind. The log writes serde output of these types, never hand-built JSON,
so the module later reads exactly the bytes the log holds.

**Where**: `~/.local/state/north/decisions/KIND/YYYY-MM-DD/HH.jsonl` in UTC, on the host that decided,
with admission snapshots in `admission/YYYY-MM-DD/HH.snapshots.jsonl`. Each write is one `write(2)` of
one line to a file opened `O_APPEND`, with no fsync and no lock beyond the one the decision already holds.
A line under `PIPE_BUF` is atomic. Longer lines (briefs) go through one `write_all` on a per-process
handle, which is safe because each process appends whole lines. A failed write prints one warning, the
decision still stands, and the failure is counted in the next line's envelope (`dropped`).

**Rotation**: the hour in the path is the rotation. A retention task under `crates/north-ops/src/ops/retention`
compresses each closed hour to `HH.jsonl.zst` and deletes days older than 14. A host's log stays local,
and replay reads it over ssh the way placement's probes already do. Box 3 measures size. If admission
passes 100 MB a day on a host, that box comes back to the record design before going on.

**Call sites** (three areas):

1. Placement. In `crates/north-dispatch/src/pacing/placement/gate.rs` `live`, move every read listed under
   [Placement record](#placement-record) into one `placement::Case` built before `decide`, with
   behavior unchanged. That includes `has_directory` (`src/cli/fleet/placement.rs`, folded into the
   per-machine probe in `hosts.rs` `script`), `here_saturation`, `here_cpu_refusal`, the battery and the
   playtest hold. The live code then decides from the `Case`. Log in `src/cli/fleet/placement.rs` `check`,
   after `settle`, with the `Placed` or `Refusal` as the decision. Each `admission.rs` wait loop retry is its
   own line.
2. Admission. In `crates/north-capacity/src/capacity/enrich.rs` `admit_enriched`, gather the enrichment
   inputs listed under [Admission record](#admission-record) into the `Facts` before `admission::admit`
   is called. Log the snapshot and the call there, and the reclaim second call with `follows`. Remove
   `AGENT_CAPACITY_SETTLE_SECONDS` from `pressure_target.rs`, since the settle window becomes a parameter.
3. Lander. In `crates/north-landing/src/land/candidate.rs` `pick`, collect the observations into a
   `land::PickCase` as they happen and log it with the `Pick`. In `serve.rs` `batch`, log `land.batch`
   after `candidate::build` and `land.judge` after `bisect::judge`. In `culprit.rs` `wave_size` and
   `sheriff.rs` `bisect`, log one line per step.

**Its own check**: a `north strategy log check --since 1h` that parses every line of every kind back
into its types and reports lines that do not parse. A gather that misses an input today shows up the
day the first module is replayed against `live`, in the shadow box. This box cannot see it yet, and its
done-when should not claim to.

## Replay

```
north strategy replay DECISION [--since 7d] [--host HOST] [--new REF] [--old REF] [--show N]
```

`DECISION` is `placement` or `admission`. `--new` defaults to the working tree's strategy and `--old` to
`origin/main`'s. Replay streams the log, runs the old and new strategy over each record, and prints:

```
north strategy replay placement --since 7d
records: 1,412 (2026-10-30 9:14 AM .. 2026-11-06 5:02 AM), hosts whiterabbit, gex45
old 3f2a1c0 (origin/main)   new 9b04e7d (working tree)
truth: old reproduces 1,412 of 1,412 recorded decisions
same: 1,377
changed: 35
   18  account claude:c -> account claude:a (machine unchanged)
   11  held: every Claude account exhausted -> account claude:d
    6  gex45 -> here
unreplayable: 0
first 3 changes:
  Nov 5 10:12 PM  n612-fix  account claude:c on gex45 -> account claude:a on gex45
    claude:a week room 26, expiring 0.31/h; claude:c week room 81, expiring 0.12/h
  ...
```

Three lines carry the truth:

- **truth**: before it compares anything, replay runs each record's own recorded version (the runtime tree
  its sha names) and checks that it reproduces the recorded decision. A mismatch means the
  record is missing an input the decision read, or the strategy is not deterministic. Replay prints the
  first such record and exits 2, and no diff prints. This check keeps the schema honest over time: the
  first day a change reads something the record lacks, replay says so. Each `north runtime update` also
  runs it over the last day, so the check runs even when no strategy change is under review.
- **unreplayable**: records that lack a field the new strategy reads. Adding a field is a kernel
  change. Records made before that change cannot answer for it, so replay counts them and names the field
  and the date it started being recorded. It never fills in a default.
- **changed**: a diff of each decision on the inputs that actually happened. Replay does not simulate:
  if the new admission strategy admits a build ten minutes earlier, the hosts' later readings would
  have been different too, and replay does not pretend to know them. The goal checks of north#641 are
  judged over these per-decision diffs, and replay reports nothing about the paths history would have taken.

The 20 s target in north#643 box 3 is easy for placement and hard for admission. Up to 1.8 million
admission calls from a week on gex45, each replayed twice (old and new), is about 3.6 million calls. That
fits in 20 s on 20 threads only if one call costs under 110 µs, decoding included. Calls with
the same request on the same snapshot repeat, so replay decides each distinct (snapshot, request) pair
once and counts the repeats. The linked call adds nothing to that budget beyond the strategy's own work.

## Loading

`north-strategy` is a pure `no_std` leaf crate linked into north, as laid out under
[The boundary](#the-boundary). It needs no virtual machine, no second language and no fallback loader.
The compiler seals purity, the kernel and the strategy share native structs with no serialization on the
call, and it adds no dependency outside the gate's allowlist. Running strategies as WebAssembly modules,
or writing them in Starlark, Rhai or TypeScript, was considered and rejected on 2026-11-06.

The landing gate for the crate refuses:

- `extern crate std`, or a `#![no_std]` attribute missing from `crates/north-strategy/src/lib.rs`;
- any dependency in `crates/north-strategy/Cargo.toml` other than `north-strategy-types`, `serde` and
  `libm`;
- `unsafe`, `static mut`, and `core::sync::atomic` or `core::cell` types in statics, all of which would
  let state survive between calls;
- a change to the crate whose 7-day replay has a call slower than 10 ms, the stand-in for fuel that a
  linked call cannot have.

## Migration order

Placement and admission go first, in that order, because north#643's later boxes name them and because
together they test both ends of the design. Placement has the widest input list and few calls, so it
proves the record captures every input. Admission has the narrowest inputs and close to a million calls on
a busy day, so it proves recording and replay keep up. After those two, migrate by last week's churn,
since churn is where replay saves the most hand-written tests:

1. placement (45 commits): one record per launch. Gathering moves out of `gate::live` and `placement::decide`.
2. admission (39 commits): `Facts` plus the enrichment inputs, recorded as snapshot and call.
3. landing gate verdicts (326 commits): safe-push's gates already keep their decisions in pure functions
   over typed inputs ([safe-push-composition.md](safe-push-composition.md)), so moving them means
   recording those inputs.
4. hook and guard verdicts (125 commits): `north hook` runs on every tool call, so this one most needs a
   cheap per-call path.
5. pace (70 commits) and dispatch ranking (70 commits): they share `room::Rank` with placement, so they
   reuse its record fields.
6. tier choice and tuning (63 commits): reads the ledger, so its record holds the ledger rows it summed,
   not a pointer to the ledger.

Churn counts commits that touched each area, so one commit can count in two areas.

## Method

- Churn: `git log -M --since=2026-10-30T15:00:00Z --numstat --format='@%h' 89e7fc553`, with each file sorted
  into the first area whose path pattern it matches: landing (`landing|safe_push|safe-push|lander`), placement
  (`placement`), admission (`capacity/(admission|run_queue|gpu_admission|queued_ahead|pressure_target|native_hold|client_reserve|land_claim|cpus|enrich)`
  and `capacity/host/(decision|policy|priority|request|shape|sizing)`), dispatch (`dispatch/|backlog|gate_order|grants`),
  tier (`tiers|tier_|routing|tune/`), hooks (`hook|guard/|language_guard|code_health`), pace (`pace/|pacing/|usage`).
  The line counts include files moved with edits during the crate split that week, so they rank the areas
  but overstate the edits.
- Admission volume: `queuedSeconds` from gex45's `~/.local/state/agents/machine-capacity-usage.jsonl`,
  summed per UTC day: 125,246 s (10-30), 905,102 (10-31), 368,612 (11-01), 11 (11-02), 914 (11-03),
  120,822 (11-04), 245,030 (11-05), with 239 to 3,620 runs a day.
- Placement record size: a record for this box, built from the laptop's real `usage-gate.json` (four
  accounts) and the gex45, nexus and ccx33 saturation cache, is 3,816 bytes as compact JSON.

