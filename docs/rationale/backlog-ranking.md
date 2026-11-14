# Backlog ranking: north's open issues in the roadmap's stage order, with the edges between them

North's open issues on 2026-11-14, ranked by the roadmap stages decided that day (south#142, record `agents/decisions/harness/north-roadmap-2026-11-14.md` in south), with the waits between them as an edge list. The list it replaces ranked the issues open on 2026-11-06; by 2026-11-14 all 32 were closed, so every issue fell to the unranked tail and `north graph ready` ordered its top by issue name, which put filler such as firn#41 and makgora#1122 above `priority:now` and red-main work (north#1544).

## How dispatch reads this order

`north fleet dispatch` (crates/north-dispatch/src/dispatch/decision.rs, `ordered`) sorts issues in `north graph ready`'s measured order: the 24 h harness usage an issue owns per dollar one of its boxes costs, most first (north#734). Among equals, and for every issue that owns no measured loss, it sorts by priority label (`main-red`, then `priority:now`, then no label, then `priority:next`, then `priority:later`), then by the rank below, then oldest first, then project, then issue number as a number. An issue with no priority label or no rank takes its parent issue's; an issue with neither its own rank nor a parent's sorts after the ranked issues of its label. `north graph ingest` reads this file into north-graph as `(north#1537 :rank 1)`, so the order here is the order dispatch staffs in among issues of equal measured worth and label.

Dispatch keeps an issue back while any issue it waits on is open. It finds those waits in `Follows:` lines, `Blocked by` lines and GitHub blocked-by relations (crates/north-dispatch/src/dispatch/issues.rs, `predecessors`). It also keeps an issue back when the fact graph holds every unticked product box of it waiting: a box waits on what its own text names after `waits on`, `waiting for`, `blocked by`, `depends on`, `follows`, `after` or `needs`, which is an issue (`north#560`, `#560`), a box of the same issue by number (`box 3`) or by its leading label (`B16`, `4-3`, `Prune 5`), and on each edge in the DAG section below. A wait is met when the box it names is ticked or the issue it names is closed. A wait that names a reference the ingest cannot resolve, or a `box` described only in words, is not an edge: ingest prints it as `cannot parse a wait of ...`, and the box stays ready until its text names the box or issue by one of the forms above. A `Needs:` line is a placement need such as `gex45` or `native`, which the brief copies so placement can read it. So an issue-to-issue wait goes in `Follows: north#N`, never in `Needs:`. Within a priority, free slots usually outnumber ready issues, so every ready one is staffed in the same round and the rank decides the order when they do not.

Dispatch refuses to place a `bug` issue that has no `Class:` line (crates/north-pace/src/router/brief.rs, `class_refusal`). It also skips an issue with no `- [ ]` box.

## Ranked list

Stage 0, a green main and a pipe that lands, ranks 1 to 6:

1. north#1537 main is red: while main is red, every after-push run carries no signal.
2. north#1508 Fleet builds run on build hosts and come back whole.
3. north#1495 A take-over, five simultaneous launches and a nested-scope request each end in a working session.
4. north#1539 After-landing gate debt.
5. north#1540 Runtime promotion: main's newest green commit is live on every host within 10 minutes.
6. north#1549 Hand-ins from build-host lanes are fetched by the lander, not decided conflict unreadable.

Stage 1, the comparison cells the announcement waits on, rank 7:

7. north#1542 T3 Code's noise band, small and large cells, the graph-split large task and the context pack on and off.

The moat track, rank 8:

8. north#1544 Every shadow gate enforces or is deleted on a reading of its rows.

Stage 2, launch-ready install and confinement, ranks 9 and 10:

9. north#1484 North's install and fleet screen are launch-ready and the history cut has its freeze.
10. north#1485 Worker confinement holds, and the sandbox refuses what it cannot confine.

The pipe's other work, ranks 11 to 13:

11. north#1543 The laptop's load number tells the truth and the owner's seat is never starved by the fleet.
12. north#1545 The laptop's home disk holds 3 TB free.
13. north#1531 A warm-spare launch reaches its first model request in under 1 s; seconds on one small task stop once it lands.

Outside the stages, ranks 14 to 16:

14. north#1547 Graded outcomes are machine facts.
15. north#1487 north-ops is gone: it serves no stage and parks after its in-flight re-land.
16. north#1546 Each later roadmap stage opens on a measurement; nothing in it is staffed before the announcement.

## DAG

`north graph ingest` reads each line below as edges when both ends are written as `north#N`, or `north#N box K` or `north#N boxes K, L and M` (the Kth box in the issue body, ticked or not), and prints each line it cannot read as `cannot parse a wait in backlog-ranking.md`. Written as `prerequisite -> dependent`:

- north#1544 -> north#1546
- north#1542 -> north#1546
