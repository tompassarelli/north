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
3. It appends the record, the decision and the strategy's version to the decision log.
4. It carries out the effects the decision names.

Seven rules keep the line clean:

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
  are covered by its crate hash. South's `agents/orchestration.toml` changes on its own schedule, so the parts of
  it a decision reads go into the record.
- **One clock per call.** Placement reads two clocks today (`storage::now` for usage and the credit,
  `capacity::mode::now` for the battery and playtest hold). Both become the record's `at`.
- **Fixture environment variables move to the kernel.** The test seams listed under [Why](#why) are read
  by the gather, never by the strategy. Once the strategy's own tests are records, they go. Production
  variables (`AGENT_CAPACITY_NEED`, `AGENT_CAPACITY_LEASE`) stay as inputs the gather reads and records.
- **The strategy formats no time.** Verdict text that names a clock time, such as the playtest hold's
  `local_twelve_hour()` at `gate.rs:557`, gets an epoch from the strategy and is formatted by the kernel.
  Otherwise the unrecorded timezone would be part of the decision.
- **The strategy reads typed config.** `gate::decide` takes `&DocumentMut` and `room.rs` reads
  `toml_edit` and `serde_json::Value` today. The kernel parses them into the typed `Case` fields, so the
  strategy needs no TOML or JSON parser.

The `World` traits that `guard::spawn_capacity` and `capacity::effects::Host` already use are this same
pattern, applied by hand to one function each. Here it becomes the one shape every decision has.

### Crates, calls and versions

- `north-strategy-types`: `#![no_std]` with `alloc` and serde. One `Case` and one `Verdict` struct per
  decision kind. The kernel fills the `Case` and the log writes it with serde. Replay reads it back into
  the same type, and the strategy reads the same type, so the kernel and the strategy share one definition.
- `north-strategy`: a leaf crate linked into north. `#![no_std]` with `alloc`, `#![forbid(unsafe_code)]`,
  and its only dependency is `north-strategy-types`. Its interface is `decide_placement(&placement::Case) -> placement::Verdict`
  and the same for each kind. Without `std` it has no file, environment, clock, process, thread or
  network API, so a strategy edit that tries to read one fails to compile. The parameters compiled in
  today from `harness/defaults.toml` and `harness/model-catalog.toml` (read through `admission.rs:15`,
  `room.rs:8` and `spawn_capacity.rs`) move into this crate, so its content hash covers them.
- The call passes native structs: no virtual machine, no serialization on the decision path. Serde runs
  only when the kernel appends the log line. The types crate turns on `serde_json`'s `float_roundtrip`,
  and replay checks that a native case and its JSON round trip decide alike. Without it the default float
  parser could move a `total_cmp` tie in `Rank::order` (`room.rs:107`). Infinity and NaN are encoded
  as strings, because JSON cannot carry them.
- Versions: a recorded call names `strategy:<content hash of crates/north-strategy and north-strategy-types>@<commit>`.
  Keeping one runtime tree per version would not work: trees are about 78 MB, about 20 appeared in three
  hours this morning, and 23 already take 1.8 GB. Instead, replay builds the leaf crate alone at the
  recorded commit into a small `strategy-replay` binary, cached by hash under
  `~/.cache/north/strategies/HASH`. That build is seconds, because the crate depends on nothing in north.
- Faults: a panic in a strategy is caught at the call (`catch_unwind`), recorded, and fails closed:
  placement refuses with the strategy error, and admission defers with `INVALID`. Stack overflow and
  allocation failure abort the process instead of unwinding, so they are not caught. The gate below
  bounds them before landing, and in production they take down only the one command that decided. A
  linked call cannot be preempted, so there is no fuel. Termination is checked before landing instead:
  replay times every call and records peak memory, and the gate refuses a strategy whose slowest call over
  the 7-day replay passes 10 ms or whose peak passes 64 MiB. North#643's purity-gate box needs this
  restated for linked code: its "empty import section and fuel" becomes this gate.
- What the seal leaves open, and what catches each:
  - State held between calls in statics (atomics or `core::cell`): caught by the gate, and by replay
    deciding each case twice.
  - Dropping `#![no_std]`, a build script, or `include_str!`: each lets IO or the host's files back in.
    The gate builds the crate for `x86_64-unknown-none`, a target with no `std`, so removing `no_std` fails
    to compile. It also refuses `build.rs` and `include_*!` in the crate.
  - Proc macros: only `serde_derive`, through the types crate, which the dependency allowlist pins.
  - Iteration order: `alloc` has no hash maps, so every map is a `BTreeMap` and its order is fixed.
  - Floats: use `libm`, never platform math, so results do not change with the host's math library.

## Placement record

Placement runs once per `north fleet launch` or `revive` of a Claude box (`src/cli/fleet/placement.rs`
`placed` → `crates/north-dispatch/src/pacing/placement/gate.rs` `live` → `decide`). Every input it reads
today, the record field that holds it, and where it comes from:

- `at`: one clock reading. Today the clock is read in four places: `PLACEMENT_NOW` or `storage::now()`
  in `live`; `storage::now()` again in `hosts.rs` `saturation` and `fetch` for the cache age; and
  `capacity::mode::now()`, which is whole seconds, for the battery and the playtest hold. All of these
  become `at`. Where today's code compares whole seconds (`hold.until > now`, `settled_battery`), the
  strategy truncates `at` the same way.
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
  credit line prints (`hosts.rs` `credit`, `credit_service`, `credit_expiry`), including the readout
  file's modification time, which `hosts.rs:61,72` falls back to.
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
  threads) and its age, since it can be up to 60 s old from the cache; `probed_at` for `live` and
  `accounts`, which the per-pass memo (`hosts.rs` `remembered`) can serve stale to a later launch in the
  same pass; `hosted`, the fleet sessions this machine runs there (`machine_load`); and `has_cwd`, whether
  the launch directory exists there. For a remote machine that is an ssh `test -d` added to the probe
  script, with the cwd added to the memo key. For this host it is a local check.
