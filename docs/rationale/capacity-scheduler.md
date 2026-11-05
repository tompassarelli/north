# Capacity scheduler: elastic, learned, fleet-wide placement by predicted finish time

Owner: north#259

Plan of 2026-11-04 (laptop time, +07). Tom asked for a capacity system that uses every core and GPU well, learns
from what actually ran, updates fast and rests on measured estimates, at maximum ambition and without ceremony
(2026-11-04). This document measures today's admission over the last 7 days, sets the design on top of the six
boxes already in flight (Wave 0), and splits the rest into boxes. It edits no code. A reviewer of a different tier
will try to break it, so every assumption is stated where it is used and collected in [Assumptions](#assumptions).
Revised the same day after an adversarial review (changes requested, 17 findings plus one on ceremony); each
finding and what changed for it is in [Review disposition](#review-disposition-2026-11-04).

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

This table was computed on committed-CPU accounting, which elastic-cpu removes, and much of it is the accounting
burst that elastic-cpu alone fixes. It is therefore not evidence that placement helps once CPU is elastic. Box W2-0
re-measures it on memory and slots before W2-3 and W2-4 are staffed.

This table was computed on committed-CPU accounting, which elastic-cpu removes. Much of it is the accounting
burst that elastic-cpu alone fixes, so it is not evidence that placement will help once CPU is elastic. Box W2-0
re-measures it on memory and slots before W2-3 and W2-4 are staffed.

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
- Offload concurrency, 2026-11-04 morning: a single gex45 offload slot per repository serialized every worker's
  offloaded run for up to 600 s. 32da14e68 gave each repository a pool of three slots, and 1b699074a made a run
  waiting for a slot print which head holds it every minute. The larger fix was measured the same morning: when
  workers verify narrowly (the checks their change touches) and leave the full suite to the landing train, which
  runs it once on the exact tree, the per-worker offload queue disappeared. Nothing in this plan sends a worker's
  full suite to an offload host; placement moves narrow checks and builds, and the full suite stays the train's.

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
| Placement by predicted finish time | keep if W2-0 still finds waiting to move, starting from a deterministic rule | 86 laptop tickets (14.2 h) and up to 1,115 gex45 tickets (266.6 h) waited while the other host had room, measured on the CPU accounting elastic-cpu removes |
| Reservation backfill from predicted durations | keep, on memory and slots only | with CPU elastic, the 0.1-core head-of-line case disappears; memory, exclusive drains, GPU slots and native client caps still block |
| Result cache by tree hash, runtime reuse | keep (Wave 0) | 32 to 33% of core-hours went to failing runs; most land checks fail |
| Shared sccache on gex45 | drop as a new item | each host already shares one disk cache across leases (96854b985); sharing it with the laptop breaks D2/B24; build-cost.md measured `target-seed` ahead of sccache (31 Oct) |
| Calibration report with drift alarms | keep, with aggregate alarms that switch placement and backfill off, and a retune proposal report instead of tuned state | thresholds in defaults.toml are mostly priors waiting on counts; per-key alarms over hundreds of keys would misfire about 4% of the time each |
| GPU on the same model, batch GPU on gex45 | keep, measurement first; never on the attended laptop | gex45 records 0 GPU-seconds; the laptop's gpu class is 8% busy |

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
   its own records. Every row carries its host, and every sizing level (exact command down to class) filters to the
   asking host: a host sizes its own memory and wall only from its own rows. Remote rows feed placement's
   prediction for that remote host and nothing else. `cold_h` and the speed factor expire after 14 days, and once a
   key has 3 runs on a host its per-key per-host walls replace the global speed factor.
3. **Placement (new), grown from a deterministic rule.** Eligibility is a hard filter applied before any cost:
   - remote eligibility fails closed: a command is offloadable only if its command key is on an explicit allowlist
     of code-only checks (`cargo test`, `cargo clippy`, `cargo check`, `cargo nextest`, headless renders whose
     output is compared, not shipped). Builds, `nix build`, installs, deploys, anything whose output the laptop
     runs or installs, and any unknown key stay on the laptop. Today's `refusal()` in `build_offload.rs` refuses
     only landing, already-offloaded and `need`-bearing requests, so it fails open; W2-3 replaces that with the
     allowlist;
   - a lane's `target/` is pinned to the host it was created on: moving a lane resets its cache and pays `cold_h`,
     and no `target/` or sccache directory is copied between hosts;
   - the laptop is eligible for batch only within the attended reserve and game reserve; while Tom's session or a
     game is active, batch beyond the reserve is not eligible there at all, whatever it would cost.

   The first rule (W2-3): stay local; if the ticket is deferred longer than a threshold and an eligible remote host
   serving its needs has memory headroom for its p90 anon and is under its offload cap, offload. Only if replay
   shows that rule leaves waiting does it grow into the finish-time comparison: for each eligible target h,
   `F(h, j) = now + wait_h(j) + ship_h(j) + cold_h(j) + run_h(j)`, every term at the same quantile (p90), where
   `wait` simulates h's queue with the backfill rule and counts offloads already dispatched to h but not yet
   started as pending load, and the laptop's `run` adds its expected freeze time from the last hour's PSI freezes.
   A local ticket moves only if the remote F beats the local one by the larger of 60 s or 20%; each target takes
   at most its offload cap concurrently (gex45's pool is three slots per repository, 32da14e68); 5% of eligible
   tickets are placed on the runner-up to keep `cold_h`, speed and counterfactual finish measured. Two callers:
   - command placement: `machine-capacity run` and build offload choose among laptop, gex45 and Nexus;
   - session placement: `north fleet launch` and `agents plan --place` choose among laptop, gex45, Nexus and
     anthropic-cloud for a worker; it records predicted against actual box wall first and keeps local as the
     default until that record shows facets predict box wall (W2-5).
4. **Backfill (new).** The head ticket that does not fit gets a reservation: the earliest time its memory and slots
   fit given running leases' predicted p90 ends. A later ticket starts now only if it fits now, its estimate is
   warm (at least 3 runs at its answering key level on this host), and either ends (predicted p90) before that
   reservation or still leaves the reservation's memory and slots free. A cold or unknown estimate never backfills
   unless the reservation's memory and slots stay free whatever it does. A backfilled lease that overruns is not
   killed or frozen (freezing frees no memory); the miss is recorded and lowers that key's coverage, which widens
   its next estimate. The bound is on the head's delay, not its age: `backfill_seconds` (300) becomes the most a
   head's start may slip past its reservation because of passed tickets; once the head has slipped that far,
   nothing more passes it. Today `backfill_seconds` in `queued_ahead.rs` is how long a head has waited before
   passing stops, which wastes backfill for a head whose reservation is far off and bounds nothing for one that is
   near. This is EASY backfill with system predictions instead of user estimates.
5. **Never twice (Wave 0 plus W3).** Land-check verdicts by tree and tool identity per host; runtime binary reuse;
   lane `target-seed` plus the per-host sccache disk cache; skip a queued runtime build only while a newer queued
   build of the same key is queued or running and has not failed, and build the skipped predecessor if the newer
   one fails; cheap land checks keep running in parallel, and only the expensive ones (predicted over 120 CPU-s) are
   ordered by predicted fail probability per CPU-second and cancelled at the landing's first failure. Estimates are
   keyed by outcome (pass, fail, cancelled) so a fail-fast check's bimodal wall does not mix into one quantile.
6. **Calibration (Wave 0 report, then W3).** `north capacity calibration` prints, per key level and host, predicted
   versus actual p50 error and p90 coverage over the last 7 days. Alarms are aggregate, not per key: one fires into
   the watchdog incidents file when a host's or class's p90 coverage over its last 200 runs drops under 85%, or when
   the replayed head-start delay of the last day passes its bound. The same signal is a circuit breaker: while it
   is raised, placement and backfill switch off and admission runs today's FIFO-on-memory policy, and it clears
   only after 200 runs at 88% or better. The retune is a report, not state: `north capacity retune` replays the
   week and prints proposed values for the pressure band's batch side, memory margin and backfill bound with the
   replayed waits for each; a person or worker commits a chosen value to `harness/defaults.toml` with a `source`
   line naming the report. Tom's protection values (attended reserve, game reserve, the protected-slice PSI marks)
   are never proposed or tuned.
7. **GPU.** gex45 gets per-process GPU time from NVML (`nvidia-smi --query-accounted-apps` or pmon) so its gpu and
   native runs carry GPU-seconds. GPU stays slot- and occupancy-admitted. While Tom is present, batch GPU work
   (renders, captures, headless Chrome) is not eligible on the laptop: it waits for gex45 however long gex45's GPU
   queue is. The laptop's 890M goes to Tom's play and native checks. Renders and captures that serve as golden
   baselines are produced and compared on one named GPU (gex45's), because another GPU or driver can differ.
8. **Waiting is visible.** A ticket waiting in admission or for an offload slot prints one line a minute with its
   reason, what holds the resource and its predicted start (1b699074a does this for offload slots). From the same
   Actions pull, a deterministic check reads main's latest conclusion per repository and writes one incident line
   when main is red, so a red main is not left for a worker to discover.

### Placement targets and their rules

| Target | Takes | Never takes | Cost model |
| --- | --- | --- | --- |
| laptop | anything it serves; required for whatever produces a binary the laptop runs, Tom's play builds and native checks | batch beyond the attended reserve and game reserve (a hard filter, not a cost); batch GPU while Tom is present | measured; batch at cpu.idle, plus expected freeze time |
| gex45 | allowlisted code-only check keys and its `serves` needs; batch GPU while Tom is present | any unlisted or unknown key; any artifact the laptop will run or install (D2/B24); a laptop lane's `target/`; authority over laptop gates or laptop sizing | measured, plus ship seconds |
| Nexus (3 cores, 5.9 GiB) | moderate and gpu-free tickets of at most 2 cores and 2 GiB predicted; never the runtime (`builds_runtime = false`) | heavy builds | measured, plus ship seconds |
| GitHub Actions | the workflows already there (public repositories: standard hosted runners are free; the gex45 farm is gex45's own capacity) | private repositories' secrets beyond what workflows already hold; paid larger runners | job duration and queue from the jobs API (W1-6) |
| anthropic-cloud | whole worker sessions for code-only boxes in north, makgora and wisp | south or any private repository; boxes needing gex45, Nexus or laptop state; native, GPU or sign-in needs | plan usage of the launching account, plus a cold clone and cold cargo cache each session |

**anthropic-cloud, modelled honestly.** A cloud session (4 cores, 15 GB) has no separate charge: it counts toward
the launching account's plan usage like a local session, so its builds are free compute and its tokens cost what a
local worker's cost. The one-time $250 credit on claude:a was spent by 2026-10-26 and expires 2026-11-05 07:59Z,
so today's rule (`place_anthropic_cloud` in `crates/north-ops/src/ops/pacing/placement.rs`, which places the cloud
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
capacity pull` subcommand beside it, `crates/north-core/src/capacity/host/estimate.rs`. Spec: pull each
`[hosts.NAME]` ssh host's `capacity_runs` rows newer than the last pulled `at` into the laptop's capacity.db with a
`host` column; the unique key is `(host, source_id)`, since `source_id` is `jsonl:{path}:{index}` and collides
across hosts; the laptop only reads remote rows; every sizing level in `estimate.rs` (`levels`, `same_exact` and
the coarser fallbacks) filters to the asking host. Done when: a fixture test loads laptop rows, sizes a key, adds
gex45 rows for the same key (including a colliding `source_id`) and gets the identical laptop sizing with all
rows kept; and a second pull of the same fixture adds 0 rows.

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
single-module. Surface: rust. Verify: local-test. Files: `crates/north-ops/src/ops/pacing/placement.rs`
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
`crates/north-ops/src/ops/dispatch/cloud.rs`, `harness/skills/workers/references/cloud.md`. Spec: `north cloud
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

### Wave 2: replay first, then backfill and placement (after Wave 1)

Every Wave 2 and Wave 3 Done-when is a replay or fixture check that runs inside the box. Checks that need hours or
days of live traffic are in [Live audits](#live-audits), dated, and run by the lead after the boxes land.

**W2-1. Admission replay simulator.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust.
Verify: local-test. After: elastic-cpu, capacity-model, W1-1. Files:
`crates/north-core/src/capacity/pressure_target_tests.rs` grows a `capacity/replay.rs`. Spec: replay a recorded
window's tickets (arrival, class, needs, anon peak, actual wall) through `decide` with a policy switch (today,
elastic, elastic plus backfill, elastic plus placement, elastic plus both). Predicted durations come from
walk-forward estimates (each ticket sees only rows before its arrival), never from its own actual wall; actual
walls only decide when a replayed lease ends. Prints, per host and class, wait p50, p90 and total hours, idle-held
core-minutes, and for backfill the head-start delay against FIFO (p90 and max). The replay ranks policies against
each other; it does not predict absolute waits, because it has no record of unleased load or of cpu.idle slowdowns
and freezes. Done when: replaying Oct 28 to Nov 2 under today's policy reproduces the measured wait p50, p90 and
total hours for every class on both hosts within 30%, and the held-out day Nov 3 is then replayed with the same
settings and its errors printed beside them; for the 30 hours with watchdog pressure samples (Nov 2 16:19Z to Nov 3
23:40Z) the replay also feeds the sampled unleased load and prints both errors.

**W2-0. Re-measure what placement and backfill could move.** Category: research. Spec: measured. Scope: one-file.
Surface: docs. Verify: none. After: W2-1, elastic-cpu and capacity-model landed. Files:
`docs/rationale/capacity-scheduler.md` (results). Spec: re-run the "waited while another host had room" query of
section 4 using memory headroom and slots instead of committed CPU, on rows recorded since elastic-cpu landed, and
replay the 7-day window under elastic alone. Done when: this document records, per host, tickets and hours still
waiting over 60 s under elastic alone and how many of them had an eligible host (allowlisted key, needs served,
memory and slot room) free; and states the staffing verdict: W2-3 and W2-4 are staffed only if elastic alone
leaves at least 10 h a week of such waiting, otherwise they are parked with that number.

**W2-2. `north capacity predict`.** Category: feature. Spec: measured. Scope: single-module. Surface: rust. Verify:
local-test. After: W1-1, W1-3, W2-1. Files: `crates/north-core/src/capacity/host/estimate.rs`, a `predict`
subcommand. Spec: `north capacity predict -- CMD` prints per eligible host `wait`, `ship`, `cold`, `run` at p90 and
`finish`, with the key level used and whether the estimate is warm; `wait` uses W2-1's queue simulation under the
policy admission is running (FIFO on memory until W2-4 lands, then backfill). Done when: walk-forward over the
recorded Oct 28 to Nov 3 laptop heavy runs, the p90 covers between 85% and 95% of actual walls and the printed
median interval width is no wider than 5x the median wall.

**W2-3. Command placement, deterministic rule first.** Category: feature. Spec: measured. Scope: cross-module.
Surface: rust. Verify: local-test. After: W2-0 (verdict: staff), W2-1, W2-2, W2-4. Files: `build_offload.rs`,
`admission.rs` (offload line), `run.rs`. Spec: replace `refusal()`'s open default with the fail-closed key
allowlist and lane `target/` pinning; replace the room threshold with the design's deterministic rule (deferred
past a threshold, eligible remote with memory headroom under its offload cap), counting dispatched-not-started
offloads as pending load on the target; laptop eligibility is a hard filter before any cost; 5% exploration on the
runner-up; record chosen and runner-up with their predictions. The finish-time comparison with its 60 s or 20%
margin is added only if the replay shows the rule leaves at least a quarter of W2-0's movable hours. Done when: the
W2-1 replay of Oct 28 to Nov 3 shows (a) zero tickets from non-allowlisted or unknown keys placed off the laptop,
(b) zero batch placements on the laptop past the reserve while a game or Tom's session was active, (c) no target
over its offload cap at any instant, and (d) total waiting over 60 s below elastic-without-placement's by the
margin W2-0 found movable, with p90 wait no worse for any class on either host.

**W2-4. Reservation backfill on memory and slots.** Category: feature. Spec: measured. Scope: single-module.
Surface: rust. Verify: local-test. After: W2-0 (verdict: staff), W2-1, elastic-cpu. Files: `queued_ahead.rs`,
`admission.rs`. Spec: the backfill rule in the design; `backfill_seconds` redefined as the bound on head-start
slip past its reservation; cold or unknown estimates never backfill unless the reservation stays free. Done when:
the replay shows, on both hosts, total and p90 heavy wait below elastic-without-backfill's, head-start delay
against FIFO with p90 under 60 s and max under `backfill_seconds`; and unit tests pin that a ticket whose p90 end
is after the reservation starts only if the reservation's memory stays free, that a cold-estimate ticket does not
pass a head whose memory it would take, and that nothing passes a head that has slipped `backfill_seconds`.

**W2-5. Session wall record, then session placement.** Category: feature. Spec: measured. Scope: cross-module.
Surface: rust. Verify: local-test. After: W1-4, W1-5, W2-2. Files: `crates/north-ops/src/ops/pacing/placement.rs`,
`placement/launch.rs`, the worker ledger. Spec: record each box's predicted compute (ledger facets), actual box
wall, model time and placement in the ledger first; rank laptop, gex45, Nexus and anthropic-cloud (cloud including
measured session start, clone and cold build per repository, and only accounts whose headroom W1-4 admits and whose
launch W1-5 proved) in the verdict line, but keep local as the chosen target until the audit below shows facets
predict box wall. Done when: on the recorded worker ledger, a fixture of past boxes prints all four targets'
estimates per box, the verdict picks local for every one, and the printed rank correlation between predicted
compute and actual box wall is reported.

**W2-6. Offload concurrency from the remote host's own admission, not a slot count.** Category: feature. Spec:
measured. Scope: cross-module. Surface: rust. Verify: local-test. After: the offload dev-shell lane (same file),
elastic-cpu, and the pending-load rule of W2-3 (review finding 8). Files:
`crates/north-ops/src/ops/land_offload/ship.rs` (`SLOTS`), `build_offload.rs`. Spec: each offloaded command runs on
the remote host under that host's own `north capacity host run` admission (memory and pressure once elastic CPU is
live); the per-repository lock only guards one worktree and target dir, created on demand, so concurrency follows
measured room; the asking host counts its dispatched-not-started offloads as pending load on the target. Done when:
`rg -n 'const SLOTS' crates/north-ops/src/ops/land_offload` prints nothing; a fixture test with remote admission
admitting four and deferring the fifth shows four offloads running at once in four on-demand worktrees and the
fifth waiting with the remote admission's reason printed; and a test shows a target with two dispatched-not-started
offloads reported with that pending load by the asking host.

Delivers alone: a replay that ranks policies, a measured answer to whether placement and backfill still pay after
elastic CPU, and backfill and placement only where that answer says so.

### Wave 3: calibration, waste and GPU placement (after Wave 2, independent of each other)

**W3-1. Aggregate alarms and circuit breaker.** Category: tooling. Spec: measured. Scope: single-module. Surface:
rust. Verify: local-test. After: W2-1. Files: the calibration command, `bin/capacity-watchdog` incidents writer,
`admission.rs`. Spec: the aggregate alarm rule and circuit breaker in the design. Done when: a fixture where one
host's class coverage drops to 70% over 200 runs writes one incident line and sets the breaker, after which
admission's placement and backfill paths report off and FIFO-on-memory decides; a fixture of 200 runs at 90%
clears it; and replaying the recorded Oct 28 to Nov 3 walk-forward estimates prints the number of alarms raised.

**W3-2. Retune proposal report.** Category: tooling. Spec: measured. Scope: single-module. Surface: rust. Verify:
local-test. After: W2-1. Files: `capacity/replay.rs`, a `retune` subcommand. Spec: grid-search the batch side of
the pressure band, `capacity_memory_margin` and `backfill_seconds` over the last week's replay and print each
candidate's replayed waits and head delay beside the defaults; it writes no state file and admission reads
nothing from it. A chosen value is committed to `harness/defaults.toml` with a `source` line naming the report and
its date. The attended reserve, game reserve and protected-slice PSI marks are not in the grid. Done when: `north
capacity retune` on the recorded week prints the table with the defaults' row first, and a test shows the
protection values are absent from its grid.

**W3-3. Fail-fast order for expensive land checks.** Category: performance. Spec: measured. Scope: cross-module.
Verify: local-test. Surface: rust. After: capacity-model, land-cache. Files: the land-check runner in
`crates/north-landing/`. Spec: cheap checks (predicted under 120 CPU-s) keep running in parallel and always finish,
so every landing still reports all their failures; expensive checks are ordered by predicted fail rate per
CPU-second (by check and changed-path class, from records) and cancelled at the first failure; cancelled checks
write an outcome row `cancelled`, and estimates are keyed by outcome. Done when: replaying the recorded land-check
runs of Oct 28 to Nov 3 (each check's actual wall, CPU-s and exit) through the new order against cancel-on-first-
failure with today's order prints both median CPU-seconds before the failure verdict, the new order's is lower,
and passing landings' wall is no more than 5% above today's.

W3-3 result (2026-11-05). The Oct 28 to Nov 3 window holds no per-check records for north's land checks: both
hosts recorded each landing as one `land-check` lease over the whole safe-push run (782 rows on gex45, 609 on the
laptop), offloaded checks wrote no row of their own, and the only per-check rows are 46 after-landing checks between
11:24 PM and 11:59 PM on Nov 3. The replay therefore cannot run on that window. The runner now writes those
records: each land check appends a row to `check-timings/outcomes.tsv` (start, landing head, changed-path class as
the sorted top-level directories, `passed`, `failed` or `cancelled`, exit, wall, CPU-s from the runner's reaped
child time for a local check, `-` for an offloaded one, whose cost falls back to wall). Estimates use the newest 20
completed rows at check and path class when that key has 3, else at check; cancelled rows count toward neither the
fail rate nor the cost, and the cost is kept separately for passing and failing outcomes. A check with no estimate,
or predicted under 120 CPU-s, is cheap: it runs first, as today, and always finishes. Expensive checks run in
descending fail rate per expected CPU-second; local ones one at a time, offloaded ones still launched together so
passing wall never grows, and every expensive check still pending or running at the first failure is cancelled,
with an `event land check cancelled` line in the lander outcome. `north land checks --replay [--records FILE]
[--since EPOCH] [--until EPOCH]` replays recorded landings (skipping any with a cancelled row) through today's order
with cancel-on-first-failure and through the fail-fast order, learning only from earlier landings, and prints both
medians. On the fixture in `tests/it/land_checks.rs` it prints 600.0 against 200.0 CPU-s and +0.0% passing wall.
The live audit below runs it on real records once 20 failing and 20 passing landings have accumulated.

**W3-4. Skip superseded runtime builds, never leave a gap.** Category: performance. Spec: measured. Scope:
single-module. Surface: rust. Verify: local-test. After: runtime-reuse. Files: `crates/north-ops/src/ops/runtime/`.
Spec: a queued runtime build whose commit a newer queued or running build for the same host contains does not
start while that newer build has not failed; if the newer build fails, the newest skipped predecessor is built
next. Done when: fixture tests show a skipped build when the newer one is queued, the skipped predecessor built
when the newer one fails, and no skip when the newer one has already failed.

**W3-5. Batch GPU on gex45, never on the attended laptop.** Category: feature. Spec: measured. Scope:
single-module. Surface: rust. Verify: native-check. After: W1-2, W2-3. Files: `gpu_admission.rs`,
`build_offload.rs`. Spec: while the laptop is attended, gpu-class batch tickets are not eligible on the laptop and
wait for gex45's GPU slot however long its queue is; renders and captures that serve as golden baselines run only
on gex45's GPU. Done when: a fixture with the laptop attended and gex45's GPU slot busy keeps a batch gpu ticket
queued for gex45 rather than placing it on the laptop, and replaying the recorded gpu-class tickets places none on
the laptop while it was attended.

**W3-6. Red-main incident from the Actions pull.** Category: tooling. Spec: measured. Scope: single-module.
Surface: rust. Verify: local-test. After: W1-6. Files: the `north capacity actions-pull` command, the incidents
writer. Spec: after each pull, read main's latest completed run conclusion per repository; on `failure` write one
incident line naming the repository, run and commit, once per failing commit. Done when: fixture pulls with a green
then a failing then the same failing main write exactly one incident line.

Delivers alone: estimates stay honest and switch the clever parts off when they are not, failing landings stop
burning a third of the core-hours without slowing passing ones, the runtime never skips a commit for good, red main
is reported, and the laptop GPU is Tom's.

### Live audits

Run by the lead after the boxes land, each recorded here as a dated line with its result. They are not Done-whens.

- W2-2, one day after landing: logged predictions' p90 covers 85 to 95% of the next 20 heavy laptop walls.
- W2-3, one day after landing: chosen-host coverage (actual finish within its predicted p90) and, on the 5%
  exploration slice, how often the runner-up would have finished first.
- W2-4, one day after landing: live head-start delay p90 and max from admission's log.
- W2-5, one week after landing: rank correlation of predicted compute against box wall; only if it is useful does
  session placement leave its local default.
- W3-1, one week after landing: alarms raised and breaker time.
- W3-3, after 20 failing and 20 passing landings: CPU-seconds before the failure verdict, and passing landing wall,
  against the values computed from `capacity_runs` before the change.
- W3-4, one day after landing: every main commit reached the runtime or was covered by a newer passing build.
- W3-5, one attended day: no gpu-class agent run on the laptop.

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

Wave 0 is in flight. Wave 1 is seven boxes in parallel, about 20 minutes each plus landing. Wave 2 is W2-1 first,
then W2-0 and W2-2 in parallel; W2-3 and W2-4 are staffed only on W2-0's verdict, W2-4 before W2-3; W2-5 runs
beside them. Wave 3 is six boxes in parallel. Uncalibrated, with three to five workers and Wave 0 landed by about
8:00 AM: Wave 1 by about 9:00 AM, W2-0's verdict by about 10:00 AM, the rest of Wave 2 by about 11:30 AM and
Wave 3 by about 1:30 PM on 2026-11-04. The live audits follow over the next day and week.

## Assumptions

1. The 7-day window is representative except for Oct 31 and Nov 1 on gex45, which carry 354 of its 459 waiting
   hours (all classes); numbers are given per day where that matters.
2. "Another host had room" in section 4 uses committed charge only, not PSI or memory, and ignores whether the
   ticket's needs were served there; it is an upper bound, and W2-0 replaces it with a memory-and-slots measure.
3. "Under 10% CPU" is a lease using under a tenth of its charged cores, over the whole run (records) or over a 30 s
   sample (watchdog).
4. Run keys use the recorded command, which has paths redacted, so two lanes' identical commands share a key.
5. Wave 0 lands as briefed, with the corrections above. If elastic-cpu does not land, W2-4's backfill must also
   cover CPU.
6. Host speed is the same for both build hosts within the measured spread; the per-host factor is learned anyway.
7. gex45 and Nexus rows inform the laptop's placement but never its gates, its sizing or its binaries; W1-1's
   host filter and W2-3's allowlist and `target/` pinning are what enforce it.
8. The cloud VM's 4 cores and 15 GB, and its cold cache each session, are per cloud.md; a setup script's cached
   result could warm dependencies (the cloud docs say a setup script is cached when it finishes in about five
   minutes); W2-5 measures cold start per repository rather than assuming either.
9. The replay ranks policies against each other; its absolute waits are not predictions, because unleased load
   before 2026-11-02 and cpu.idle slowdowns are not in the records.

## Review disposition, 2026-11-04

An adversarial review of the first version (cbdabdee8) requested changes. Every finding was accepted; none was
shown wrong by code or data. Findings 1, 3, 4 and 8 were checked against `build_offload.rs` (`refusal()` refuses
only landing, already-offloaded and `need`-bearing requests; `first_free` takes the first free host),
`host/database.rs` (no host column) and `queued_ahead.rs`, and hold.

1. Remote eligibility failed open: now an allowlist of code-only keys, unknown keys stay local, lane `target/`
   pinned to one host (Shape 3, targets table, W2-3 check a).
2. The section 4 baseline rests on CPU accounting elastic-cpu removes: W2-0 re-measures on memory and slots and
   gates W2-3 and W2-4; W2-3 and W2-4 compare against elastic without placement or backfill.
3. gex45 rows could size laptop runs: host-qualified rows, `(host, source_id)` key, every sizing level filtered to
   the asking host (Shape 2, W1-1 Spec, Files and Done-when).
4. Backfill bounded by head age at 4.5x p90 error: now bounded by head-start slip, no backfill on a cold or unknown
   estimate, head-delay replay metrics (Shape 4, W2-1, W2-4).
5. Replay judged on one statistic: now p50, p90 and total wait for every class on both hosts, a held-out day, the
   30 sampled hours with unleased load, walk-forward estimates only, and a stated relative-ranking scope (W2-1,
   assumption 9).
6. W3-5 contradicted the GPU rule: batch GPU never on the attended laptop; golden renders on one named GPU.
7. Laptop eligibility as a cost: now a hard filter before F, plus expected freeze time (Shape 3, W2-3 check b).
8. Herding and quantile mixing: dispatched offloads count as pending load, a 60 s or 20% margin, a per-target cap,
   one quantile (p90) throughout (Shape 3, W2-3 check c).
9. `cold_h` and speed lock in: 5% exploration on the runner-up, 14-day expiry, per-key per-host data once it
   exists (Shapes 2 and 3).
10. Dependencies: W2-1 after elastic-cpu, capacity-model and W1-1; W2-2 after W2-1; W2-3 after W2-4; W2-5 after
    W1-5.
11. Done-whens needing days of traffic or unprovable: every Wave 2 and 3 Done-when is now a replay or fixture
    check; the live checks moved to [Live audits](#live-audits); W2-3 no longer compares against an unobserved
    runner-up; W2-2 bounds interval width; W3-3 compares against cancel-on-first-failure alone; W3-4 drops the
    exit 73 count; W3-1 drops the arbitrary threshold.
12. Retune as hidden mutable state: W3-2 is a proposal report committed to `defaults.toml` with a `source` line;
    protection values are excluded.
13. Per-key alarms misfire, no breaker: aggregate and class alarms that also switch placement and backfill off
    (Shape 6, W3-1).
14. Fail-fast costs latency and information: cheap checks stay parallel, only expensive ones are ordered and
    cancelled, cancelled checks write an outcome row, estimates keyed by outcome, passing landing wall guarded.
15. Supersede gap: skip only while the newer build has not failed; build the skipped predecessor on its failure.
16. Missing incidents: the single offload slot (fixed by 32da14e68, now a per-target cap in placement), silent
    waits (1b699074a for offload slots; Shape 8 extends it to admission), red main (W3-6). Also recorded: verifying
    narrowly and leaving the full suite to the exact-tree train removed the per-worker offload queue, so no box
    sends a worker's full suite to an offload host.
17. Session placement from build compute: W2-5 records box wall against facets first and keeps local as the
    default until the audit shows signal; cloud only under accounts with headroom.
18. Ceremony: W2-3 starts from a deterministic rule and grows into F only if replay shows the rule leaves waiting.

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
  overrun widens the estimate, and `backfill_seconds` bounds how far a head may slip past its reservation.
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
