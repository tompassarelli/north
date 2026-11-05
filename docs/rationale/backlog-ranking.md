# Backlog ranking: north's open issues ranked by leverage, with the edges between them

North's open issues on 2026-11-06, ranked by how much other work each one unblocks or speeds up, then by cost, with the waits between them as an edge list.

## How dispatch reads this order

`north dispatch` (crates/north-dispatch/src/dispatch/mod.rs, `ordered`) sorts ready issues by priority label (`priority:now`, then no label, then `priority:next`, then `priority:later`), then oldest first, then project, then number. It keeps an issue back while any issue it waits on is open. It finds those waits in `Follows:` lines, `Blocked by` lines and GitHub blocked-by relations (crates/north-dispatch/src/dispatch/issues.rs, `predecessors`). A `Needs:` line is a placement need such as `gex45` or `native`, which the brief copies so placement can read it. So an issue-to-issue wait goes in `Follows: north#N`, never in `Needs:`. Dispatch has no rank finer than the priority label. Within `priority:now` it staffs the oldest first, and since free slots outnumber ready `now` issues, every ready one is staffed in the same round. The rank below therefore decides the label, not a position in the queue.

Dispatch refuses to place a `bug` issue that has no `Class:` line (crates/north-pace/src/router/brief.rs, `class_refusal`). It also skips an issue with no `- [ ]` box. Every open bug therefore carries a `Class:` line, except the bodies that a writer rewrites (north#560, north#572, north#627). North#630's last box makes those writers emit the fields themselves.

## Ranked list

`priority:now`, ranks 1 to 11:

1. north#633 Landing takes seconds: all other work reaches main through the lander, and queue-to-main was p50 712 s against a 2 s push.
2. north#638 GitHub through one budgeted gateway: the personal REST quota ran out at 4:05 AM and again at 4:24 AM on 2026-11-06. Each time, dispatch, the sheriff, the CI watch, after-landing ci-budget and issue edits all stopped together.
3. north#624 gex45 cannot mint the push token: about 15 lanes that already pass their checks sit on gex45 until this lands, and the fix is small (answer from the host's pushed App token).
4. north#634 Verification 10 to 100 times faster: every after-push run, bisect and revert waits on it.
5. north#622 Innocent reverts and main's red shell tests: box 3 is settled (8f51ede07). What remains is main's safe-push and follows-release tests and the reland of north#615's help line.
6. north#560 main is red: while main is red, every after-push run carries no signal.
7. north#635 Every test earns its place: the cull cuts the cost of each verification run, which multiplies north#634.
8. north#636 Proxy orchestration by construction: proxy overrides of the queue hold starved the lander, and workers launched with no reader reported to no one.
9. north#562 Ledger host history reads: concurrent launches and summaries each read the whole history on the 3-core ledger host, so launches and sends time out in bursts.
10. north#631 Exited-claim sweep: every mutating fleet verb pays a 2 s ledger timeout, and claims never clear.
11. north#573 Placement picks a host that can run the box (merged with north#632): launches die at start or land on nexus, the ledger host.

`priority:next`, ranks 12 to 29:

12. north#630 Dispatch's false launch failures, plus machine-written issues it cannot staff: the noise trips ops-check and hides real failures.
13. north#637 fleet-tick timeouts: spooled cross-host messages and park and revive decisions wait while the tick runs long.
14. north#589 lane-gc keeps worktree targets: gex45's disk falls under the admission floor, and admission then defers every build.
15. north#593 Session-start probe timeouts: these fail runtime promotes, so hosts fall behind main. The fix landed (e38064279), and the remaining boxes are observations.
16. north#612 Release link failures: box 1 landed (9feb36e44), and 10 clean builds remain to observe.
17. north#627 Agent configuration check on whiterabbit: a rolling alarm that closes itself.
18. north#611 Harness services on whiterabbit: a rolling alarm that closes itself.
19. north#626 Capacity watchdog caps a bystander: the wrong session gets throttled while the real load runs on.
20. north#621 safe-push as a Rust module tree: removes the largest shell file, which the language policy bans, and the scenarios that only bash can run.
21. north#616 release-on-tag in the last repositories: two repositories remain, one of which waits on clause#1.
22. north#572 After-landing gate debt: a log that the gate rewrites. It can be staffed once north#630 box 4 makes the writer keep a box.
23. north#259 Capacity from measured usage: an umbrella, and its live landing work moved to north#633 and north#634.
24. north#360 Landing flow: an umbrella, and its precheck boxes were dropped as superseded by north#633.
25. north#88 Port North's Python and shell to Rust: a queue, and its safe-push boxes moved to north#621.
26. north#255 Ledger truth: estimates and cost cells. It now holds the per-run tier cost box from north#579.
27. north#112 The learning loop: most of its boxes wait on dates or on Tom's proxy-window decision.
28. north#522 Fleet and runtime follow-ups: the gex45 access plan boxes B16 to B23, two of which are Tom's.
29. north#608 Cost rise alarm: a rolling alarm that `north tune` keeps.

`priority:later`, ranks 30 to 33:

30. north#585 wayvnc dies on a clipboard event: affects only private-desktop runs.
31. north#523 Guards and skills follow-ups: one box, which waits on the 2026-11-19 review.
32. north#525 Experiments: their boxes wait on 2026-11-08 and 2026-11-18.
33. north#564 Tooling arm window: its box is gated until 2026-11-20.

## Closed or merged on 2026-11-06

- north#618: both boxes were ticked (eb3fddf5f).
- north#620: closed into north#622, whose box 2 is the same help line.
- north#628: the fix landed as 0e32eef60.
- north#610: settled by 9acedb2be and 6909cadd2. No landed orphan land-* branch remains on origin.
- north#632: merged into north#573.
- north#579: db9698881 closed it. Its per-run tier cost cause moved to north#255.
- north#625: its own worker closed it during the straightening.

## DAG

Only one wait is at the issue level. Dispatch reads it from GitHub's blocked-by relation:

- clause#1 -> north#616

Every other wait is between boxes. Dispatch cannot express a wait between boxes, so these edges are for the workers who take the boxes, written as `prerequisite -> dependent`:

- north#624 -> north#622 box 2, when the reland lands from gex45 (from the laptop it waits on nothing)
- north#630 box 4 -> north#572 (staffable by dispatch)
- north#630 box 4 -> north#627 (staffable by dispatch)
- north#630 boxes 1 and 2 -> north#630 box 3 (a day of ops check)
- north#633 boxes 2, 4 and 5 -> north#633 box 6 (the measured p50 and p90)
- north#634 Splits 1, 2 and 3 -> north#634 box 1 (15 to 20 s end to end)
- north#259 "Split north-core" boxes -> north#634 Split 2 (north-guard and north-ops stop rebuilding)
- north#635 box 2 (per-test catch evidence) -> north#635 boxes 4, 5 and 6 (makgora, wisp and other culls)
- north#638 box 2 (broker) -> north#638 boxes 3 (guard), 4 (mirror) and 5 (budgets)
- north#112 F1 (Tom's decision) -> north#636 box 4 (one proxy at a time)
- north#522 B18 -> B19 -> B20; B16 and B21 are Tom's; B21 -> B23 -> north#360 B23/B24
- north#621 plan box order P2a to P15 (docs/rationale/safe-push-composition.md) -> north#621 boxes 2 to 5
- cloud lane cloud-ea86cb99 -> north#562 box 3
- each fix -> its observation box: north#593 box 2, north#612 box 2, north#631 box 3, north#637 box 3