- `here`: the host name; `serves`; `gaming` (`agent_load::gaming_declared`); `battery`, the settled
  percent and discharging-since from `/sys/class/power_supply`; `playtest_hold_until` from the capacity
  mode state; `saturation` (`here_saturation`); and `spawn_cpu`, the spawn gate's inputs (`provisioned`,
  `limit`, `protected` pressure, `local_workers`) that `here_cpu_refusal` → `guard::spawn_capacity::launch_refusal`
  reads through its probe.

The decision: `target` (cloud lane, account, held or unserved, with why), `machine`, `account`, the
override `note`, the refusal kind (saturated, exhausted, final), `need`, and the verdict lines `--report`
prints. Despite its name, `src/cli/fleet/admission.rs` is placement's wait loop, not capacity admission.
It stays in the kernel: each retry is a placement call with its own record, `follows` set to the
previous attempt's id, and its 60 s poll and 2-minute limit stay kernel parameters. `gate::live` has a
second caller, `north dispatch` placement at `crates/north-dispatch/src/pacing/placement.rs:312`, which
records the same case kind.

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
- `pair_leases` beyond the request: the Wisp LAN pair queue files, found through `HOME`/`XDG_STATE_HOME`
  (`enrich.rs:10-13`) and `host_name()` (`enrich.rs:41`). The `/proc/<pid>` liveness check that
  `native_hold.rs:157` makes on each queued pair is recorded as a per-pair `alive` flag.
- `disk_free_gib`'s root: `AGENT_CAPACITY_DISK_ROOT`.
- `need`: `AGENT_CAPACITY_NEED`.
- `within_exclusive`: `AGENT_CAPACITY_LEASE` checked against the exclusive leases.
- the settle window: `AGENT_CAPACITY_SETTLE_SECONDS`, read in `pressure_target::starting`. The gather
  reads it into the case, so behavior and the existing tests stay as they are while the log is built. It
  becomes a strategy parameter when admission migrates.
- `serving(needs)`: the offload host, a lookup in `[hosts]`. The `serves` table goes into the record, and
  the strategy picks the host itself.
