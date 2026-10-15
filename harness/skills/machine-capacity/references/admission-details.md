# Admission details

Exact thresholds and output fields behind the rules in `SKILL.md`. `bun "$capacity" status` and `probe` print the live values; the helper source owns the thresholds, so change them there and not in a parallel calculator.

Contents:
- Run sizing and charging
- Native sessions and GPU
- Profiles and mode
- Queue and spawn gate
- Watchdog and usage records

## Run sizing and charging

- Omit `--class` on `run` to size from the usage log: p90 peak cores and memory of the shape's last 20 runs plus 25%, gpu when GPU-busy over a fifth of the run, moderate until three runs exist. The admission line shows `sizing`, with `p75Seconds` from the last 20 exact-signature runs' wall time (null and `uncalibrated` under three).
- A declared moderate, heavy, native or gpu `run`/`session` is charged, once three runs of its owner's leading letters plus command shape exist, the p90 of per-run p90 cores within 1 and 1.5x the class and p90 peak memory plus 20%. The scope keeps the declared CPUQuota/MemoryHigh; admission and the spawn gate count the charge; `status` shows `cpus` beside `declaredCpus` and `evidenceRuns`.
- A heavy run is charged and committed at least its declared 6 CPUs, so at most three heavy builds share the 20-CPU line.
- Agent leases charge 768 MiB without reserving local CPU.
- Lease lifetimes: batch `session` default 30 minutes, batch maximum one hour, exclusive maximum 15 minutes; only native sessions have no default deadline.
- Native-only `--memory-gib 1.5` (positive whole-MiB values, memory and CPU admission rules still apply) charges that client 1536 MiB instead of 4096 MiB and applies the same MemoryHigh to that client only; for measured smaller offline clients.

## Native sessions and GPU

- `session --class native --owner OWNER -- COMMAND ARG...` for Warcraft clients and private desktops: one scope per client, two per pair.
- Native DEFER_GPU_BUSY while GPU busy averages at least 85% over 5 s with no gpu lease running; attended DEFER_GPU_CLIENTS while four Warcraft clients run. `status` and `probe` show `gpuBusyPercent`, `gpuClients` and `gpuLeases`.
- gpu DEFER_GPU_SLOTS at two gpu leases; DEFER_NATIVE_WAITING for a minute after a native client was deferred, because native clients outrank renders.
- Scale native pairs one at a time and stop adding above protectedCpuSomeAvg10 20 in either profile.

## Profiles and mode

- CPU weights: compositor session 300, native 200, app 100, batch agent 20.
- Attended batch stays within cores minus four and 20% RAM available, and defers while protected-slice CPU some avg10 is at least 10%.
- Unattended stays within all cores and an 8 GiB available-memory floor, never above a quarter of RAM (small hosts such as nexus).
- Leased memory is capped at 75% RAM in both profiles.
- swayidle sets `mode away` after 10 minutes idle and `mode present` on input; auto without an away signal is attended. `mode` and `probe` show the active mode and profile.

## Queue and spawn gate

- Moderate and heavy work queues in arrival order while declared batch CPUs exceed the core limit or system CPU some avg10 exceeds 30%.
- Queued exclusive work drains heavy jobs while moderate/update:wisp continue; it starts after batch leases end and blocks new batch work until release, without blocking native clients.
- Spawn gate: no new compute-using worker when committedBatchCpus (each live batch lease at max(reserved, measured cores) plus unleased heavy load, which includes all agent.slice work outside agent-capacity.slice, such as local Codex sessions) reaches aggregateCpuLimit, or protectedCpuSomeAvg10 exceeds 20. Queue, use the farm or use cloud workers instead.
- Retry a DEFER from probe/reserve/native after a known release or 30 seconds.

## Watchdog and usage records

- `status` shows holders, remaining seconds, queue order, each run lease's `measured` cores, memory and GPU beside its reservation, and `unleasedHeavy` (cgroups outside leases, session and native slices over one core in the capacity-watchdog's latest 30 s sample, with top processes and claude or codex `sessions`).
- `~/.local/state/agents/watchdog/incidents.md` and `capacity-watchdog report` list the 30 s sampler's unleased-tree and stray git maintenance incidents and 5/30-minute pressure windows; the `[routine:watchdog]` cron reads them every 5 minutes.
- `~/.local/state/agents/machine-capacity-usage.jsonl` holds each released run's measured CPU seconds, mean/peak cores, peak memory and GPU seconds by owner and command shape.
