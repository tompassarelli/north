# Repeat runs, 2026-11-06: why a box takes more than two worker runs

Tom asked on 2026-11-06 that north's own staffing and oversight be fixed rather than capacity added, after the worker ledger showed 6 to 30 mean attempts per box and a 9% to 40% first-pass rate over the week from 2026-10-30. Owner: north#636.

## What the ledger counted

`north worker-ledger --summary` reads `attempts` and `ok` from `crates/north-threads/src/threads/calibration/read.rs`. Runs were chained by the ledger's `item` column, which holds the issue alone (`north#360` held 110 runs that week), and a run staffed within 24 hours of an earlier run's end on the same issue was the next attempt. `attempts` is the mean of each run's position in its chain, so one long chain weighs as many times as it has runs. `ok` is `done` with no later run in the chain. So the next box on the same issue counted as a repeat of the box before it, and it also marked the earlier run not ok.

## Method

Read-only copy of `~/.local/state/threads/snapshot.db` (newest run at about 05:10Z on 2026-11-06). The window holds 1,280 settled worker runs that name an issue, and the issue-level chain counts 848 of them as repeats. A run's box comes from its brief: the `Box:` line, else the `Item:` line when it names more than the issue. The ledger's `brief_text` gave the box for 398 runs, and the first user message of 204 more local transcripts gave more. Once bare `Item: north#N` lines are dropped, 471 runs have a known box, and 357 of them are issue-level repeats. Each same-box repeat was classified by hand from the previous run's report, and matched by item and start time to `~/.local/state/agents/dispatch/launches.jsonl` to tell dispatch launches from hand launches.

## Causes, by count (of 357 issue-level repeats with a known box)

1. **The next box counted as a repeat: 317.** 302 runs worked a different box of the same issue. 15 more were a fan-out under one shared `Item:` line: the test-cull slices of north#635, review rounds, and the safe-push phases of north#621. This is a counting error. Fixed: chains are keyed by issue and box, so only the same box run again is a re-attempt, and a run on another box leaves the earlier run kept (`calibration::boxes_tests::a_run_on_another_box_of_the_same_issue_is_no_reattempt_and_leaves_the_earlier_run_kept`). A run whose brief is unread still chains per issue.
2. **Placed on a host that could not do the box: 20.** Most ran on gex45 while it could not decrypt the GitHub App key (north#624, since fixed at its cause), or needed the laptop that gex45 cannot reach (north#636's window-move box, north#631, wisp#230). Of the 15 same-box repeats with a launch record, 11 came from dispatch, 10 of them placed on gex45. The worker's report named the host in prose, which dispatch never read. Fixed: a report ending `Stop: host NAME` makes the next dispatched brief carry `Needs: NAME`, so placement refuses the host that lacks it.
3. **Relaunched while the box waited on a clock, an owner or an outside party: 9.** Examples: north#647 was relaunched 13 minutes after its report said its last box needs the 24 hours ending at 6:07 AM, and north#589 and north#255 likewise. Only `Stop: decision` and `Blocked:` held a box. Fixed: a report ending `Stop: wait until YYYY-MM-DDTHH:MMZ` holds the box until that UTC time, and a later comment does not lift it (`dispatch::stops_tests::a_last_report_holds_its_box_until_the_issue_changes_after_a_decision_until_its_time_after_a_wait_and_off_the_host_after_a_host_stop`).

The other true repeats: 9 continued partial progress on a box too large for one run, and 2 were blocked at landing. None was lost to an edit or build that a pre-edit check could have refused, so a linter gate in the SWE-agent style would not have saved any of these runs.

A keyword sort of 436 Not done or unclear reports by a sibling session of north#636 agrees on the order: host or host access 134, ended with a run or landing still pending 79, waiting on Tom 37, capacity or usage limit 35, other 130.

## Before and after on the same week

`THREADS_DB=<copy> north threads summary --group category --since 2026-10-30` on the same copy, the installed north against this change, for every cell whose mean attempt position was 8 or more:

- bug-known-cause on medium, arm -: attempts 28.9 to 23.5, first-pass ok 26% to 26%
- bug-known-cause on medium, arm worker: attempts 8.6 to 3.7, first-pass ok 28% to 48%
- bug-known-cause on sonnet, arm sonnet-high: attempts 10.6 to 2.9, first-pass ok 25% to 41%
- bug-known-cause on xhigh, arm -: attempts 82.5 to 67.5, first-pass ok 0% to 0%
- debugging-unknown-cause on medium, arm -: attempts 9.8 to 3.8, first-pass ok 52% to 59%
- docs-policy on sonnet, arm -: attempts 14.2 to 7.0, first-pass ok 9% to 9%
- feature on medium, arm -: attempts 16.1 to 11.0, first-pass ok 17% to 17%
- feature on medium, arm worker: attempts 14.5 to 10.0, first-pass ok 12% to 12%
- feature on sonnet, arm handoff-150k: attempts 9.3 to 6.5, first-pass ok 19% to 19%
- feature on sonnet, arm sonnet-high: attempts 17.9 to 12.9, first-pass ok 9% to 12%
- mechanical on haiku, arm -: attempts 26.2 to 7.7, first-pass ok 33% to 41%
- mechanical on sonnet, arm -: attempts 30.8 to 22.6, first-pass ok 11% to 11%
- performance on medium, arm -: attempts 15.1 to 7.4, first-pass ok 17% to 25%

The cells still high are mostly runs whose brief the ledger never stored, so they still chain per issue: 916 of the week's 1,344 worker rows on an issue have no `brief_text`. Recording the brief, or at least its box, on every fleet run is what moves the rest.