- `reclaim_disk`: an effect followed by a second decision. It becomes a decision field
  (`reclaim: true`) and a second call that `follows` the first.

`Facts` itself is built in `crates/north-capacity-host/src/capacity/host/decision.rs` `facts()` from the
signals (`/proc/pressure/cpu`, meminfo, `cpu.max`), GPU busy and clients, the lease and queue directories,
the land-check and native-waiting markers, the Warcraft cgroups, the requester lease (environment or
cgroup), the profile and the mode. All of that is kernel work and already in `Facts`. The gather is
not free of effects: building `Facts` writes `clientsSeen` and `startedOwners` to lease files
(`decision.rs` `with_started_clients`, `record_started_owner`), `record_gpu_busy` writes a sample, and
`read_leases` reclaims dead leases. Those writes stay in the kernel and happen before the case is sealed.
The case records what they left. `now` is in `Facts` in milliseconds, so admission already has its one
clock.

Volume is what makes admission hard. A waiting ticket decides again every second (`QUEUE_POLL_MILLISECONDS`,
with up to 1 s of jitter, `run.rs:92`), or every 10 s when it is held behind another ticket. gex45 logged
905,102 queued seconds on 2026-10-31 and 1.77 million over 2026-10-30 to 2026-11-05
(`machine-capacity-usage.jsonl`, `queuedSeconds` summed by day), so a heavy day there makes up to about
900,000 admission calls. Every call reads its own signals and its own `now` (`decision.rs` `read_signals`,
`now_ms`, inside `with_lock`), so two calls almost never share a host reading. A shared snapshot table
would not deduplicate anything. Each call's `Facts`, leases and queue included, is its own case.

So the size is measured, not estimated. The decision-log box logs every admission call in full for
its first day and reports bytes and calls per host. If gex45's day stays under 100 MB compressed, every
call stays logged. Otherwise the log keeps every call whose decision differs from that ticket's previous
call, plus one call per ticket per 10 s. Replay then reports admission diffs at 10-second resolution and
prints that resolution on its first line, so the record never claims calls it did not keep.

Recording must not slow the decision. The gather already makes slow reads under the queue lock
(`nvidia-smi` through `aperture::memory`, `statvfs`, `/proc`), and those stay as they are. Logging adds
nothing under the lock: the kernel moves the owned `Facts` and the decision out of `with_lock` and
serializes and appends them after the lock is released.

## Lander records

The lander's choices (`crates/north-landing/src/land/serve.rs` `batch`, `bisect.rs`, `culprit.rs`,
`sheriff.rs`) interleave git effects with decisions. A cherry-pick's outcome is an input to the next
choice. So each choice is logged as one step: the observations so far, and the step chosen.

Only some of these are judgements a strategy can own: `reservation::judge` inside `candidate::pick`,
`bisect::judge`, `half` and `escalate`, `culprit::wave_size` given the slot count, and `sheriff::find`.
`candidate::pick` and the batch loop are git procedures. `readable` fetches, `queued_ahead` runs only when
`with_main && !red_fix`, `refuse` writes logs, and `Strikes::load` and `pause::held` read state. So
`land.pick` and `land.batch` are observation logs that the strategy's judgements are checked against.
They do not become strategy themselves.

- `land.pick`, one per ticket per candidate (`candidate::pick`). Case: `base`; the ticket's `branch`,
  `head`, `red_fix`; the branches of earlier queued tickets; the observations `readable`, `unlanded`
  (`git cherry` shows a `+`), `picked_clean`, `conflict_files`, `bare_main`, `with_main`, `queued_ahead`,
  `dropped` (commit counts differ, or the cherry-pick dropped a commit), `generator_moved`, `stale_reverts`
  and `lane_gate` (its refusal or none). Decision: the `Pick` (included, decided, refused, undecided or
  deferred) and its text.
