# Capacity scheduler: elastic, learned, fleet-wide placement by predicted finish time

Plan of 2026-11-04 (laptop time, +07). Tom asked for a capacity system that uses every core and GPU well, learns
from what actually ran, updates fast and rests on measured estimates, at maximum ambition and without ceremony
(2026-11-04). This document measures today's admission over the last 7 days, sets the design on top of the six
boxes already in flight (Wave 0), and splits the rest into boxes. It edits no code. A reviewer of a different tier
will try to break it, so every assumption is stated where it is used and collected in [Assumptions](#assumptions).

## Measured baseline, 7 days to 6:40 AM 2026-11-04

Window: released runs with `at` from 2026-10-28T00:00Z to 2026-11-03T23:40Z. Sources, all read-only:

- `~/.local/state/agents/machine-capacity-usage.jsonl` on the laptop (whiterabbit, 24 threads, 94 GiB: 3,928 runs),
  gex45 (20 threads, 62 GiB: 11,801 runs) and Nexus (3 threads, 5.9 GiB cap: 104 runs). One gex45 line was two
  records interleaved by concurrent appends and was dropped.
- Watchdog 30 s samples (`~/.local/state/agents/watchdog/samples*.jsonl`): the laptop from 2026-11-02 16:19Z and
  gex45 from 17:41Z to 2026-11-03 23:40Z, about 30 h each, with each live lease's `cpus` and `recentCores`.
- Live scope `memory.stat` and `memory.pressure` on both hosts at 6:4x AM.
- `gh run list` for north (Oct 31 to Nov 3, 1,000 runs, the API cap) and makgora.

Method: records were loaded into SQLite; a run's start is `at - wallSeconds` and its queue start is that less
`queuedSeconds`. Charged cores are `chargedCpus` (falling back to `reservedCpus`). The queries are reproduced in
[Reproducing the baseline](#reproducing-the-baseline).

### 1. Queue wait by class (seconds)

| Host | Class | Runs | p50 | p90 | Max | Total wait (h) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| laptop | moderate | 1,818 | 0.02 | 0.08 | 1,111 | 7.6 |
| laptop | heavy | 1,336 | 0.02 | 332 | 3,759 | 38.8 |
| laptop | native | 406 | 4.6 | 4.7 | 572 | 1.0 |
| laptop | runtime / gpu / tom / critical | 161 / 105 / 91 / 11 | 0.02 | 0.03 to 5.7 | 360 | 0.4 |
| gex45 | moderate | 2,934 | 0.02 | 699 | 5,734 | 190.9 |
| gex45 | heavy | 2,594 | 0.02 | 810 | 5,261 | 186.1 |
| gex45 | gpu | 1,222 | 0.07 | 577 | 8,475 | 56.5 |
| gex45 | native | 4,612 | 0.09 | 4.2 | 1,259 | 19.6 |
| gex45 | tom | 102 | 2.2 | 567 | 2,079 | 4.4 |
| gex45 | exclusive / runtime / critical | 11 / 255 / 71 | 0.02 | 0.03 to 129 | 996 | 0.9 |
| Nexus | moderate / heavy | 83 / 21 | 0.02 | 0.03 | 2,927 | 1.0 |

- Waiting is bursty, not chronic: gex45 waited 251 h of ticket time on Oct 31 and 102 h on Nov 1 (the farm and
  native-pressure episodes behind north#460 and north#473), then 0.0 h on Nov 2 and 0.3 h on Nov 3. The laptop's
  worst day was Oct 30 (23.6 h); Nov 3 was 3.3 h.
- By owner, the longest waits: gex45 `mutant:smashcraft` (452 runs, p90 1,228 s, 35.7 h), gex45
  `heavy-command-guard` (829, p90 523 s, 35.5 h), gex45 `smashcraft:inputs-derive` (331, p90 1,235 s, 33.2 h),
  laptop `heavy-command-guard` (814, p90 381 s, 25.9 h), gex45 `smashcraft:cpuField` (60, p90 3,876 s, 20.4 h),
  gex45 `gate:sc-ft-throne` (9, p50 2,515 s). Nexus never queued past 0.03 s at p90.
- GitHub Actions: north's `Land check` ran 291 times in 3.5 days at p50 290 s, p90 326 s; `Cargo` 108 at p50 371 s.
  `gh run list` reports `startedAt = createdAt`, so runner queue time is only visible per job (box W1-6).

### 2. Cores held by leases that were not using them

| Measure | Laptop | gex45 | Nexus |
| --- | ---: | ---: | ---: |
| Held core-minutes (charged cores x wall), 7 d | 26,375 | 127,343 | 668 |
| Used core-minutes (CPU-seconds / 60), 7 d | 14,029 (53%) | 45,650 (36%) | 367 (55%) |
| Held by runs averaging under 10% of their charge, 7 d | 3,008 (290 runs) | 28,915 (1,881 runs) | 120 |
| Held core-minutes in 30 s samples where a lease used under 10% of its cores, of all held, ~30 h | 1,313 of 4,415 | 1,650 of 10,099 | |

- Used share of held cores by class, 7 d: gex45 exclusive 1%, tom 5%, critical 27%, heavy 34%, native 34%, runtime
  43%, moderate 62%; laptop critical 19%, tom 38%, native 40%, heavy 52%, moderate 57%, runtime 68%.
- Largest idle holders: a gex45 private desktop lease held 2,415 core-min over 40 h; `wisp-lan-pair-*` native
  pairs 5,700 core-min in the six largest; `gex45-raid0-hold` (`sleep` under `exclusive`) 1,093; laptop `land-check`
  `bash` (the whole safe-push under one lease) 889 over 43 runs, the case a717bf640 fixed for safe-push only.
- When a gex45 ticket waited with its host over the committed-CPU limit, the leases there had committed 28.4 cores
  on average and were using 8.9 (3.2x). On the laptop the same was 22.9 committed, 11.2 used.

### 3. Utilization by host

| Host | Leased CPU, 7 d | Whole-host busy cores, ~30 h | Mean CPU some avg10 | GPU recorded |
| --- | ---: | ---: | ---: | --- |
| laptop (24) | 234 core-h = 5.8% | 3.92 = 16.3% | 3.2% | 1.6 GPU-h (gpu class: 0.26 GPU-h over 3.3 h wall, 8% busy) |
| gex45 (20) | 761 core-h = 22.6% | 2.74 = 13.7% | 0.5% | 0 for all 1,222 gpu and 4,612 native runs: not measured |
| Nexus (3) | 6 core-h = 1.2% | | | |

The fleet is mostly idle; queueing came from accounting (reservations, pressure reads of unleased load) in bursts,
not from a lack of cores. gex45's GPU use is invisible to the records, because the sampler reads DRM fdinfo engine
time, which the NVIDIA driver there does not report.

### 4. Tickets that waited while another host had room

Heavy and moderate tickets that waited over 60 s, tested at the moment they began waiting: the other host's
committed charge plus the ticket's cores fit within 20 cores and the other host had no ticket queued.

| Waiting host | Tickets | Wait (h) | Other host had room | Their wait (h) |
| --- | ---: | ---: | ---: | ---: |
| laptop | 337 | 44.8 | 86 (gex45 had room) | 14.2 |
| gex45 | 1,466 | 373.7 | 1,115 (laptop had room) | 266.6 |

Also: 283 of the laptop's 337 waits (36.3 h) and 301 of gex45's 1,466 (66.6 h) began while their own host's
committed charge plus the ticket fit the limit, so they waited on CPU pressure or queue order, not on lease
accounting. The gex45 column is an upper bound on what placement can move: many of those tickets came from lanes
that live on gex45 or needed its build inputs, and the laptop's attended reserve and Tom's play come first.

### 5. Error of today's per-command sizing

Runs of 10 s or more in heavy, moderate, runtime, gpu, tom and critical:

| Ratio | Host | Runs | p10 | p50 | p90 | Aggregate |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| used CPU-s / (charged cores x wall) | laptop | 2,198 | 0.23 | 0.73 | 0.93 | 0.55 |
| | gex45 | 5,078 | 0.19 | 0.59 | 0.91 | 0.39 |
| peak memory / charged memory | laptop | 2,198 | 0.17 | 0.83 | 1.00 | 0.57 |
| | gex45 | 5,078 | 0.21 | 0.83 | 0.93 | 0.61 |

- Peak memory is censored: 74% of moderate runs on the laptop (799 of 1,086) and 75% on gex45 (1,557 of 2,064)
  ended within 3% of their MemoryHigh, as did 28% and 13% of heavy runs. `measure.rs` records `memory.peak`, which
  counts page cache. Live laptop moderate scopes at 6:4x AM held 1,685 MiB anon under a 2,048 MiB MemoryHigh and
  2,149 MiB anon under 2,459 MiB, with 3.9 s and 2.1 s of total memory stall: these runs are being throttled, and
  their recorded peak is the cap, not their need. A gex45 heavy scope held 3,558 MiB of file cache beside 1,435 MiB
  anon.
- 249 laptop and 380 gex45 runs peaked above their charge.
- Duration and CPU-seconds, walk-forward over each run's previous 20 runs of the same host, owner, class and
  command (the geometric mean as the prediction; `wallSeconds >= 5`, native and agent excluded):

| Host | Runs | Fewer than 3 prior (cold) | Wall error factor p50 / p90 | Inside prior max | CPU-s error factor p50 / p90 | Peak memory within 1.2x prior max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| laptop | 2,406 | 1,078 (45%) | 1.52 / 4.52 | 91.5% | 1.65 / 9.34 | 99.5% |
| gex45 | 5,656 | 3,073 (54%) | 1.51 / 4.69 | 90.6% | 1.43 / 5.19 | 99.5% |

  Using only the previous run instead gives p50 1.34 to 1.35 but p90 6.05 to 8.8.
- Host speed: 26 commands that succeeded at least three times on both hosts ran at a gex45/laptop wall ratio of
  0.97 (geometric mean; 0.43 to 2.13) and a CPU-seconds ratio of 0.82. Placement gains come from waits, shipping
  and cache warmth, not from one host being faster.

### Other measured facts the design uses

- Failed work: non-native runs that exited nonzero used 67 of 210 core-h on the laptop (32%) and 111 of 340 on gex45
  (33%). `land-check` exited 1 in 370 of 693 laptop runs and 523 of 892 gex45 runs.
- Runtime builds: 161 on the laptop (median 862 CPU-s, 389 s wall) and 255 on gex45 in 7 d.
- The laptop's warm-cache `north` release build has p90 1,162 CPU-s; a cloud VM's cold build is more than that, so
  on its 4 cores it costs at least 5 min before any test.

## Wave 0, in flight, and what the measurements say about it

Six boxes are staffed in parallel with this plan: elastic-cpu, capacity-model, land-cache, offload-lane,
runtime-reuse and threads-to-actions. The design below assumes they land as briefed. Corrections the baseline
supports, to send to their workers now:

**elastic-cpu** (batch slice at CPUWeight=1 with cpu.idle, no per-scope CPUQuota, admission on memory, counts and
PSI, PSI-triggered freeze). The direction is right: leases use 36 to 55% of what they hold, and admission's own
accounting caused the bursts. Six things it must also do:

1. Keep the tom, critical and runtime classes out of the idle slice. They are Tom's play builds and the landing
   train, front of the queue by design; under cpu.idle they would lose to any desktop task.
2. Stop the `north capacity deadline` clock while a lease is frozen, as cd2686528 does for queue time, or a frozen
   land check dies at exit 124.
3. Keep charging frozen leases their memory: freezing frees CPU, not memory.
4. Move build offload's room test (`build_offload.rs`, committed CPUs against `aggregateCpuLimit`) and the spawn
   gate (`committedBatchCpus`) to memory headroom and PSI in the same change. Committed CPUs stop meaning anything
   once CPU is not admitted, and both would read hosts as full or empty at random.
5. Test priority inversion before landing: an idle-class build that holds Cargo's build-directory lock or a git
   index lock blocks a normal-weight waiter for as long as the desktop keeps the CPU busy. Done when a critical
   `cargo test` behind an idle-class `cargo build` in the same target directory, with all 24 threads busy at normal
   weight, finishes within 1.5x its unloaded wall.
6. Know where cpu.idle acts. systemd documents `CPUWeight=idle` as getting CPU only when no unmarked process runs
   "in this cgroup or its siblings", so it is relative to siblings. On the laptop at 6:5x AM the leased scopes sit
   in `agent.slice/agent-capacity.slice` (cpu.max 20 CPUs, the non-work-conserving part to remove), whose parent
   `agent.slice` (weight 20) competes with `app.slice` and `session.slice` (1000), `native.slice` (200),
   `game.slice` (100) and `background.slice` (30). Marking the batch slice idle makes batch yield to agent sessions
   and fleet services inside `agent.slice`, not to Tom's desktop; that protection stays agent.slice's weight 20
   against 1000 plus the game reserve's AllowedCPUs. `agent-capacity-farm.slice` already runs at weight 1 with
   cpu.idle 1.

**capacity-model** (per-lease wall, CPU-seconds, peak memory, cache state and wait; online p50/p90 per key x host x
cache; `north capacity model` and `north capacity calibration`; MemoryHigh from p90). Four corrections:

1. Do not fit MemoryHigh to p90 of `memory.peak`. Three quarters of moderate runs end at their MemoryHigh, so p90
   of peak learns the cap and never moves. Record the sampled maximum of `memory.stat` anon (plus shmem) and the
   scope's `memory.pressure` total, mark a run that stalled more than 1 s at its MemoryHigh as censored, and raise
   the next MemoryHigh for its key by 1.5x instead of fitting it.
2. Back off through coarser keys: 45% (laptop) and 54% (gex45) of runs had fewer than three prior runs of their
   exact command. Fall back from exact command to owner and shape, then shape and class, then class, and record
   which level answered.
3. Report coverage, not only error: the median prediction is within 1.5x, but the 90th percentile error is 4.5x.
   Backfill and placement must use the upper quantile and learn its coverage (target: 90% of runs end under their
   predicted p90).
4. Write the record to SQLite in the same transaction as the release, with the host in the row, or at least format
   the JSONL line into one buffer before a single write. `record_usage` in `host/database.rs` calls `writeln!` on an
   unbuffered `File`, which may issue several write calls per line, so concurrent releases interleave (one corrupt
   gex45 line in 21,018, which sizing then skips); and the fleet model needs the host.

**land-cache** (land-check results keyed to the tree ids of each check's filtered paths). Two corrections:

1. Put the check's tool identity in the key (the `flake.lock` blob and the runtime or runner image), or a toolchain
   bump reuses stale passes.
2. Reuse only verdicts this host produced. A laptop gate that skips a check because gex45 or a gex45 farm runner
   passed it hands gex45 authority over what the laptop lands. Actions verdicts already gate main on their own.

Also: most land-check runs fail (53% laptop, 59% gex45, exit 1). Reusing passes saves the passing third; the larger
saving is ordering checks to fail fast (W3-3).

**offload-lane**, **runtime-reuse**, **threads-to-actions**: nothing in the baseline contradicts them.

## Design

### What is kept, what is dropped

| Target from the brief | Verdict | Why (measured) |
| --- | --- | --- |
| Elastic CPU, memory the only hard-admitted resource, PSI freeze | keep (Wave 0) | leases use 36 to 55% of held cores; Nexus and gex45 sit at 1 to 23% |
| Learned per-command model updated after every run | keep (Wave 0), with the four corrections | p50 error 1.5x is usable; p90 4.5x and 45 to 54% cold demand backoff and quantiles |
| Placement by predicted finish time | keep | 86 laptop tickets (14.2 h) and up to 1,115 gex45 tickets (266.6 h) waited while the other host had room |
| Reservation backfill from predicted durations | keep, on memory and slots only | with CPU elastic, the 0.1-core head-of-line case disappears; memory, exclusive drains, GPU slots and native client caps still block |
| Result cache by tree hash, runtime reuse | keep (Wave 0) | 32 to 33% of core-hours went to failing runs; most land checks fail |
| Shared sccache on gex45 | drop as a new item | each host already shares one disk cache across leases (96854b985); sharing it with the laptop breaks D2/B24; build-cost.md measured `target-seed` ahead of sccache (31 Oct) |
| Calibration report with drift alarms | keep, plus bounded retuning | thresholds in defaults.toml are mostly priors waiting on counts |
| GPU on the same model, batch GPU on gex45 | keep, measurement first | gex45 records 0 GPU-seconds; the laptop's gpu class is 8% busy |

### Shape

One model, one placement function, one enforcer per host.

1. **Enforcer (per host, Wave 0 elastic-cpu).** CPU is shared by weight: Tom's sessions, game and native slices
   above batch, batch at cpu.idle, with the tom, critical and runtime classes at normal weight. Memory is the hard
   resource: a lease starts only if MemAvailable less the floor covers its predicted p90 anon plus 20%; MemoryHigh
   is set from that prediction and raised on a censored run. Counts stay hard: GPU slots, Warcraft clients, BAR1,
   exclusive. PSI protects Tom: when protected-slice CPU some avg10 passes the band's high mark, the newest batch
   leases freeze until it falls under the low mark; their deadlines pause and their memory stays charged. Game
   reserve keeps pinning agent work off Tom's game cores.
2. **Model (Wave 0 capacity-model, corrected).** For each run: key levels, host, cache state (lane `target/`
   seeded, warm or cold; sccache hits and misses), change size (files and lines against the lane's base), wall,
   CPU-seconds, anon peak, memory stall, GPU-seconds, queue wait, ship seconds, exit. Estimates are quantiles over
   the newest 20 runs at the finest key level with at least 3, each multiplied by a per-host speed factor learned
   from commands run on both hosts (today 0.97). The laptop holds the fleet copy: it pulls gex45's and Nexus's
   rows over ssh, the way `north worker-ledger --emit` already pulls worker rows, so each host stays the owner of
   its own records.
3. **Placement (new).** For a ticket j and each eligible target h, predicted finish
   `F(h, j) = now + wait_h(j) + ship_h(j) + cold_h(j) + run_h(j)`, where `run` is the model's p50 times h's speed
   factor, `cold` the measured extra for a cold cache on h, `ship` the measured rsync time for the lane (0 locally),
   and `wait` comes from simulating h's queue with the backfill rule below on predicted p90 ends. Choose the minimum
   F; break ties toward the host that keeps the laptop free. Two callers use it:
   - command placement: `machine-capacity run` and build offload choose among laptop, gex45 and Nexus;
   - session placement: `north fleet launch` and `agents plan --place` choose among laptop, gex45, Nexus and
     anthropic-cloud for a worker, using the worker ledger's predicted compute for the brief's facets.
4. **Backfill (new).** The head ticket that does not fit gets a reservation: the earliest time its memory and slots
   fit given running leases' predicted p90 ends. A later ticket starts now only if it fits now and either ends
   (predicted p90) before that reservation or still leaves the reservation's memory and slots free. A backfilled
   lease that overruns is not killed or frozen (freezing frees no memory); the miss is recorded and lowers that
   key's coverage, which widens its next estimate. `backfill_seconds` stays as the hard bound on how long any head
   can be passed. This is EASY backfill with system predictions instead of user estimates.
5. **Never twice (Wave 0 plus W3).** Land-check verdicts by tree and tool identity per host; runtime binary reuse;
   lane `target-seed` plus the per-host sccache disk cache; skip a queued build whose inputs a newer queued build of
   the same key supersedes; land checks ordered by predicted fail probability per CPU-second, and a landing's
   remaining checks cancelled at its first failure.
6. **Calibration (Wave 0 report, then W3).** `north capacity calibration` prints, per key level and host, predicted
   versus actual p50 error and p90 coverage over the last 7 days. A drift alarm fires into the watchdog incidents
   file when a key's p90 coverage over its last 20 runs drops under 80% or its median error doubles against the
   previous 7 days. A weekly retune replays the recorded week through the admission simulator
   (`pressure_target_tests.rs` grows into a replay) and writes tuned values for the pressure band, memory margin
   and backfill bound into a state file that admission reads, clamped to bounds stated in defaults.toml; the
   defaults keep their sourced values as the fallback.
7. **GPU.** gex45 gets per-process GPU time from NVML (`nvidia-smi --query-accounted-apps` or pmon) so its gpu and
   native runs carry GPU-seconds. GPU stays slot- and occupancy-admitted. While Tom is present, batch GPU work
   (renders, captures, headless Chrome) places on gex45 by predicted finish; the laptop's 890M goes to Tom's play
   and native checks first.

### Placement targets and their rules

| Target | Takes | Never takes | Cost model |
| --- | --- | --- | --- |
| laptop | anything it serves; required for whatever produces a binary the laptop runs, Tom's play builds and native checks | batch beyond the attended reserve and game reserve | measured; batch at cpu.idle |
| gex45 | code-only and its `serves` needs; batch GPU while Tom is present | any artifact the laptop will run or install (D2/B24); authority over laptop gates | measured, plus ship seconds |
| Nexus (3 cores, 5.9 GiB) | moderate and gpu-free tickets of at most 2 cores and 2 GiB predicted; never the runtime (`builds_runtime = false`) | heavy builds | measured, plus ship seconds |
| GitHub Actions | the workflows already there (public repositories: standard hosted runners are free; the gex45 farm is gex45's own capacity) | private repositories' secrets beyond what workflows already hold; paid larger runners | job duration and queue from the jobs API (W1-6) |
| anthropic-cloud | whole worker sessions for code-only boxes in north, makgora and wisp | south or any private repository; boxes needing gex45, Nexus or laptop state; native, GPU or sign-in needs | plan usage of the launching account, plus a cold clone and cold cargo cache each session |

**anthropic-cloud, modelled honestly.** A cloud session (4 cores, 15 GB) has no separate charge: it counts toward
the launching account's plan usage like a local session, so its builds are free compute and its tokens cost what a
local worker's cost. The one-time $250 credit on claude:a was spent by 2026-10-26 and expires 2026-11-05 07:59Z,
so today's rule (`place_anthropic_cloud` in `crates/north-core/src/ops/pacing/placement.rs`, which places the cloud
only while `credit()` says the credit is usable, and harness/skills/workers/references/cloud.md) stops using cloud
compute for the wrong reason. The corrected rule: a code-only box in a public repository may place on
anthropic-cloud under any Claude account whose headroom admits a local session by the same 5h and weekly rules, when
its predicted finish there (session start, cold clone, cold build, run) beats the best local host, or when every
local host is at its resident cap. Cloud sessions cannot reach gex45, Nexus or laptop state; they land by pushing
`claude/land-NAME` (Autoland in makgora and wisp, a courier for north), and their auto-mode permission prompts are
cleared by `north cloud sweep` on the host whose login launched them. Accounts a, b and c are at their weekly limit
today; d has headroom.

Launching under d: the laptop already holds claude:d's login (its profile has credentials), and Nexus holds only
claude:a's. `north cloud launch` and `north cloud sweep` run under one login on one host, so launching d's sessions
from the laptop needs no new sign-in; launching them from Nexus would need Tom to sign claude:d in there. Whether
account d's claude.ai GitHub connection can push `claude/land-*` is unknown; W1-5 probes it, and if it cannot, the
session hands back through `north cloud collect`, which needs no push access.

## Boxes

Each box is at most about 20 minutes of worker time, runs in its own lane and lands alone through safe-push. Within a
wave, boxes are independent unless a box says `After:`. "Done when" checks are commands with a recorded result.

### Wave 0 corrections (send now to the in-flight workers)

These are not new boxes; each is a Done-when line added to a staffed box.

- elastic-cpu: `systemctl --user show -p CPUWeight` of the tom, critical and runtime scopes is not `idle`; a frozen
  land check under `north capacity deadline 60` survives a 90 s freeze and exits 0; frozen leases still appear in
  `status` `leasedMemoryMiB`; `build_offload.rs` and the spawn gate read no `committedBatchCpus`; the priority
  inversion test above.
- capacity-model: a run record carries `anonPeakMiB`, `memoryStallUsec` and `censored`; `north capacity model`
  prints the key level used; `north capacity calibration` prints p90 coverage; the record is inserted with its host.
- land-cache: the cache key includes the `flake.lock` blob id; a verdict row records the host that produced it and
  reuse requires it to be this host.

Delivers alone: work-conserving CPU, a model that learns from every run, reused passing land checks.

### Wave 1: measurements the placement needs (all independent)

**W1-1. Fleet capacity rows on the laptop.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust.
Verify: local-test. After: capacity-model. Files: `crates/north-core/src/capacity/host/database.rs`, a `north
capacity pull` subcommand beside it. Spec: pull each `[hosts.NAME]` ssh host's `capacity_runs` rows newer than the
last pulled `at` into the laptop's capacity.db with a `host` column; the laptop only reads remote rows. Done when:
`sqlite3 capacity.db "select host, count(*) from capacity_runs where at >= '2026-11-04' group by host"` lists
whiterabbit, gex45 and nexus, and a second pull adds 0 rows.

**W1-2. GPU-seconds on gex45.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust. Verify:
native-check. Files: `crates/north-core/src/capacity/host/gpu.rs`, `measure.rs`. Spec: where DRM fdinfo gives no
engine time, read per-process GPU time from NVML through `nvidia-smi --query-accounted-apps=pid,gpu_util,time`
or `nvidia-smi pmon -c 1` sampled every 5 s (pmon needs no driver setting; accounting mode needs `nvidia-smi -am 1` on
gex45 first). Done when: one headless wisp render on gex45 under `--class gpu` records
`gpuSeconds > 0` in its usage row.

**W1-3. Ship and cold-cache cost in build offload.** Category: tooling. Spec: measured. Scope: single-module.
Surface: rust. Verify: local-test. After: offload-lane. Files: `crates/north-core/src/capacity/build_offload.rs`.
Spec: record `shipSeconds` (rsync of the lane) and the lane's cache state (`target/` seeded or not, sccache hits and
misses from the per-lease server) in the offloaded run's record. Done when: the next five offloaded builds' rows on
gex45 carry `shipSeconds` and `cacheState`, and the laptop prints their median.

**W1-4. Cloud placement by account headroom, not credit.** Category: bug-known-cause. Spec: measured. Scope:
single-module. Surface: rust. Verify: local-test. Files: `crates/north-core/src/ops/pacing/placement.rs`
(`place_anthropic_cloud`), its gate in `placement/gate.rs`, `harness/skills/workers/references/cloud.md` (Placement
and Cloud credit lane sections), `[placement] cloud_credit` and `cloud_host` in south `agents/orchestration.toml`
(read; the south edit is its own landing). Spec: anthropic-cloud is eligible for a code-only public box whenever
some Claude account's headroom admits a session by the 5h and weekly rules local sessions use; the verdict names
that account and its launch host (the host holding its login); the credit no longer gates it, and the
`cloud credit:` lines go. Done when: `agents plan --place BRIEF` on a code-only north brief prints `target
anthropic-cloud claude:d` today (a, b, c at weekly limit), and with every account over its limit prints a held or
local verdict.

**W1-5. Launch and sweep cloud sessions under the account that has headroom.** Category: tooling. Spec: measured.
Scope: cross-module. Surface: rust. Verify: local-test. After: W1-4. Files:
`crates/north-core/src/ops/dispatch/cloud.rs`, `harness/skills/workers/references/cloud.md`. Spec: `north cloud
launch` and `north cloud sweep` take the account (default: the placement verdict's) and run under that account's
profile on the host that holds its login (claude:d on the laptop); the trust-dialog write goes into that profile's
`.claude.json`; a lead's tick sweeps every launch host it used. Probe once whether account d's session can push
`claude/land-NAME`; if it gets 403, the brief's landing steps switch to the `north cloud collect` handback. Done
when: one docs-only north box launched under claude:d from the laptop reaches `Done: pushed claude/land-NAME` or a
collected mbox that `git am -3` applies, and `north cloud sweep` from the laptop lists that session.

**W1-6. Actions jobs as a capacity host.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust.
Verify: local-test. After: W1-1. Files: a `north capacity actions-pull` beside W1-1. Spec: read
`gh api repos/OWNER/REPO/actions/runs/ID/jobs` for north, makgora and wisp runs since the last pull; record per job
`createdAt` to `startedAt` (queue) and `startedAt` to `completedAt` (run), runner label and conclusion as host
`actions:LABEL` rows. Done when: `select host, count(*) from capacity_runs where host like 'actions:%'` is above 0
for each repository and the p50 queue per runner label prints.

**W1-7. Censored-memory replay.** Category: research. Spec: measured. Scope: single-module. Surface: rust. Verify:
none. After: capacity-model. Files: `docs/rationale/capacity-scheduler.md` (results). Spec: over 3 days of new rows,
compare anon peak with the old `memory.peak` per key and count censored runs. Done when: this document records the
censored share for moderate and heavy, laptop and gex45, and the median anon/peak ratio.

Delivers alone: the fleet's runs in one place, GPU on gex45 visible, ship and cold costs measured, Actions queue
measured, and cloud workers placed again under accounts with headroom.

### Wave 2: predicted finish and backfill (after Wave 1)

**W2-1. Admission replay simulator.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust.
Verify: local-test. Files: `crates/north-core/src/capacity/pressure_target_tests.rs` grows a
`capacity/replay.rs`. Spec: replay a recorded window's tickets (arrival, predicted and actual wall, anon peak,
class) through `decide` with a policy switch (today, elastic, elastic plus backfill, plus placement); print queue
wait p50/p90 per class and idle-held core-minutes. Done when: replaying Oct 28 to Nov 3 under today's policy
reproduces the measured laptop heavy p90 wait within 30% (measured 332 s).

**W2-2. `north capacity predict`.** Category: feature. Spec: measured. Scope: single-module. Surface: rust. Verify:
local-test. After: W1-1, W1-3. Files: `crates/north-core/src/capacity/host/estimate.rs`, a `predict` subcommand.
Spec: `north capacity predict -- CMD` prints per eligible host `wait`, `ship`, `cold`, `run` p50/p90 and `finish`,
with the key level used. Done when: for the next 20 heavy runs on the laptop, the logged prediction's p90 covers at
least 16 actual walls.

**W2-3. Command placement by predicted finish.** Category: feature. Spec: measured. Scope: cross-module. Surface:
rust. Verify: local-test. After: W2-1, W2-2. Files: `build_offload.rs`, `admission.rs` (offload line). Spec: replace
the room threshold with the minimum predicted finish across laptop, gex45 and Nexus (Nexus only for predicted at
most 2 cores and 2 GiB), keeping every rule in the targets table; record chosen and runner-up finish. Done when: the
W2-1 replay of Oct 28 to Nov 3 shows fewer than 20 laptop tickets waiting over 60 s while another host had room
(measured 86), and over the next day the chosen host's actual finish beats the runner-up's prediction in at least
70% of offload decisions.

**W2-4. Reservation backfill on memory and slots.** Category: feature. Spec: measured. Scope: single-module.
Surface: rust. Verify: local-test. After: W2-1, elastic-cpu. Files: `queued_ahead.rs`, `admission.rs`. Spec: the
backfill rule in the design; `backfill_seconds` stays the hard bound. Done when: the replay shows heavy p90 wait
below today's policy's on both hosts with no ticket passed for longer than `backfill_seconds`, and a unit test pins
that a backfilled ticket whose p90 end is after the head's reservation starts only if the reservation's memory stays
free.

**W2-5. Session placement by predicted finish.** Category: feature. Spec: measured. Scope: cross-module. Surface:
rust. Verify: local-test. After: W1-4, W2-2. Files: `crates/north-core/src/ops/pacing/placement.rs`,
`placement/launch.rs`. Spec: rank laptop, gex45, Nexus and anthropic-cloud for a box by the predicted finish of its
predicted compute (worker ledger facets), cloud including measured session start, clone and cold build per
repository; record the ranking in the verdict line. Done when: `agents plan --place` on a code-only north brief
prints all four targets' finish estimates, and for one week of verdicts the chosen target's measured box wall is
within the predicted p90 for at least 80%.

Delivers alone: work moves to the host that will finish it first, and the queue head no longer blocks smaller work
for memory it will not get soon.

### Wave 3: calibration, waste and GPU placement (after Wave 2, independent of each other)

**W3-1. Drift alarms.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust. Verify: local-test.
Files: the calibration command, `bin/capacity-watchdog` incidents writer. Spec: the alarm rule in the design. Done
when: a fixture where a key's walls double fires one alarm line in incidents.md, and the real 7 days fire fewer
than 5.

**W3-2. Bounded weekly retune.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust. Verify:
local-test. After: W2-1. Files: `capacity/replay.rs`, `harness/defaults.toml` (bounds on `admit_psi_*`,
`capacity_memory_margin`, `backfill_seconds`). Spec: grid-search those values over the last week's replay,
minimizing heavy p90 wait subject to protected-slice PSI under its high mark in replay; write the result to
`~/.local/state/agents/machine-capacity/tuned.json`; admission reads it clamped to the bounds. Done when: `north
capacity retune --dry-run` prints the chosen values and the replayed waits for them and for the defaults.

**W3-3. Fail-fast land-check order.** Category: performance. Spec: measured. Scope: cross-module. Surface: rust.
Verify: local-test. After: capacity-model, land-cache. Files: the land-check runner in `crates/north-landing/`.
Spec: order a landing's checks by predicted fail rate per CPU-second (by check and changed-path class, from
records), cancel the remaining checks at the first failure. Done when: over 20 failing landings the median
CPU-seconds spent before the failure verdict is under half of today's median for failing land checks (computed
from capacity_runs before the change).

**W3-4. Skip superseded builds.** Category: performance. Spec: measured. Scope: single-module. Surface: rust.
Verify: local-test. After: runtime-reuse. Files: `crates/north-core/src/ops/runtime/`. Spec: a queued runtime build
whose commit a newer queued one for the same host contains does not start. Done when: over one day, runtime builds
that exit 73 (killed by supersede) drop to 0 and every main commit still reaches the runtime within the current p90
latency.

**W3-5. Batch GPU on gex45.** Category: feature. Spec: measured. Scope: single-module. Surface: rust. Verify:
native-check. After: W1-2, W2-3. Files: `gpu_admission.rs`, `build_offload.rs`. Spec: while the laptop is attended,
gpu-class tickets whose needs gex45 serves place by predicted finish on gex45; the laptop keeps gpu tickets only
when gex45 is predicted later. Done when: one day attended shows no gpu-class run on the laptop while gex45 had a
free gpu slot, and DEFER_GPU_BUSY on the laptop counts 0 for agent work.

Delivers alone: estimates stay honest as code changes, failing landings stop burning a third of the core-hours, and
the laptop GPU is Tom's.

## Decisions for Tom

Only one, and only if W1-5's probe finds that account d's cloud sessions cannot push `claude/land-*`:

- Recommended: land d's cloud work through the `north cloud collect` handback (the session's commits come back as an
  mbox from its saved transcript and land through safe-push on the laptop). It needs no new sign-in and widens no
  access; it costs one collect step per box.
- Alternative: Tom connects GitHub for claude.ai account d (the Claude GitHub App or `/web-setup`), so d's sessions
  push like a's did. Pushes then land without the collect step, but a fourth account gets push access to Tom's
  repositories.
- Not recommended: signing claude:d in on Nexus so Nexus stays the only cloud launch host. It needs Tom at a login
  prompt and buys nothing, because the laptop already holds d's login and the launch uses no laptop CPU worth
  counting.

## ETA

Wave 0 is in flight. Wave 1 is seven boxes in parallel, about 20 minutes each plus landing; Wave 2 five, of which
W2-1 and W2-2 run first; Wave 3 five in parallel. Uncalibrated: with three to five workers, Wave 1 lands by about
8:30 AM, Wave 2 by about 11:00 AM and Wave 3 by about 1:00 PM on 2026-11-04, if Wave 0 lands by 7:30 AM.

## Assumptions

1. The 7-day window is representative except for Oct 31 and Nov 1 on gex45, which carry 354 of its 459 waiting
   hours (all classes); numbers are given per day where that matters.
2. "Another host had room" uses committed charge only, not PSI or memory, and ignores whether the ticket's needs
   were served there; it is an upper bound.
3. "Under 10% CPU" is a lease using under a tenth of its charged cores, over the whole run (records) or over a 30 s
   sample (watchdog).
4. Run keys use the recorded command, which has paths redacted, so two lanes' identical commands share a key.
5. Wave 0 lands as briefed, with the corrections above. If elastic-cpu does not land, W2-4's backfill must also
   cover CPU.
6. Host speed is the same for both build hosts within the measured spread; the per-host factor is learned anyway.
7. gex45 and Nexus rows inform the laptop's placement but never its gates or its binaries.
8. The cloud VM's 4 cores and 15 GB, and its cold cache each session, are per cloud.md; a setup script's cached
   result could warm dependencies (the cloud docs say a setup script is cached when it finishes in about five
   minutes); W2-5 measures cold start per repository rather than assuming either.

## Prior art

Each source was fetched on 2026-11-04 and the claim below is what its text says; entries are in
[docs/references.md](../references.md#scheduling-and-capacity).

- **Google Borg** (Verma et al., EuroSys 2015,
  https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/43438.pdf). Each task has a limit
  and a reservation: the reservation starts at the limit, decays toward measured usage plus a safety margin after
  300 s and rises fast when usage exceeds it. Prod work is scheduled against limits; non-prod work against other
  tasks' reservations, so it runs in reclaimed resources, "about 20% of the workload" in a median cell. Taken here:
  admit on measured use, never on the declared size, and keep a class (tom, critical, runtime) that never depends
  on reclaimed capacity.
- **Google Autopilot** (Rzadca et al., EuroSys 2020, "Autopilot: workload autoscaling at Google Scale",
  https://research.google/pubs/autopilot-workload-autoscaling-at-google-scale/). Limits set from usage history by
  moving-window recommenders (48 h half-life for memory) and a bandit-like ML recommender: ML-recommended jobs ran at
  23% slack against 46% for manually limited jobs with soft limits (31% for moving-window), and jobs severely hit by
  OOMs fell tenfold. Taken here: MemoryHigh from a learned quantile of anon use, raised on a censored run.
- **Slurm backfill** (https://slurm.schedmd.com/sched_config.html). Backfill starts lower-priority jobs only "if
  doing so does not delay the expected start time" of higher-priority ones, which needs "reasonably accurate time
  limits"; `bf_window` bounds how far ahead it plans and `bf_resolution` its time step. EASY backfill traces to
  Lifka, "The ANL/IBM SP scheduling system", JSSPP 1995 (https://link.springer.com/chapter/10.1007/3-540-60153-8_35;
  the open preview does not show the backfill text). Taken here: one reservation for the head, later work passes
  only if it cannot delay it.
- **Backfilling with system-generated predictions** (Tsafrir, Etsion and Feitelson, IEEE TPDS 2007; abstract read at
  https://manuscript.isc.ac/Inventory/49/827041.htm, the authors' copy was unreachable). Predicting a job's runtime
  as the mean of the same user's last two jobs, with the kill time kept apart from the prediction and predictions
  corrected when exceeded, beat EASY on user estimates. Taken here: predicted p90 ends drive the reservation, an
  overrun widens the estimate, and `backfill_seconds` stays the kill-free bound.
- **HEFT** (Topcuoglu, Hariri and Wu, IEEE TPDS 13(3), 2002, abstract at
  https://experts.arizona.edu/en/publications/performance-effective-and-low-complexity-task-scheduling-for-hete/).
  Picks the task with the highest upward rank and places it on the processor that minimizes its earliest finish
  time, with insertion into idle slots. Taken here: place each ticket where its predicted finish (wait, ship, cold
  cache, run) is earliest, not where room exists.
- **PSI** (https://docs.kernel.org/accounting/psi.html, already cited as [kernel-psi](../references.md#kernel-psi)).
  "some" is the share of time at least one task stalled, averaged over 10, 60 and 300 s, plus a microsecond total;
  triggers can be polled with windows of 500 ms to 10 s. Taken here: protection by pressure, and per-scope
  `memory.pressure` total as the censoring signal.
- **senpai** (https://github.com/facebookincubator/senpai) and **TMO** (Weiner et al., ASPLOS 2022,
  https://par.nsf.gov/servlets/purl/10332477). senpai uses PSI and cgroup2 memory limits to apply "just enough
  memory pressure" to find a container's working set; TMO saved 20 to 32% of memory across Meta's fleet. Taken here:
  learn memory need from pressure instead of from a peak the limit itself caps.
- **cgroup v2 CPU and memory controls** (https://docs.kernel.org/admin-guide/cgroup-v2.html,
  https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html). `cpu.weight` (1 to 10000,
  default 100) is work-conserving: only children that can use the CPU at the moment take part. `cpu.max` is a
  bandwidth cap, so a quota leaves cores idle (an inference from its definition; the docs do not use the word).
  `cpu.idle` is "the cgroup analog of the per-task SCHED_IDLE" policy; systemd sets it with `CPUWeight=idle`
  (added in systemd 252 per its NEWS), relative to the cgroup's siblings. `memory.high` throttles and never invokes
  the OOM killer; `cgroup.freeze` freezes a subtree. Taken here: the Wave 0 elastic-cpu shape and its correction 6.
- **Bazel remote caching and the Remote Execution API** (https://bazel.build/remote/caching,
  https://github.com/bazelbuild/remote-apis). The action cache maps action hashes to result metadata and a
  content-addressable store holds outputs; an Action is identified by the digest of its encoding, which carries
  the command and input-root digests. Taken here: land-check verdicts keyed by input tree plus tool identity.
- **Buck2** (https://buck2.build/docs/about/why/, https://buck2.build/docs/users/remote_execution/). "Buck2 is remote
  execution first": local execution is a special case of remote, it speaks Bazel's Remote Execution API, and its
  DICE engine recomputes only invalidated nodes. Taken here: one placement function for local and remote hosts.
- **sccache** (https://github.com/mozilla/sccache, docs/Rust.md). Caches rustc output on local disk or remote stores
  (S3, Redis, GCS, GitHub Actions and others) and offers distributed compilation; crates that invoke the linker
  (`bin`, `dylib`, `cdylib`, `proc-macro`) cannot be cached and incremental crates are passed through. Taken here:
  no new shared cache; each host keeps its own disk cache because a cache shared with the laptop would put gex45's
  compiler output into laptop binaries.
- **Quasar** (Delimitrou and Kozyrakis, ASPLOS 2014,
  https://www.csl.cornell.edu/~delimitrou/papers/2014.asplos.quasar.pdf). Does not rely on reservations: it
  classifies workloads to estimate the effect of resource amount, type and interference, monitors and readjusts,
  and raised utilization 47% on a 200-server cluster. Taken here: sizing from measured behaviour by key, with
  backoff to similar workloads when a command is new.
- **Claude Code cloud sessions** (https://code.claude.com/docs/en/claude-code-on-the-web) and **GitHub Actions
  billing** (https://docs.github.com/en/billing/concepts/product-billing/github-actions). A cloud session "counts
  toward your plan's usage limits" with "no separate charge for the cloud VM", and a setup script is cached when it
  finishes in about five minutes; standard GitHub-hosted runners are free in public repositories, and self-hosted
  runners are free. Taken here: the anthropic-cloud and Actions rows of the targets table.


## Reproducing the baseline

All read-only. Copy each host's usage log and watchdog samples into a scratch folder, load them into SQLite with
`jq -c -R 'fromjson? | . + {host: $h}'` and `.import`, then:

- queue wait: `row_number() over (partition by host, class order by queuedSeconds)` and take the 50th and 90th
  percentiles;
- idle held: `sum(chargedCpus * wallSeconds) / 60` where `meanCores < 0.1 * chargedCpus`; per sample, the same over
  each sample's `leases` array with `recentCores < 0.1 * cpus`;
- other-host room: for each heavy or moderate ticket with `queuedSeconds > 60`, the other host's `sum(chargedCpus)`
  over runs whose `[start, at)` covers the ticket's queue start, and its count of tickets queued at that moment;
- sizing error: `cpuSeconds / (chargedCpus * wallSeconds)` and `peakMemoryMiB / chargedMemoryMiB`, censored where
  `peakMemoryMiB >= 0.97 * reservedMemoryMiB`;
- walk-forward: window `partition by host, owner, class, command order by at rows between 20 preceding and 1
  preceding`, prediction `exp(avg(ln(wallSeconds)))`.
