# Diagnosing pressure and long waits

What a DEFER means, where a build went, and who holds the machine. Exact thresholds are in [admission details](admission-details.md).

## Read the state

- For diagnosis read `status` (holders, queue order, measured versus reserved, `unleasedHeavy`), the watchdog's `incidents.md` and `capacity-watchdog report`, and the usage log; the paths and fields are in [admission details](admission-details.md).
- Treat memory PSI as diagnostic and system CPU PSI as admission pacing rather than desktop harm, because batch quota throttling inflates both without hurting the desktop.
- Assign exact-scope cleanup to the owner or accountable parent, and leave signaling of peers to them, because only they know what is safe to stop.

## Why heavy work waits

- Heavy admission waits on CPU pressure against a target band, not on a job count, and a DEFER names its reason; the thresholds are in [admission details](admission-details.md#queue-and-spawn-gate).
- Builds on a `lease_builds` host wait for a lease through the `cargo`, `bun` and `nix` shims and `depbox`, so `nix develop --command cargo build` started over ssh waits too; `north capacity gate` states the rest.

## Offload

- Expect a heavy `run` (not a landing check) that local admission would defer, for any DEFER reason including an occupied queue, to run on the first other `[hosts.NAME]` with `offload_builds = true`, an `ssh` destination and room (6 free cores less 6 for each earlier build already on its way, memory and 20 GiB of disk, read over ssh within 5 s), reported as `{"decision":"OFFLOADED","reason":"OFFLOAD","host":...}`: the lane's commit, its commits beyond origin/main and its uncommitted diff (untracked files included) rebuild in a per-lane checkout under `~/.cache/build-offload` there, so target directories persist per lane, and the same command runs under that host's own heavy admission with its output and exit code returned to the caller. A command that names the lane's or the main checkout's absolute laptop path stays on this host (`OFFLOAD_DECLINED`, `refused: absolute path`), because the host keeps its own trees at those paths; run it with paths relative to the lane.
- A request with an `AGENT_CAPACITY_NEED` (GPU, native, Warcraft client, private asset), a private-asset file (w3x, mpq, blp and the like) or a diff over 16 MiB in the lane, an unreachable or full host, or a failed sync runs the local path unchanged, and a build already offloaded never offloads again; each such decline, when an offload host is configured, prints one `{"decision":"OFFLOAD_DECLINED","reason":...,"detail":...}` stderr line per change of reason, `no room`, `unreachable` (every host), `refused: CLASS` (the need, `landing`, `already offloaded`, or the lane's private asset or size) or `failed` (a sync or local git fault).

## Landing checks

- A batch land check that safe-push runs under its landing lock (owner land-check, wherever it runs: the owner alone marks it, since the lander's wrapped checks and a check shipped to another host do not carry SAFE_PUSH_LAND_HEAD) is admitted as critical; its scope may burst to all cores minus the attended desktop reserve, never fewer than a heavy build gets (16 CPUs on a 20-core build host, 20 on the 24-thread laptop), but it is charged only the land-check cores (`land_check_cpus`: the learned peak of recent land checks, else `land_offload_cpus`), so two or three checks share a host instead of one filling it, in the CPU-quota scope of `agent-capacity.slice` at normal weight with a 16 GiB memory ceiling while it is still charged its measured memory, so the lock holder never waits on batch load and its test step is not held to the 2 CPUs of the moderate class it asks for; running builds keep their scopes (north#360).
- Expect `north capacity offload run` to run the command inside the checkout's `nix develop` shell on the host whenever the checkout has a flake.nix (not again when the command already starts with `nix develop`), so pass the bare command (`cargo test`, not `nix develop -c cargo test`) and never probe where cargo lives on the host (2026-11-04).
- While the local host is gaming-held and its CPU split is not enforced, `safe-push --to main` offloads the land checks (an enforced split keeps them here, confined to the agent CPUs): `north capacity offload choose` names the first other `[hosts.NAME]` with an `ssh` destination whose cores (6 free), memory (the heavy request) and disk (20 GiB free where its offload slot lives) have room, and `landing.offloaded-capacity` (moderate class, not gaming-deferred) replaces `landing.capacity` for the light part of the landing. Without such a host, or with a `land.local-only` check, the landing runs here under game reserve and otherwise waits here as before.

## Desktop and profiles

- Weight Tom's desktop slice above agent work while attended: every profile change and every admitted run sets app.slice and session.slice (the compositor) to CPUWeight and IOWeight 1000 (100 and 300 unattended) and agent.slice to 20 (`slice_weight_app_attended`, `slice_weight_app_unattended`, `slice_weight_session_unattended`, `slice_weight_agent`), a 50 to 1 share under contention, because unleased bursts (betterleaks at 400%, a pre-push bun at 200%, rustc at 300%) held CPU pressure at 17 to 18% for 5 minutes and slowed his browser while a stale runtime drop-in of 2026-10-29 had left both slices at 100; hosts with a fixture subslice leave the real slices alone.