- `land.batch`, one per candidate round. Case: `base`; tickets in queue order (`branch`, `head`,
  `red_fix`); the picks; `limit`; `red_only`; `pause` (the held reason or none); the strike streaks
  keyed as `Strikes::key`; `spares`; `rebuilds`; `bisect_depth`. Decision: the lanes included, and whether
  to halve (`bisect::half`) instead of checking. Halving happens at two sites, the exhausted-strikes path
  (`serve.rs:230`) and the rebuild path (`serve.rs:336`), and both log.
- `land.judge`, one per checked candidate. Case: the verdict (green, refused, red-main, transient,
  broken-base or deferred, plus detail), `included`, `base_moved`, `streak`, and whether this base was
  already searched. Decision: the `Judgement` after `bisect::escalate`.
- `land.wave`, one per culprit-search wave (`culprit::wave_size`). Case: `wanted`, and the free and total
  offload slots, or none when `NORTH_OFFLOAD_ROOT` has no slots. The gather reads the flock slots, which
  `free_slots` reads today. Decision: the wave size.
- `land.sheriff`, one per step of `sheriff::find`. Case: the red request (`head`, `parent`, `run`,
  `failing`), the tip, the lanes, the reverted set (read from files at `sheriff.rs:110`), and every bisect
  and probe result so far. Decision: the next lane to bisect or probe, or the `Finding` (stale, unnamed,
  revert or environment).

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
- `strategy`: `live:<runtime sha>` while the live code decides, and `strategy:<crate hash>@<commit>`
  once `north-strategy` does.
- `follows`: the id of the call this one continues, as with admission's reclaim-then-decide-again or a
  later lander step in the same round.
- `case`: the record defined in this doc for that kind. A placement brief is stored once under
  `decisions/briefs/HASH` and the case holds its hash, which keeps lines short.
- `decide_us`: the time the decision took, so that recording's own cost stays visible.

**Types**: create `crates/north-strategy-types` (`no_std` plus `alloc` and serde) holding one `Case`
and one `Decision` struct per kind. The log writes serde output of these types, never hand-built JSON,
so the module later reads exactly the bytes the log holds.

**Where**: `~/.local/state/north/decisions/KIND/YYYY-MM-DD/HH.jsonl` in UTC, on the host that decided.
Each line is written with a single `write(2)` call to a file opened `O_APPEND`, outside any decision
lock, with no fsync. The writer checks the returned length. A short write counts as a failure, never as
a second write, so lines from different processes cannot interleave. A failed write prints one warning,
the decision still stands, and the failure is counted in the next line's envelope (`dropped`).

