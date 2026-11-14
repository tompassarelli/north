# Dispatch spread: one run bounded by spend, sessions, builds and disk on every host

At 12:05 PM on 2026-11-06 (laptop time) one `north fleet dispatch` run launched 17 boxes, all on gex45,
several times what the account bound allowed. Within 15 minutes 9 to 12 builds waited in gex45's capacity admission,
which ran 4 at a time, its root disk was 99% full, and the laptop sat at load 8 (north#259). Three
gaps let it happen, each closed in `crates/north-dispatch/src/dispatch`:

- **The account bound read only held accounts.** The bound on one account's launches in a run was applied to held accounts alone, so a
  non-held account was unbounded. The spend rule (`docs/rationale/spend-rule.md`) now admits each launch on every account and recounts
  after every launch in the run.
- **Free room was decided once, before the queue formed.** Sessions against `resident_cap` were already taken per
  launch; builds were not counted at all. Each host now carries `builds`, the builds its admission queue takes before
  it reaches `QUEUED_LIMIT` (the queue depth at which placement skips a host), read from the same backlog probe
  placement uses. Each launch in the run takes one, since a fresh worker's first act is a build, so `Pools::host` is
  the smaller of free sessions and free builds after every earlier launch in the run.
- **Disk was refused only under an absolute floor.** `Saturation::refusal` refused a root disk with less than
  `build_disk_free_gib` free, which a 936 GiB array at 99% still clears. The probe now also reads the disk's size
  from `df`, and a root disk past the lease reaper's eviction line (`platform::disk_line::Line::eviction`,
  `offload_evict_free_gib` free or 20% of a smaller disk) refuses new sessions in dispatch and in
  `north fleet launch` placement alike. A fixed 85%-used line refused the 936 GiB array at 123 GiB free while the
  reaper, at 94 GiB, slept (north#634); one line means a refused host is always one the reaper is reclaiming.
- **Disk was read once per round and never charged.** On 2026-11-11 gex45 fell to 3% free because one round placed
  workers against a disk reading taken before the round, each worker cold-building 4 to 8 GiB in its lane. A host's disk
  bound is `[hosts.NAME] disk_free_gib`, else the eviction line; under it the host takes nothing and the refusal names the
  host, its free GiB and the bound, and above it the host's room is the smaller of its memory room and the free space
  over the bound divided by `dispatch_cold_build_gib` (8), so each placement in a round takes a cold build from the disk
  and the host takes work again in the first round after free space is back over the bound. On 2026-11-12 a 150 GiB
  host bound sat over the reaper's lines, so a host at 149 GiB free stayed closed with nothing reclaiming it and the freeze
  alarm named an unrelated guard; the reaper now reads the same bound as the line it evicts under, and the alarm names each
  closed host's refusal (north#1232).
- **A host whose GitHub token had gone stale was still given boxes.** A host without the App key trusts the token it was
  handed for 55 minutes. Two workers launched on such a host after that trust lapsed stopped with nothing done because
  they could not read their issue (north#818). The saturation probe now also prints the age of that token (empty on a
  host that mints its own), and `Saturation::refusal` refuses a host whose token is at or past
  `github_app::PUSHED_FRESH_SECONDS`, naming the host and the age, in dispatch and in `north fleet launch`
  placement alike.

`north fleet dispatch --dry-run` prints each host's line with its refusal or the builds its queue still takes, and a
`skip ITEM: ready, not placed: ...` line naming the bound that stopped each box: the spend rule's refusal for the account, the
host's free slots, its admission queue, or its saturation reading.

## Sizing resident_cap by build slots

A host's sessions should never outnumber what its build slots serve. If every session on a host starts a build at
once, admission runs as many as its slots allow and queues the rest; once the queue reaches `QUEUED_LIMIT` (4)
placement stops sending sessions there, but the sessions already there keep waiting. So `resident_cap` is the build
slots plus `QUEUED_LIMIT`: the worst case fills the queue exactly to the depth placement refuses at, never past it.

Slots are the aggregate CPU limit over the cores a heavy build lease takes, read from `north capacity host status`
on 2026-11-06:

- gex45: limit 20, heavy leases of 4 to 6 cores, so 4 slots and `resident_cap` 8 (was 24, sized for memory at about
  400 MB a CLI, which memory never bound).
- ccx33: limit 8, heavy leases of 3 cores, so 2 slots and `resident_cap` 6 (was 8, one CLI per core).
- whiterabbit: limit 22 outside a playtest lease (2026-11-07: agent.slice holds 22 of 24 threads and that ceiling replaces the attended reserve), heavy leases of about 5 cores, so 4 slots; under a playtest lease the limit drops to 12. The laptop departs from slots plus `QUEUED_LIMIT`: its `resident_cap` is 12 (was 7 at limit 14) because its sessions spend most of their time in model turns, not builds, and builds already queue in its admission, so the session cap need not equal build slots; memory still does not bind, twelve sessions of about 400 MB a CLI plus a heavy lease of up to 5 GB each come to about 65 GB of its 94 GB (2026-11-07).

Nexus keeps 6; it takes no dispatch boxes until it becomes a target. Interactive sessions count against the same cap
through `north resident-cap gate`, so a lead on gex45 shares those 8 places with the workers it staffs.

## A fleet session's build goes to the build hosts first

The rule before north#1036 was local first: a host that leases its own builds, which the laptop is in every capacity
mode but a playtest lease, decided on its sessions' compiles itself and offered one to a build host only when its own
admission deferred it. The laptop admits up to 22 of its 24 threads, so it saturated while the build hosts idled.

One rule replaces it (`build_offload::offers_first`): a fleet session's heavy or moderate compile on a host that is not
itself a build host (`[hosts.NAME] offload_builds` unset) is offered to the build hosts it reaches before its own
admission decides, and goes to the one with the most free cores (`build_offload::most_free`). The host builds it
itself only when every build host is short of room or out of reach, and its queued run offers itself again every 15 s.
A build host decides on its own sessions' builds first, and a build outside a fleet session never leaves
the host it started on.

Every entry a session's compile takes meets that rule at the capacity gate: `cargo`, `bun` and `nix` through their
shims, and `north capacity dev-shell run -- COMMAND` by handing COMMAND to the gate before it enters a dev shell, since
inside one the shell's own toolchain comes before the shims on PATH and the compile would run here unleased. The gate's
lease enters the dev shell itself, so a run the gate leases nothing for, or one already in its lease, enters it directly.
