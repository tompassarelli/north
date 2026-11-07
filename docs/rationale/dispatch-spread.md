# Dispatch spread: one run bounded by pace, sessions, builds and disk on every host

At 12:05 PM on 2026-11-06 (laptop time) one `north dispatch` run launched 17 boxes, all on gex45,
several times what its pace bound allowed. Within 15 minutes 9 to 12 builds waited in gex45's capacity admission,
which ran 4 at a time, its root disk was 99% full, and the laptop sat at load 8 (north#259). Three
gaps let it happen, each closed in `crates/north-dispatch/src/dispatch`:

- **The account bound read only held accounts.** d71b6206a bounded launches by `allowed_workers` less the working
  sessions only when pace-control marked the account `held`; the account was not `held`, so
  it was unbounded. `survey::pace_workers` now reads `allowed_workers` for every account and None only when pace names
  none.
- **Free room was decided once, before the queue formed.** Sessions against `resident_cap` were already taken per
  launch; builds were not counted at all. Each host now carries `builds`, the builds its admission queue takes before
  it reaches `QUEUED_LIMIT` (the queue depth at which placement skips a host), read from the same backlog probe
  placement uses. Each launch in the run takes one, since a fresh worker's first act is a build, so `Pools::host` is
  the smaller of free sessions and free builds after every earlier launch in the run.
- **Disk was refused only under an absolute floor.** `Saturation::refusal` refused a root disk with less than
  `build_disk_free_gib` free, which a 936 GiB array at 99% still clears. The probe now also reads the disk's size
  from `df`, and a root disk past the lease reaper's eviction line (`capacity::disk_line::Line::eviction`,
  `offload_evict_free_gib` free or 20% of a smaller disk) refuses new sessions in dispatch and in
  `north fleet launch` placement alike. A fixed 85%-used line refused the 936 GiB array at 123 GiB free while the
  reaper, at 94 GiB, slept (north#634); one line means a refused host is always one the reaper is reclaiming.

`north dispatch --dry-run` prints each host's line with its refusal or the builds its queue still takes, and a
`skip ITEM: ready, not placed: ...` line naming the bound that stopped each box: the account's `allowed_workers`, the
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
