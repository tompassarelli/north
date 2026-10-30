---
name: machine-capacity
description: >-
  Bound sustained multi-core or >1 GiB local work and diagnose agent-caused resource pressure. Skip ordinary edits and small checks.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Machine capacity

## Admission

- Resolve the helper at `$(dirname "$(agents path machine-capacity)")/scripts/machine-capacity.mjs` and invoke it with Bun, because every caller must reach the one shared admission state.
- Admit sustained multi-core or >1 GiB work through the helper, never through worker slots, because idle slots measure neither CPU nor memory and several reasonable commands together exhaust the desktop.
- Expect heavy and exclusive admissions to defer with DEFER_RUN_QUEUE, on a live host, while CPU pressure (PSI some avg10) is at or above `run_queue_psi_attended` (40%, attended) or `run_queue_psi_unattended` (60%), and load average is not read at all (threads that touched the run queue, such as rsync streams and Bun workers, raise it without saturating the CPU: 2026-10-31, load 11.5 on 24 cores at PSI 6.9% deferred six builds for 20 minutes); the batch CPU-pressure gate (DEFER_CPU_PRESSURE) already holds heavy work from 30%, so those limits decide exclusive work and the reason heavy work shows; for facts stated without a pressure reading (fixtures, stdin) it defers while the 1-minute load average once system CPU some avg10 is under 1%, else the higher of the 1- and 5-minute load averages, exceeds the thread count (exclusive) or, for heavy, while it plus the request's CPUs, capped at half the thread count, does, Warcraft clients included, naming as `offload` the first `[hosts.NAME]` that serves the box's `AGENT_CAPACITY_NEED`, and tickets held by it hold no place for moderate work; heavy builds therefore pause on a host while its clients and sessions already fill it (Tom's direction of 2026-10-30 that the queue waits rather than oversubscribe); load average is only that stated-facts fallback (a live host that cannot read pressure fails admission), because load nothing leased accounts for (game clients, perf runs, unadmitted builds) once let admission keep starting heavy steps at 34.7 on 24 threads (north#156). Critical, tom, runtime, native, agent and moderate ignore it ([admission details](references/admission-details.md)).
- Run `machine-capacity estimate -- CMD` before staffing local work, because leases size from it by default.
- Never set CPUQuota, CPUWeight or AllowedCPUs outside machine-capacity, because uncounted quotas defeat shared admission; `heavy-command-guard` refuses it.
- Run cargo build, full `bun test` suites, `bun wisp map build` and headless wisp renders (`view`, `headless --render`) through `run`, because `heavy-command-guard` refuses them unwrapped. It routes cargo test, tsc --build and multi-file bun test through admission automatically; cargo check and single test files remain local.
- Builds on a `lease_builds` host wait for a lease through the `cargo` and `bun` shims and `depbox`; `north capacity gate` states the rest.
- Never start wine, proton, steam-run or a game .exe outside a native session, because unadmitted game clients starve the desktop; `native-launch-guard` refuses it.
- Use `run --class CLASS --owner OWNER --timeout-seconds N -- COMMAND ARG...` for batch work, with the timeout covering setup and download time, because every run needs a finite bound.
- Use `session --class native --new-clients N --owner OWNER -- COMMAND ARG...` for Warcraft clients and private desktops (one scope per client, two per pair), because they must outlive a batch deadline.
- Pass `--wait-seconds S` on a native `run` or `session` that starts clients to wait for admission in arrival order for up to S seconds, then exit 75 with the last DEFER, because a caller retrying a DEFER on a timer loses the freed client slot to whoever asks during its gap (wisp#76).
- Start native evidence runs (accept, gate runs, capture batches, native soaks, pad scripts) with `session --class native --new-clients 0 --quiet --quiet-seconds N` for the run's duration only, because a declared length is what makes the lease hold heavy tests and builds, which pass a quiet lease that declared none (north#335), and because I/O, copies and memory pressure from batch work stall both clients even at `nice -n 19`; moderate, heavy, exclusive and gpu work then queues until the run ends and is refused when its own timeout runs out first. Never mark idle pair pools, clones or private desktops quiet, because they are live most of the day while nothing is measured. Pass `--beside-native NATIVE-OWNER` only for that evidence run's own build steps.

- Treat Tom as never gaming unless he says so in words ("I'm gaming, quiet the machine"), and run `north capacity gaming on` then and `north capacity gaming off` when he says he is done (`status` reads it), because no process, GPU load, logind idle hint or other signal is read to infer it; the declared hold has no expiry and stays until `off`. While it is on, native, heavy, exclusive and gpu admissions defer with DEFER_GAMING and each 30-second laptop-auto tick stops every native lease, agent Warcraft client and private desktop and freezes heavy, exclusive and gpu scopes, because agent GPU and memory load beside his game hard-froze the laptop; the same tick stops the scope of any run lease whose wrapper died, so no client outlives its runner, and thaws the frozen scopes once the hold is off.
- Expect `north capacity game-reserve on` to admit heavy and moderate batch on the agent CPUs it pins (`GAME_RESERVE_AGENT_CPUS`, the batch CPU limit while it is on) and to leave heavy scopes unfrozen, because the reserve already confines agent.slice away from the game's cores; native, exclusive and gpu still defer with DEFER_GAMING, native scopes are still stopped and exclusive and gpu scopes frozen, and any non-agent request that would leave less than `game_reserve_headroom_mib` (24 GiB) of measured available memory defers with DEFER_GAME_MEMORY, so the game and desktop keep their memory.
- While the local host is gaming-held, `safe-push --to main` offloads the land checks: `north capacity offload choose` names the first other `[hosts.NAME]` with an `ssh` destination whose cores (6 free), memory (the heavy request) and disk (20 GiB free where its offload slot lives) have room, and `landing.offloaded-capacity` (moderate class, not gaming-deferred) replaces `landing.capacity` for the light part of the landing. Without such a host, or with a `land.local-only` check, the landing runs here under game reserve and otherwise waits here as before.
- Declare `--new-clients N` on a native `run`, `session` or `probe`: 1 for a clone or one client (the default), 2 for a pair start, 0 for a run on clients already up, because admission defers only when live Warcraft clients plus N exceed the host's client cap: its measured `warcraft_clients` (else four when attended) as a ceiling, lowered to the live clients plus those free BAR1 (`bar1_client_mib` each after `bar1_margin_mib`) and free VRAM (`vram_client_mib` each after `vram_margin_mib`) from the same `nvidia-smi -q -d MEMORY` still fit (north#259).
- Declare `--new-desktops N` beside it for the private desktops the request starts (2 for a pair whose clients each get a desktop), because admission defers while live BAR1 Free is below the declared desktops and clients at their measured MiB plus a margin.

## Classes

Choose the smallest sufficient class, because oversized claims queue peers for nothing.

- moderate = 2 CPUs/2 GiB; heavy = 6 CPUs/8 GiB; exclusive = all allowed cores and no peer batch; gpu = 1 CPU/2 GiB for headless Chrome and wisp renders (not grim captures), two at a time; native = 2 CPUs/4 GiB/no CPU quota.
- critical (all cores minus the attended desktop reserve, 16 GiB, one-hour batch limits, front of the queue, no CPU-capacity or pressure wait) only for the landing-train holder or a run with an explicit `--critical` release-blocker diagnostic, because it jumps the queue and ignores pressure.
- tom (critical sizing: all cores minus the attended desktop reserve, 16 GiB; front of the queue with critical, no CPU-capacity or pressure wait) only for the build Tom is waiting to play (the Tom-build watcher and lane builds), because it ignores pressure and a 2-CPU grant left his cold wisp-window build at 121 of 348 crates after 5 minutes on 24 threads (2026-10-31).
- runtime (4 CPUs, 8 GiB, front of the queue with critical, no run-queue, CPU-capacity or pressure wait, still held by the build disk floor) only for `north runtime update` building a landed north runtime, because every landing waits on that one build and heavy work once queued it for hours (north#308).
- A batch land check that safe-push runs under its landing lock (owner land-check, SAFE_PUSH_LAND_HEAD set) is admitted as critical at its own size, so the lock holder never waits on batch load.
- Omit `--class` on `run` to size from the usage log (p90 of the last 20 runs plus 25%; moderate until three runs exist), because measured sizing beats guessing; the charging rules are in [admission details](references/admission-details.md).
- Request native-only `--memory-gib 1.5` for measured smaller offline clients, because it charges 1536 MiB instead of 4096 MiB for that client only.
- Respect batch `session`'s 30-minute default, batch's one-hour maximum and exclusive's 15-minute maximum, because only native sessions have no default deadline.

## Leases and supervision

- Reserve an agent lease before compute-using workers, renew before expiry and release at settlement, because the lease is the only record that frees the allowance; it charges 768 MiB without reserving local CPU.
- Give worker local commands their own run scopes and persist reservations only for the agent class, because larger classes cannot outlive their wrapper.
- Keep native sessions foreground until command exit, Ctrl-C or explicit stop, because a detached tree escapes its scope.
- Keep every descendant inside its scope and supervise the wrapper through terminal `RELEASED`; never detach work or renewal processes, because a forcibly killed wrapper cannot clean up and its allowance stays charged until its scope ends.
- Keep queued wrappers supervised until they start automatically, because a dropped wrapper drops its queue ticket.
- Retry a DEFER after a known release or 30 seconds and never busy-poll, because polling adds the load the queue exists to avoid.
- Treat RUN/RESERVED as permission to continue, NESTED as a run admitted under the held batch lease it was started from (no lease, scope or queue ticket of its own, bounded by its own timeout), and RECLAIMED as expired leases or finished helper scopes, never as permission to kill peers, because peers belong to their owners.
- Retain allowances while a wrapper or scope is live and within its deadline, including native sessions, because a live scope is real load; a run lease past its deadline has its scope killed and is reclaimed.
- Assign exact-scope cleanup to the owner or accountable parent, and leave signaling of peers to them, because only they know what is safe to stop.
- Respect the spawn gate when committed batch CPUs reach the aggregate limit or protected CPU pressure exceeds 20; queue, use the farm or use cloud workers, because adding local workers then slows everything (thresholds in [admission details](references/admission-details.md)).
- Expect a heavy run to be charged at least its declared 6 CPUs, so at most three heavy builds share the 20-CPU line.
- Never lower correctness requirements because of pressure, because slower correct work beats fast wrong work.

## Profiles and pressure

- Attended batch stays within cores minus four and 20% RAM available, and defers while protected-slice CPU some avg10 is at least 10%, because the desktop must stay responsive. Unattended stays within all cores and an 8 GiB available-memory floor, never above a quarter of RAM on small hosts.
- Cap leased memory at 75% RAM in both profiles, so the kernel never has to reclaim from interactive work.
- Treat memory PSI as diagnostic and system CPU PSI as admission pacing rather than desktop harm, because batch quota throttling inflates both without hurting the desktop.
- Scale native pairs one at a time and stop adding above protectedCpuSomeAvg10 20, because Warcraft clients are the heaviest steady load.
- For diagnosis read `status` (holders, queue order, measured versus reserved, `unleasedHeavy`), the watchdog's `incidents.md` and `capacity-watchdog report`, and the usage log; the paths and fields are in [admission details](references/admission-details.md).

Read [leases and limits](references/leases-and-limits.md) for reserve/renew/release commands, admission rationale and session recovery.
Read [admission details](references/admission-details.md) for thresholds, native GPU deferrals, mode switching and queue behavior.
Read [testing on this machine](references/testing-on-this-machine.md) for the local test runners, timing checks and review paths the verification skill leaves unnamed.
