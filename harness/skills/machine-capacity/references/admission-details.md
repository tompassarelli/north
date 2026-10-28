# Admission details

Exact thresholds and output fields behind the rules in `SKILL.md`. `bun "$capacity" status` and `probe` print the live values; the helper source and `src/capacity/admission.rs` (the decision behind `north capacity admission`) own the thresholds, so change them there and not in a parallel calculator.

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
- A running batch lease commits 1.25x the larger of its measured cores and its estimate (heavy: at least its charge), never more than its scope's CPUQuota, so a 1-CPU gpu render commits at most 1 CPU whatever its command family's history estimates; `status` marks a capped lease `cpuBasis: scope-quota`.
- Agent leases charge 768 MiB without reserving local CPU.
- Lease lifetimes: batch `session` default 30 minutes, batch maximum one hour, exclusive maximum 15 minutes; only native sessions have no default deadline.
- Native-only `--memory-gib 1.5` (positive whole-MiB values, memory and CPU admission rules still apply) charges that client 1536 MiB instead of 4096 MiB and applies the same MemoryHigh to that client only; for measured smaller offline clients.

## Native sessions and GPU

- `session --class native --new-clients N --owner OWNER -- COMMAND ARG...` for Warcraft clients and private desktops: one scope per client, two per pair.
- Native DEFER_GPU_BUSY while GPU busy averages at least 85% over 5 s with no gpu lease running; DEFER_GPU_CLIENTS when live Warcraft clients plus the request's `--new-clients` exceed the host's `warcraft_clients` in `[hosts.NAME]` of agents/orchestration.toml (a measured count, applied in either profile), else four when attended. A client is a Wine prefix (`STEAM_COMPAT_DATA_PATH`, else `WINEPREFIX`, else the process itself) holding a `Warcraft III` process, so two processes of one client count once (north#181; `AGENT_CAPACITY_PROC` names another /proc root). `status` and `probe` show `gpuBusyPercent`, `gpuClients` and `gpuLeases`.
- A native request with `--new-clients` or `--new-desktops` above 0 and every gpu request defer as DEFER_GPU_APERTURE (the line carries `bar1FreeMiB` and `bar1NeedMiB`) while NVIDIA BAR1 Free, read live at decision time from `nvidia-smi -q -d MEMORY` (`AGENT_CAPACITY_NVIDIA_MEMORY` names a file of that output instead; no nvidia-smi or no BAR1 section gives no aperture signal; an nvidia-smi still running after 1 s, well inside the admission lock's 2 s stale takeover, defers the same requests as DEFER_GPU_APERTURE_TIMEOUT) is below the request's measured cost: `bar1_desktop_mib` per declared desktop, `bar1_client_mib` per declared client or gpu run, plus `bar1_margin_mib` in harness/defaults.toml, whose margin `bar1_margin_mib` in `[hosts.NAME]` of agents/orchestration.toml replaces (north#179, where a full aperture crashed new desktops' Xwayland; north#254). `north private-desktop start` declares `--new-desktops 1 --wait-seconds 1800`, so a desktop waits in the native line before its compositor starts; one started inside an admitted capacity scope waits up to 1800 s for the same BAR1 cost before it starts, then exits 75.
- gpu DEFER_GPU_SLOTS at two gpu leases; DEFER_NATIVE_WAITING for a minute after a native client was deferred, because native clients outrank renders.
- While a native lease started with `--quiet` is live, new moderate, heavy, exclusive and gpu leases queue as DEFER_NATIVE_QUIET (the QUEUED line names `quietOwners`) and start within a second of the last quiet lease ending; agent, native, critical and tom leases still run, and native leases without `--quiet` never hold batch work. A wait that outlasts the job's `--timeout-seconds` exits 75 naming the evidence run.
- Only the native owner's own build steps pass `--beside-native NATIVE-OWNER` on `run`, `session` or `probe`; the usage log records it as `besideNative`.
- Scale native pairs one at a time and stop adding above protectedCpuSomeAvg10 20 in either profile.

## Profiles and mode

- CPU weights: compositor session 300, native 200, app 100, batch agent 20.
- Attended batch stays within cores minus four and 20% RAM available, and defers while protected-slice CPU some avg10 is at least 10%.
- Unattended stays within all cores and an 8 GiB available-memory floor, never above a quarter of RAM (small hosts).
- Leased memory is capped at 75% RAM in both profiles.
- swayidle sets `mode away` after 10 minutes idle and `mode present` on input; auto without an away signal is attended. `mode` and `probe` show the active mode and profile.

## Queue and spawn gate

- Moderate and heavy work queues in arrival order while declared batch CPUs exceed the core limit or system CPU some avg10 exceeds 30%. A ticket the host itself holds by class (gaming-deferred classes while gaming, heavy builds under the root disk floor as DEFER_DISK_FREE) holds no place ahead of a class that host condition does not hold. Each queued ticket records its latest DEFER reason, which `status` shows as `reason` on every `queued` entry.
- Heavy and exclusive work defers as DEFER_RUN_QUEUE while `runQueueLoad`, the 1-minute load average from `/proc/loadavg` (`AGENT_CAPACITY_LOADAVG` overrides the path; an unreadable file gives no load signal), exceeds the thread count, whatever the leases account for. When `AGENT_CAPACITY_NEED` names the box's needs as a comma-separated `NEED` or `NEED:REPO` list, which `north fleet launch` sets from the brief's local needs for Claude sessions, the line carries `offload`: the first host other than this one in `[placement] order`, then any other `[hosts.NAME]` with an `ssh` destination, whose `serves` covers every need, read from the same orchestration.toml placement reads; no `offload` when none serves them all. Critical, tom, native, agent and moderate never wait on load average, and nothing else in admission reads it. `capacity-run-queue-overload.recorded.json` pins the north#156 overload.
- Queued exclusive work drains heavy jobs while moderate/update:wisp continue; a heavy job whose deadline ends before every running batch lease's deadline still starts, because it ends before the drain does. The exclusive starts after batch leases end and blocks new batch work and new `--quiet` native evidence runs (DEFER_EXCLUSIVE) until release, except a quiet run started inside that exclusive lease (its `AGENT_CAPACITY_LEASE`), without blocking native clients; `status` lists the leases each queued exclusive waits for as `waitingFor`.
- A native start (`--new-clients` or `--new-desktops` above 0) given `--wait-seconds` waits in its own arrival-order line, beside the gpu line and apart from CPU work; any native start, probe or session, defers as DEFER_QUEUED while an earlier native ticket waits, so no later client start takes the slot it waits for, except that a waiter whose own `--new-clients` and `--new-desktops` would defer as DEFER_GPU_CLIENTS holds no place in line, so a smaller start behind it takes a slot that frees and the oldest waiter that fits takes the next (north#219). Its polls also refresh the native-waiting marker that holds gpu work, though a native decision that runs (such as a `--new-clients 0` session) clears it until the next poll. `--wait-seconds` on any other request is refused. `status` lists the native tickets under `queued` with their `newClients` and `newDesktops`. Inside a private desktop, whose own D-Bus has no systemd, the helper reaches the user manager over `unix:path=/run/user/UID/bus` and the command keeps the desktop's bus (north#248).
- Spawn gate: no new compute-using worker when committedBatchCpus (each live batch lease's commitment plus unleased heavy load, which includes all agent.slice work outside agent-capacity.slice, such as local Codex sessions) reaches aggregateCpuLimit, or protectedCpuSomeAvg10 exceeds 20. Queue, use the farm or use cloud workers instead.
- Retry a DEFER from probe/reserve after a known release or 30 seconds; a native client start waits with `--wait-seconds` instead.

## Watchdog and usage records

- `status` shows holders, remaining seconds, queue order, each run lease's `measured` cores, memory and GPU beside its reservation, and `unleasedHeavy` (cgroups outside leases, session and native slices over one core in the capacity-watchdog's latest 30 s sample, with top processes and claude or codex `sessions`).
- `~/.local/state/agents/watchdog/incidents.md` and `capacity-watchdog report` list the 30 s sampler's unleased-tree and stray git maintenance incidents and 5/30-minute pressure windows; the `[routine:watchdog]` cron reads them every 5 minutes.
- `~/.local/state/agents/machine-capacity-usage.jsonl` holds each released run's measured CPU seconds, mean/peak cores, peak memory and GPU seconds by owner and command shape, with `queuedSeconds`, the admission wait the run spent queued.
