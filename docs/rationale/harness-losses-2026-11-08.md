# First tries lost to the harness, 24 h to 2026-11-08 03:15 UTC

The readiness bar "first tries lost to the harness" (ceiling 0.1%, `ready-harness-loss` in goals.toml) read 6% over 24 h on 2026-11-08 at 03:08 UTC, with n=323. That is 20 of 327 north first tries in the ledger snapshot this list was read from. This note lists each of the 20, the class the bar charged it to, what actually ended it and the evidence, then ranks the classes by what ended them (north#903). The bar's class comes from `docs/rationale/attribution.md`. The real cause comes from the session's archive (`~/.local/state/north/fleet/.archive/NAME-*/retired.json`), its boundary rows (`boundary_causes`), its `runs.box` and its report.

Reproduce the list with `north ledger scoreboard losses --since 24h` on the ledger host. It prints the same rows from the current ledger, each with the boundary record or report line that decided it. Every time below is UTC.

## The 20 losses

Each line gives the end time, the session and issue, the bar's class, then what ended the session.

- 11-07 04:12 north-725-d0411 north#725, placement: the box needed the laptop row of a per-host table, and dispatch placed it on gex45 (`Stop: host laptop`). It had no relaunch within 24 h.
- 11-07 04:14 gex45-n757-leases north#757, capacity: it reported "Clippy and the lease_lifecycle test run are queued in admission, and their end will notify me ... Stop: capacity". The reaper then retired it as "its parent took its final report" while those builds were still queued.
- 11-07 04:19 north-744-d0351 north#744, halted: it was parked on a usage limit and moved, then a fleet stop ended the turn (boundary rows at 03:53, 04:20, 04:23 and 04:24).
- 11-07 04:26 north-766-d0421 north#766, capacity: its first fleet test run was killed after 20 min queued in admission. It restarted the run, ended its turn waiting on it on claude-api, and never ran again.
- 11-07 06:03 n-land-slice north#784, capacity: it said the proxy had parked the issue on usage. The reaper retired it as "its parent took its final report" with its tests and clippy not yet run.
- 11-07 06:25 north-786-d0611 north#786, capacity: its new tests had not run when it ended on `Stop: capacity` on claude-api, and it never ran again.
- 11-07 08:03 n730-arms north#730, halted: it was parked on a usage limit and moved twice, then a launch took over its name mid-turn.
- 11-07 08:04 north-796-d0801 north#796, capacity: the box's readings exist only on whiterabbit, and the session ran on gex45, which cannot resolve whiterabbit.
- 11-07 16:02 north-815-d1556 north#815, access: origin/main had moved and its lane conflicted, so safe-push queued nothing. The access class came from a phrase rule; nothing failed to authenticate.
- 11-07 18:29 north-637-d1828 north#637, placement: its one open box checks a full day with no tick start timeout. A missed dated check had just reopened it, and the next day without timeouts could be read on 11-09 at the earliest.
- 11-07 21:04 bd-719-restarts north#719, halted: north-burndown-0530 launched it on a box already struck on the issue, then stopped it.
- 11-07 21:10 north-634-d2051 north#634, landing refusal: this second turn of a session that had already finished Done was handed the box "gex45 placement fits its build capacity". bd-634-gexplace had been launched on that same box 25 s earlier, and the box's second check still failed.
- 11-07 21:10 bd-634-gexplace north#634, halted: launched at 21:05:59 on the same box, it reported half the box met, then its lead stopped it.
- 11-07 21:14 bd-706-unread north#706, landing refusal: it held every build while origin/main did not move for its whole run (542aced08d), waiting on the lander's batch.
- 11-07 21:33 north-634-d2131 north#634, landing refusal: its claim on "Replay never holds the lander for long" was refused because bd-634-replay already held that box.
- 11-07 23:27 north-865-d2326 north#865, waits: it was given "Keep-or-revert, read 7 days after it enforces" while the enforce box above it was unticked. Dispatch recorded it as stopped until 11-16 on a box that was not due.
- 11-08 00:09 north-872-d0006 north#872, landing refusal: it was given "Enforce after review: a reviewer reads 24 h or 20 of the shadow rows" while the Shadow box above it had not landed.
- 11-08 00:46 north-872-d0036 north#872, landing refusal: it reported Done with the box ticked, and its lane ffd1e6b510 is on origin/main. The lander's message named main's head (5820023fa) instead of the lane's commit, so the ledger joined no landed sha, and the run counted as not done.
- 11-08 01:14 north-877-d0111 north#877, landing refusal: it was given "After 30 landed per arm, ..." while the assign-and-record box above it had not landed, so no row existed.
- 11-08 02:42 north-892-d0226 north#892, placement: safe-push from cpx62 exited 75 twice ("NOT queued"). The enqueue mutant-kill scan needs an offload host, and cpx62 could not resolve gex45.

Two losses after the snapshot belong to the same classes. n892-hostfault (11-08 03:03) reported "Not done: clippy is still running ... Stop: capacity", and the reaper retired it 63 s after its clippy notification woke its next turn. north-706-d0311 (cpx62b) is the same enqueue exit 75 as north-892-d0226.

## Classes by what ended them

1. A box staffed that could not be finished at launch: 8 losses.
   - Its wait counted from a box above it still unticked, or from a date not yet reached: north-865-d2326, north-872-d0006, north-877-d0111, north-637-d1828.
   - Two sessions on one box: north-634-d2131, north-634-d2051, bd-634-gexplace.
   - The box was already struck: bd-719-restarts.
   Dispatch's relative-wait reading (`issues.rs` WINDOW) knows only "after|over|within N h|d", anchored on the newest cited landing. "7 days after", "a day after", "After 30 landed" and "reads 24 h or 20 of" pass as workable.
2. The fleet ended a session that still held work which would wake it: 4 losses (gex45-n757-leases, n-land-slice, north-766-d0421, north-786-d0611), and n892-hostfault after the snapshot. The reaper's Finished verdict reads a `Not done:` report with a `Stop:` line, taken by the parent, as finished. It never looks at the background tasks the CLI still holds, and for a session on another machine it reads a mirror up to a minute old.
3. A host that cannot do what the box or its landing needs: 4 losses. north-725-d0411 and north-796-d0801 are a box needing the laptop or whiterabbit, placed on gex45. north-892-d0226 is a landing host that could not queue. bd-706-unread is a lander batch that held main for a whole run.
4. Usage-limit parks and the moves after them: north-744-d0351 and n730-arms, with n-land-slice, north-766-d0421 and north-786-d0611 on the same claude-api lane. No run has used claude-api since 2026-11-07 08:02.
5. The measurement itself: north-872-d0036 (a misnamed landing sha) and north-815-d1556 (a lane conflict read as access).

## What removes each class

- Class 1, waits: dispatch holds a box whose wait counts from an unticked box above it, or counts something with no date, and names what it waits on (north#903, worker n903-waits).
- Class 1, duplicates: a launch, or a turn that hands a running session a new box, takes the box claim before it starts, and a refused claim refuses it (north#888). The issue-level and cross-issue claims landed as 210c4f1383 and cc9f745bff86.
- Class 2: the reaper keeps any session whose CLI holds a background task without its notice, an armed Monitor or an unfired wakeup, and re-reads the host before it archives (north#903, worker n903-reaper).
- Class 3, landing hosts: a build host that reaches none of its declared hosts runs the enqueue kill scan itself instead of exiting 75 (1338880a64, north#892 class 11).
- Class 4: no worker runs on claude-api.
- Class 5: since 14a61c6877, the push ledger and the landing record name each lane's own commit, not the probe's top (north#842).