**Rotation**: the hour in the path is the rotation. A retention task under `crates/north-ops/src/ops/retention`
compresses each closed hour to `HH.jsonl.zst` and deletes days older than 14. A host's log stays local,
and replay runs on the host that holds the log: `north strategy replay --host gex45` runs there over ssh
and prints back only the summary. The first day's measured admission size decides the sampling rule under
[Admission record](#admission-record).

**Call sites** (three areas):

1. Placement. In `crates/north-dispatch/src/pacing/placement/gate.rs` `live`, move every read listed under
   [Placement record](#placement-record) into one `placement::Case` built before `decide`, with
   behavior unchanged. That includes `has_directory` (`src/cli/fleet/placement.rs`, folded into the
   per-machine probe in `hosts.rs` `script`), `here_saturation`, `here_cpu_refusal`, the battery and the
   playtest hold. The live code then decides from the `Case`. Log in `src/cli/fleet/placement.rs` `check`,
   after `settle`, with the `Placed` or `Refusal` as the decision. Each retry of the placement wait loop
   in `src/cli/fleet/admission.rs` is its own line, with `follows` naming the previous attempt. Log
   `north dispatch` placement's call of `gate::live` (`crates/north-dispatch/src/pacing/placement.rs:312`)
   too.
2. Admission. In `crates/north-capacity/src/capacity/enrich.rs` `admit_enriched`, gather the enrichment
   inputs listed under [Admission record](#admission-record) into the `Facts` before `admission::admit`
   is called, with `AGENT_CAPACITY_SETTLE_SECONDS` read into the case and its tests unchanged. Return the
   owned case and decision out of `with_lock` in `crates/north-capacity-host/src/capacity/host/decision.rs`
   `decision`, and append after the lock is released. Log the reclaim second call with `follows`. On its
   first day the box reports admission bytes and calls per host, which picks the sampling rule.
3. Lander. In `crates/north-landing/src/land/candidate.rs` `pick`, collect the observations into a
   `land::PickCase` as they happen and log it with the `Pick`. In `serve.rs` `batch`, log `land.batch`
   after `candidate::build` and at both halving sites, and log `land.judge` after `bisect::judge`. Log one
   line per step in `culprit.rs` `wave_size` and `sheriff.rs` `find`.

**Its own check**: a `north strategy log check --since 1h` that parses every line of every kind back
into its types and reports lines that do not parse. A gather that misses an input today shows up the
day the first module is replayed against `live`, in the shadow box. This box cannot see it yet, and its
done-when should not claim to.

**As built** (2026-11-06): `crates/north-strategy-types` holds the records; `north_base::platform::decision_log`
writes them; placement gathers in `crates/north-dispatch/src/pacing/placement/case.rs` (`gather`, then
`decide` from the `Case` alone, with `gate::live` the two in a row); admission splits its facts in
`crates/north-capacity/src/capacity/admission_record.rs`; the lander's lines come from
`crates/north-landing/src/land/steps.rs` and `candidate.rs`. `NORTH_DECISION_LOG` moves the log (empty
turns it off), and `.cargo/config.toml` points it into `target/` so tests never write the real log. Two
parts differ from the brief above. `AGENT_CAPACITY_SETTLE_SECONDS` stays until the admission strategy
module exists to own the settle window as a parameter, because two capacity-host journeys shorten it to
run in seconds; each admission line records the window it used (`settle_seconds`). Retention is
`north strategy log retain` (zstd on closed hours, days past 14 deleted) rather than a step of
`north retain`, which archives to one host, while this log stays on the host that decided.

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
determinism: old reproduces 1,412 of 1,412 recorded decisions, each decided twice
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

Truth rests on two checks that test different things.

- **Against live** proves the case is complete. This is the check that matters. A pure strategy can
  only read its case, so a strategy rerun on its own records always agrees with itself and cannot reveal
  a missing input. What can be wrong is the gather: an input the live code read that the case left out.
  Only the live code's decisions can expose that. `north strategy replay DECISION --against live --since 48h`
  runs the module on cases the live code decided and counts every disagreement (north#643's
  shadow-agreement box). One disagreement blocks the switch to the module and names the record. After the
  switch nothing live is left to compare with. So any later change to the gather, or any new input the
  kernel adds, runs in the same shadow mode for 48 h again before the module reads it.
- **determinism** proves version integrity and determinism. Replay rebuilds each record's recorded
  version from its crate hash and commit, decides every case twice, and checks both answers against the
  recorded decision. A mismatch means the strategy is not deterministic, or the version store is wrong.
  Replay prints the first such record and exits 2, and no diff prints.
- **unreplayable**: records that lack a field the new strategy reads. Adding a field is a kernel
  change. Records made before that change cannot answer for it, so replay counts them and names the field
  and the date it started being recorded. It never fills in a default.
- **changed**: a diff of each decision on the inputs that actually happened. Replay does not simulate:
  if the new admission strategy admits a build ten minutes earlier, the hosts' later readings would
  have been different too, and replay does not pretend to know them. The goal checks of north#641 are
  judged over these per-decision diffs, and replay reports nothing about the paths history would have taken.

The 20 s target in north#643 box 3 is easy for placement and hard for admission. Up to 1.8 million
admission calls from a week on gex45, each replayed twice (old and new), is about 3.6 million calls. That
fits in 20 s on 20 threads only if one call costs under 110 µs, decoding included. Calls almost never
repeat (see [Admission record](#admission-record)), so nothing can be deduplicated, and replay runs
where the log lives. A replay is a batch job, so it asks capacity admission for its threads like any
build does, and a heavy lease gives it only 6 CPUs. On 6 CPUs the full-volume week fits in 20 s only
under about 33 µs a call. If the first week's measured replay misses that, the 10-second sampling rule
applies, and replay prints the resolution it ran at.

## Loading

`north-strategy` is a pure `no_std` leaf crate linked into north, as laid out under
[The boundary](#the-boundary). It needs no virtual machine, no second language and no fallback loader.
The compiler seals purity, the kernel and the strategy share native structs with no serialization on the
call, and it adds no dependency outside the gate's allowlist. Running strategies as WebAssembly modules,
or writing them in Starlark, Rhai or TypeScript, was considered and rejected on 2026-11-06.

The landing gate for the crate refuses:

- a crate that does not build with `cargo check -p north-strategy --target x86_64-unknown-none`, a target
  with no `std`, so dropping `#![no_std]` or writing `extern crate std` fails;
- any dependency in `crates/north-strategy/Cargo.toml` other than `north-strategy-types`, `serde` and
  `libm`, and any `build.rs` or `include_*!` in the crate;
- `unsafe`, `static mut`, and `core::sync::atomic` or `core::cell` types in statics, all of which would
  let state survive between calls;
- a change to the crate whose 7-day replay has a call slower than 10 ms or a peak over 64 MiB. That
  stands in for the fuel a linked call cannot have, and bounds the stack overflow and allocation aborts
  that `catch_unwind` cannot catch.

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


## Reviewer findings

A reviewer on a different model tier (Sonnet, 2026-11-06) read the first landed version (`e96d6a3bb`)
against the code and tried to break it. Its findings, and the one revision that answers each:

1. The truth check could not find what it claimed to. A pure strategy rerun on its own records always
   agrees with itself, so it cannot expose a missing input. The revision splits truth into two checks.
   Against-live replay (the shadow box) proves the case is complete, and the gather runs in shadow mode
   again whenever it changes. Determinism replay proves version integrity.
2. Shared admission snapshots do not exist. Each call reads its own signals and `now` under jitter, so
   the snapshot table and its deduplication are gone. The first logged day measures size and sets the
   sampling rule: every call, or decision changes plus one per ticket per 10 s, with the resolution printed.
   Replay runs on the host that holds the log, and the per-call budget is stated for a 6-CPU lease.
3. Recording under the queue lock. Logging now happens after `with_lock` releases. The gather's own
   lease writes are named as kernel effects that happen before the case is sealed.
4. Runtime trees cannot be kept for 14 days (about 78 MB each, about 20 a morning). Versions are now the
   strategy crate's content hash and commit, rebuilt alone and cached.
5. Missing inputs: `AGENT_CAPACITY_DISK_ROOT`, the Wisp queue's `HOME`/`XDG_STATE_HOME` and host name,
   `/proc` pair liveness, the credit file's modification time, the probe memo's staleness (`probed_at`),
   and the four clock reads, one in whole seconds. All added. The production variables stay inputs.
6. The decision code was not `no_std` as written. Config is parsed into typed fields by the kernel, time
   formatting moves to the kernel, and compiled-in parameters move into the strategy crate.
7. Lander scope. Only `reservation::judge`, `bisect::judge`/`half`/`escalate`, `wave_size` and
   `sheriff::find` become strategy. Pick and batch are observation logs. Both halving sites log, and the
   sheriff case covers `find`.
8. Float round trip. `float_roundtrip` is on, infinity and NaN are encoded as strings, and replay
   checks native against round-tripped cases.
9. Seal escapes. The gate builds for `x86_64-unknown-none`, refuses `build.rs` and `include_*!`, and the
   replay limit also caps peak memory, since aborts are not caught.
10. Append atomicity. The writer makes one checked `write(2)` per line outside the lock, and briefs go by hash.
11. Brief errors: the placement wait loop's misleading file name, the second `gate::live` caller, the
    settle variable kept so the tests stay unchanged, the cwd in the probe memo key, and `follows` for
    retries. All fixed.

The reviewer found that `Facts.now` as admission's one clock, the reclaim as a second call, the
`has_directory` fold into the probe, the recorded saturation age, and the replay arithmetic held up.
