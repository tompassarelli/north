# Run, park and land

Read this when you wait on a long run, a child's report is late, a Budget line is reached, you land or own lanes, staff native workers or get a `north flow` alert.

## Throughput

- Batch ready lanes into one landing, run independent unknown-cause hypotheses in parallel (a `Done: ruled out` with evidence closes one), render 2–4 variants and judge them once, and send code-only work to cloud workers first.
- Key every generated record, digest and gate only to the inputs it checks, so landing unrelated work never invalidates another lane.
- Hand Tom the artifact he is waiting for the moment it is usable, and run checks that cannot change it afterwards; only release and landing gates block, and a native capture is never one (see warcraft-modding).

## Waits and parking

- Expect a report at 45 minutes or twice the ETA, whichever is first.
- Count working sessions, not resident ones, against a host slot: an idle session or one parked on a registered run takes none, and dispatch fills each host to its target of working sessions. A worker (and a lead) that starts a farm run, an Autoland landing, a capture batch or a background job longer than about 5 minutes registers it with `north fleet wait job:NAME|farm:ID|branch:BRANCH|capture:PATH` and ends its turn; the session parks, keeps its claims, and the fleet tick revives it with the run and its result. An idle session with no registered wait parks at the 10-minute idle limit and revives on its next message; a box worker that ended its turn on prose with no registered wait is woken by the tick after a minute, at most three times, and then its lead is told.
- A brief's Budget line is a pacing signal, not a bill: read your API-equivalent spend with `north fleet spend --self`, and at the line commit and land any change already passing its checks, then report `Split:` with the remaining parts as boxes; never stop with passing work unlanded, because each stop at the line on 31 Oct left finished work for a restaff to re-read.
- Run at most as many native workers as free client slots (machine-capacity allows 4 Warcraft clients, 2 per pair), and give each a box that fits 45 minutes, because on 19 Oct three native workers queued on slots and five hit leash or context limits mid-run.
- On a stall or saturation alert from `north flow`, which repeats every 15 minutes while it lasts, diagnose and act within 15 minutes (stop launching onto the host, move work to a free host, clear the blocker) and report only the result to Tom, because a pipeline left alone cost 2 hours of landings on 2026-10-29.

## Landing

- Land as [notes](notes.md#landing-mechanics) describes for Autoland and other repositories. Lane lifecycle and `lane-gc` belong to `repo-safety`; every unlanded lane `lane-gc` keeps shows in its lead's tick as an `OWNED` row, which the lead continues, lands or archives, and a lead holding more than 25, or any older than 72 h, gets a supply-alert.
- File an unproven process idea as an issue and promote it to policy only after a measured result.

## Reports

- The runtime records delivery and completion, so keep no ACK lines or re-pushes.
- A worker that finds independent parts, or projects past twice its ETA, reports `Split:` with the parts as boxes, each with a Done when and what it learned. Those boxes go on its own issue, never into new issues, because the open-issue count measures open outcomes (Tom, 2026-10-29). Fix a finding that fits in 20 minutes in the current lane and land it with the task instead of filing it, and put ideas under `Ideas:` in the final report (Tom, 2026-10-30). The `gh issue create` guard states what a worker may file.
