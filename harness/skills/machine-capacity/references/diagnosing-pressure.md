# Diagnosing pressure and long waits

What a DEFER means, where a build went, and who holds the machine. Exact thresholds are in [admission details](admission-details.md).

## Read the state

- For diagnosis read `status` (holders, queue order, measured versus reserved, `unleasedHeavy`), the watchdog's `incidents.md` and `capacity-watchdog report`, and the usage log; the paths and fields are in [admission details](admission-details.md).
- Treat memory PSI as diagnostic and system CPU PSI as admission pacing rather than desktop harm, because batch quota throttling inflates both without hurting the desktop.
- Assign exact-scope cleanup to the owner or accountable parent, and leave signaling of peers to them, because only they know what is safe to stop.

## Why heavy work waits

- Expect a heavy admission to start while CPU pressure (PSI some avg10) sits under a target band and memory has room, not while a job count has room: below the low mark (`admit_psi_unattended_low` 20%, attended `admit_psi_attended_low` 10%, Tom's game under game reserve `admit_psi_game_low` 5%) the next queued build starts at once, one per `admit_settle_seconds` (30 s) so the next reading includes the last; between the marks it starts only if its declared CPUs fit the batch cores; at the high mark (40, 25, 20%) it waits (DEFER_RUN_QUEUE, which names an `offload` host), and declared CPUs past `admit_overcommit_unattended`, `_attended` or `_game` (1, 1, 1) times the batch cores, so committed CPUs never pass `aggregateCpuLimit` wait as DEFER_CPU_CAPACITY; memory counts the declared MiB of heavy leases younger than `admit_memory_ramp_seconds` against what is available, while a build's resident set is still growing (the laptop held 7 to 12 builds for up to 35 minutes at PSI 1 to 4% under the old counts, 2026-10-31).
- Exclusive admission defers as DEFER_RUN_QUEUE at `run_queue_psi_attended` (40%) or `run_queue_psi_unattended` (60%); facts stated without a pressure reading (fixtures, stdin) defer heavy and exclusive on the 1-minute load average once system CPU some avg10 is under 1%, else the higher of the 1- and 5-minute load averages, past the thread count, and a live host that cannot read pressure fails admission. Tickets held by DEFER_RUN_QUEUE hold no place for moderate work, and critical, tom, runtime, native, agent and moderate ignore it.
- Expect a heavy run to be charged at least its declared 6 CPUs, so at most three heavy builds share the 20-CPU line.
- Builds on a `lease_builds` host wait for a lease through the `cargo`, `bun` and `nix` shims and `depbox`, so `nix develop --command cargo build` started over ssh waits too; `north capacity gate` states the rest.

## Offload

- Expect a heavy `run` (not a landing check) that local admission would defer, for any DEFER reason including an occupied queue, to run on the first other `[hosts.NAME]` with `offload_builds = true`, an `ssh` destination and room (6 free cores less 6 for each earlier build already on its way, memory and 20 GiB of disk, read over ssh within 5 s), reported as `{"decision":"OFFLOADED","reason":"OFFLOAD","host":...}`: the lane's commit, its commits beyond origin/main and its uncommitted diff (untracked files included) rebuild in a per-lane checkout under `~/.cache/build-offload` there, so target directories persist per lane, and the same command runs under that host's own heavy admission with its output and exit code returned to the caller. A command that names the lane's or the main checkout's absolute laptop path stays on this host (`OFFLOAD_DECLINED`, `refused: absolute path`), because the host keeps its own trees at those paths; run it with paths relative to the lane.
- A request with an `AGENT_CAPACITY_NEED` (GPU, native, Warcraft client, private asset), a private-asset file (w3x, mpq, blp and the like) or a diff over 16 MiB in the lane, an unreachable or full host, or a failed sync runs the local path unchanged, and a build already offloaded never offloads again; each such decline, when an offload host is configured, prints one `{"decision":"OFFLOAD_DECLINED","reason":...,"detail":...}` stderr line per change of reason, `no room`, `unreachable` (every host), `refused: CLASS` (the need, `landing`, `already offloaded`, or the lane's private asset or size) or `failed` (a sync or local git fault).

## Landing checks

- A batch land check that safe-push runs under its landing lock (owner land-check, SAFE_PUSH_LAND_HEAD set) is admitted as critical at its own size, so the lock holder never waits on batch load.
- While the local host is gaming-held, `safe-push --to main` offloads the land checks: `north capacity offload choose` names the first other `[hosts.NAME]` with an `ssh` destination whose cores (6 free), memory (the heavy request) and disk (20 GiB free where its offload slot lives) have room, and `landing.offloaded-capacity` (moderate class, not gaming-deferred) replaces `landing.capacity` for the light part of the landing. Without such a host, or with a `land.local-only` check, the landing runs here under game reserve and otherwise waits here as before.

## Desktop and profiles

- Attended batch stays within cores minus four and 20% RAM available, and defers while protected-slice CPU some avg10 is at least 10%, because the desktop must stay responsive. Unattended stays within all cores and an 8 GiB available-memory floor, never above a quarter of RAM on small hosts.
- Cap leased memory at 75% RAM in both profiles, so the kernel never has to reclaim from interactive work.
- Weight Tom's desktop slice above agent work while attended: every profile change and every admitted run sets app.slice and session.slice (the compositor) to CPUWeight and IOWeight 1000 (100 and 300 unattended) and agent.slice to 20 (`slice_weight_app_attended`, `slice_weight_app_unattended`, `slice_weight_session_unattended`, `slice_weight_agent`), a 50 to 1 share under contention, because unleased bursts (gitleaks at 400%, a pre-push bun at 200%, rustc at 300%) held CPU pressure at 17 to 18% for 5 minutes and slowed his browser while a stale runtime drop-in of 2026-10-29 had left both slices at 100; hosts with a fixture subslice leave the real slices alone.
- Respect the spawn gate when committed batch CPUs reach the aggregate limit or protected CPU pressure exceeds 20; queue, use the farm or use cloud workers, because adding local workers then slows everything.
