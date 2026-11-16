# Throughput DAG: north's open work ordered by what raises landed commits a day

North's open issues on 2026-11-16, each placed on the pipeline stage it widens, ordered by how much it raises the rate at which commits land on main, with a claim for each of the first twenty about what changes when it lands. The owner asked for this on 2026-11-16 (north#1632, box "The open backlog is ranked by throughput leverage in one saved graph"). It is a proposal: no label, rank or issue was changed to write it, and `north graph ingest` does not read this file (dispatch's order still comes from [backlog-ranking.md](backlog-ranking.md) and the priority labels).

What exists before this note: dispatch staffs by measured harness loss per dollar, then the priority label (65 `now`, 42 `next`, 14 `later`, 27 unlabelled of 148 open at 03:00 UTC), then the rank in backlog-ranking.md, then age (crates/north-dispatch/src/dispatch/decision.rs, `ordered`). Nothing in that order names a pipeline stage. The aim of north#1632 is to show that the architecture sustains 5,000 landed commits a day, about 210 an hour, not to produce them.

Each reading below is marked **M** (measured by the command named, 2026-11-16 about 03:00 UTC), **C** (read from code), **B** (a worker's report of that morning, quoted in the brief or in north#1632's boxes, not re-taken) or **G** (a guess, with what settles it).

The layer column follows north#1648, which the owner decided after this note was begun: layer 1 is landing (lanes, hand-in and its checks, the lander, review), 2 session policy (skills, hooks, guards in a profile), 3 fleet (launching, messaging and reading sessions across hosts, the sandbox and the broker), 4 autopilot (dispatch, the reaper, routing, runtime promotion, after-landing search and revert). A new feature in layers 2 to 4 waits until that layer's measured verdict pays; a stability fix does not wait.

## 1. Stages

The rate is the smallest stage capacity. Capacities are in landed commits an hour unless the column says otherwise.

| Stage | Capacity an hour today | How known | What limits it | Reading that shows it moved |
|---|---|---|---|---|
| Landed on main (the outcome) | 447 commits in 24 h, 19/h on average, peak 35/h; 28 in the last hour; 13 reverts | M `git log origin/main --since='24 hours ago' --first-parent` | the stages below | same command, per hour |
| Model usage | about 56/h at 19 commits a weekly-window point (24 h), 27/h at 9 a point (7 days); 5 accounts give at most 71 points a day | B north#1610 (measured 2026-11-15 from `north accounts --json`) | points a landed commit costs; leads took 22.3% and reviewers 15.8% of a day's usage (north#1610) | commits per point in north#1610's profile |
| Dispatch starts sessions | 9 working of about 28 slots at 03:05 UTC; about one start per 20 min on 2026-11-16 (north#1632 box 49) | M `north fleet tree` (9 working, 41 parked, 17 idle); B box 49 | the round died with the tick until f7ea2d64e6; host CPU room reads 0 (box 49) | working sessions near free slots; the round line of `north work` (e94f5e2413) |
| Session slots | about 28: idle cores / 0.5 per build host, plus `slot_cap = 16` on the Go route; memory would carry about 23 per 16-core host | C crates/north-dispatch/src/dispatch/slots.rs:9 (`SESSION_CORES_GUESS`, a guess by its own comment), harness/opencode.toml:15; B box 49 | build-host CPU, mostly not sessions: on gex45 after-landing runs, reruns and culprit searches took 4.6 of 9.8 busy cores, session clients 0.45 for 14 sessions (box 39) | `north fleet dispatch --dry-run` prints free slots on two hosts three rounds running (box 39) |
| Session yield | about 2 landed per working session-hour; 248 of 311 sessions landed exactly one commit; 257 hand-ins refused against 431 landed (72 missing accept, 56 code-health, 46 stale base, 22 tests) | B box 6 and the brief; M `north land gate-stats --since 48h`: 372 pre-queue checks exited 1, 203 refused because every build host's check slot line was full | refusals inside the worker's turn; hung sessions listed as working (box 30); one commit per session | `north work` landed per working worker-hour (box 11) |
| Pre-queue checks | unknown as a rate; 203 refusals in 48 h for no build-host check slot | M gate-stats | the same build-host CPU as slots | that refusal count near 0 |
| Review | 1 reviewer session at about 35 accepts an hour peak; 60% of lanes need one, so about 58 landings an hour | B box 4 (`north land profile`) | one listed reviewer, no claim per patch | accepts by two reviewers in one hour (box 4) |
| Lander, queue to main | 440 landed in 24 h, queued to main p50 0.5 min, p90 6.4; last hour p50 0.4, p90 1.0; 0 queued | M `north ledger tempo --window 1440` and `--window 60` | serial candidate checks; about 120/h if each takes the p50 (G: the proof run, box 1, settles it) | the proof run's queue-to-main p90 at 210 hand-ins an hour |
| After-landing verification | one run at a time (`after-runs = 1`, .safe-push:8); a run covers every landing that arrived while the one before it ran (`Pending::add` absorbs each landing whose base is the waiting batch's head, crates/north-lander/src/land/watch.rs:135), so it covers any number of landings and caps verdicts, not landings: about 3,600 / 651 = 5.5 runs an hour at the p50 (north#1583), each covering about 5 landings at 28/h and about 38 at 210/h. In 45 min to 03:06 UTC, 22 landed, 14 had no verdict, 7 read red; queue waits for the run in flight of 755, 541, 486, 417 and 400 s; one run 730 s, 560 s of it the culprit search | C watch.rs, .safe-push; M `north land profile --since 45m --repo north` (read by the coordinator) | each run is bounded by its own deadline (`post_land::limit`, learned per head from the ledger); a red verdict queues a culprit search, one at a time per repository, bounded by the run deadline times 2 per lane in the batch plus 18 rounds (crates/north-landing/src/land/post_land.rs:67, `WALK_BACK = 16`), so a search grows with the batch and at 38 lanes a batch could run 94 run-deadlines | verdict lag under one run time; searches per red window 0 for fix-forward lines (box 8) |
| After landing, follow and gates | full graph ingest 177 s median, now coalesced (95c7a5c470); per-head gates 19 s median, about 190/h serial (G) | B box 2 | not a cap on landings since the coalesce; with verification it takes the build-host CPU slots need (box 39) | box 39's per-class CPU shares |
| Per-landing work on the laptop | unknown | M at about 03:00 UTC the launcher refused to place a session on the laptop because the owner's seat was stalled 16% of the time (reported by the coordinator) | laptop IO and CPU shared with the owner's seat (north#1553, north#1543) | per-landing seconds on the laptop; not measured by any command today |
| Reaper | parked sessions fell by about 1 a minute with 240 waiting | B brief | parked Claude sessions hold no slot (box 49); parked Go-route sessions hold the cap of 16 (box 53) | parked count under 20 with 60 working (box 22) |
| GitHub API | about 10 units a landing, over 8,000 units left in an hour on one token, so over 500/h | B brief | rate limit per token | units per landing at the proof run |

## 2. What limits the rate now

**Now: session slots, through build-host CPU.** The rate is working sessions times yield. 9 working sessions at about 2 landed an hour give about 18/h, the 24-hour average of 19/h; the last hour's 28 is the top of the same band. Dispatch does not leave slots free: slots read 0 on every build host at 9:21 PM on 2026-11-15 (box 39) and again on 2026-11-16 (box 49) because build-host CPU is held, and box 39 measured that about half the busy cores on gex45 do after-landing runs, reruns and culprit searches, while 14 sessions' own clients used 0.45 cores. A session is charged 0.5 cores against a measured ~0.03 (C slots.rs:9 against B box 39), so the rule turns landing work into closed slots. Of the 28 slots, the 16 on the Go route landed 0 of 22 lanes on 2026-11-15 (box 33), so the working ceiling today is the 12 host slots × 2 = 24/h, which is the observed 19 to 28. With all 28 slots landing it is 56/h.

**After-landing verification does not cap landings, and is ranked as stability and CPU, not as the limit.** A run covers every landing since the last (watch.rs:135), so landings never wait on it; verdicts trail by one to two run times (10 min and more at 28/h, as the 03:06 profile shows). What grows with the rate is the cost of a red verdict: the culprit search runs one at a time with a bound of 2 run-deadlines per lane in the batch plus 18, so at 210/h a red batch of about 38 lanes could hold the search for over an hour while newer red verdicts queue behind it, and every search takes the build-host CPU slots need. That is why north#1641, box 8 (no search for fix-forward lines) and box 41 rank above the dispatch work in section 5: they free the CPU of the limiting stage and bound the one cost that grows with the batch.

**Next two, both near 56/h:** review, at 35 accepts an hour over a 60% share, 35 / 0.6 = 58/h; and model usage, 71 points a day × 19 commits a point = 1,350 a day = 56/h (27/h at the 7-day rate). Removing the CPU limit lifts the rate toward 56/h, where these two bind together, once the Go route lands (box 33, box 53). The lander (about 120/h, G) and GitHub (over 500/h) are not on the path below 100/h.

**What 210/h needs**, at today's yield of 2: about 105 working sessions (CPU at a measured 0.03 cores each is about 3 cores; memory at about 23 per host gives about 70 on three hosts, so memory binds next at about 140/h, G); 126 accepts an hour, which is four reviewer sessions or one-shot review calls (north#1610); 3.75 times fewer usage points a commit, or provider capacity outside the five accounts (the Go route landed 0 of 22 lanes on 2026-11-15, box 33); and a lander that holds 210/h, which only the proof run (box 1) can show. Raising yield (box 11: several commits per session) divides the session count by the same factor. The proof run uses stand-in sessions with no model spend, so it tests every stage except model usage.

## 3. Every open issue

One line per open north issue at 03:00 UTC on 2026-11-16 (`gh issue list -R tompassarelli/north --state open`), and one per open box of north#1632, numbered by its place among all boxes in the body, ticked or not, as `north graph ingest` counts them. Leverage: **limiting** unblocks the limiting stage (build-host CPU, slots, dispatch starting sessions); **later** widens a later stage; **stability** keeps main green or the fleet running; **none** touches no stage. Kind is stability fix or feature, in north#1648's sense. Depends on lists open blocked-by relations, parents and `Follows:` lines (GraphQL `blockedBy` and `parent`). Stage and class are this note's reading of each title and body, not a measurement.

| Issue | What it is | Stage | Depends on | Leverage | Layer | Kind | Label |
|---|---|---|---|---|---|---|---|
| north#1036 | Place and admit fleet builds by measured free capacity | slots | - | limiting | 4 | feature | now |
| north#1054 | Every fleet launch is built once, admitted and starts | dispatch | - | limiting | 4 | stability | next |
| north#1247 | No test runs twice; no queue waits (original of north#1559) | slots | - | limiting | 1 | feature | now |
| north#1418 | Processes outlive their session on a build host | slots | - | limiting | 3 | stability | now |
| north#1508 | Fleet builds run on build hosts and come back whole | slots | - | limiting | 3 | stability | now |
| north#1559 | No test runs twice (rewrite of north#1247) | slots | - | limiting | 1 | feature | - |
| north#1583 | Tests that do not earn their time are removed | after-landing | - | limiting | 1 | stability | now |
| north#1615 | gex45 under its disk bound; eviction does not restore | slots | - | limiting | 4 | stability | - |
| north#1625 | Dispatch staffs only allowed boxes under accepted names | dispatch | - | limiting | 4 | stability | now |
| north#1632 | The architecture is shown to sustain 5,000 commits a day | all | - | limiting | none | stability | now |
| north#1022 | Runtime canary checks the core loop | after-landing | - | stability | 4 | stability | now |
| north#1353 | A lost after-landing run is never promoted green | after-landing | - | stability | 4 | stability | - |
| north#1362 | Fleet work stalls the laptop's disk | laptop | - | stability | 3 | stability | now |
| north#1393 | Agent-login guards did not fire on cpx62 | not throughput (security) | - | stability | 2 | stability | now |
| north#1397 | Socket hiding misses some runtime sockets | not throughput (security) | - | stability | 3 | stability | now |
| north#1422 | A trust-boundary change landed with no accept | review | - | stability | 1 | stability | next |
| north#1423 | Lander reverts when fails-first could not run | after-landing | - | stability | 4 | stability | next |
| north#1453 | Fleet tick burns half a laptop CPU | laptop | - | stability | 3 | stability | next |
| north#1485 | Worker confinement holds | not throughput (security) | - | stability | 3 | stability | now |
| north#1538 | Untasked launch records an untasked lead | not throughput (security) | - | stability | 3 | stability | now |
| north#1540 | Main's newest green commit live on every host in 10 min | after-landing | - | stability | 4 | stability | now |
| north#1541 | Builds outside the sandbox must not read cargo home config | not throughput (security) | - | stability | 3 | stability | now |
| north#1543 | Laptop load tells the truth; Tom's seat never starved | laptop | - | stability | 3 | stability | now |
| north#1550 | A confined check can plant source an unconfined run uses | not throughput (security) | - | stability | 3 | stability | now |
| north#1551 | Laptop runtime follower fails to start | after-landing | - | stability | 4 | stability | now |
| north#1553 | Fleet disk work never stalls Tom's desktop | laptop | - | stability | 3 | stability | now |
| north#1566 | Sheriff counts a no-test check as a pass | after-landing | - | stability | 4 | stability | next |
| north#1572 | Control plane runs on a build host | laptop | north#1619, north#1608 | stability | 3 | stability | now |
| north#1573 | No test times real work against a wall clock | after-landing | - | stability | 1 | stability | now |
| north#1608 | Control-plane lease out of worker-writable stores | not throughput (security) | - | stability | 3 | stability | next |
| north#1612 | A break on main is caught, blamed and reverted | after-landing | - | stability | 4 | stability | now |
| north#1620 | Offloaded build links an older source tree | yield | - | stability | 1 | stability | next |
| north#1624 | Offload nested-job test misses a 10 s bound | after-landing | - | stability | 1 | stability | next |
| north#1628 | A dependency-box test overwrote cpx62b's fetcher cache | slots | - | stability | 3 | stability | now |
| north#1629 | Broker holds each caller by a handle through its checks | not throughput (security) | - | stability | 3 | stability | - |
| north#1631 | Lander stopped 25 min while the alarm said healthy | lander | - | stability | 1 | stability | now |
| north#1634 | A '#' in .safe-push reasons refuses every hand-in | yield | - | stability | 1 | stability | now |
| north#1641 | main is red (CI runs, culprit ranges) | after-landing | - | stability | 1 | stability | - |
| north#259 | Capacity from measured usage: pace, spawn guard, placement | usage | - | later | 4 | feature | now |
| north#1115 | Buck2 with remote execution (original of north#1590) | slots | - | later | 1 | feature | now |
| north#1132 | Cost per closed box rose over 25% week on week | usage | north#1279 | later | 4 | stability | later |
| north#1233 | Small tasks start on the small model | usage | north#1279 | later | 4 | feature | now |
| north#1249 | Continue unlanded lanes the triage kept | yield | north#1054 | later | 3 | stability | next |
| north#1334 | Cut the compile cost of each batch's pre-push build | lander | - | later | 1 | feature | next |
| north#1352 | Sandbox broker refuses a finish check with no session scope | yield | - | later | 3 | stability | now |
| north#1379 | Hand-in refuses a lane whose commit deletes a file | yield | - | later | 1 | stability | next |
| north#1394 | A finish call from the agent login on cpx62 is refused | yield | - | later | 3 | stability | now |
| north#1430 | gex45's Buildbarn worker serves no action | slots | - | later | 1 | stability | now |
| north#1438 | Reasoning effort per model (DeepSeek runs low) | usage | - | later | 4 | feature | next |
| north#1455 | Hand-in loses the commit message's trailer | yield | - | later | 1 | stability | now |
| north#1549 | Build-host hand-ins fetched, not 'conflict unreadable' | yield | north#1634 | later | 1 | stability | now |
| north#1556 | A land check's bun run keeps its environment | yield | - | later | 1 | stability | next |
| north#1557 | Build host cargo config fails a capacity host case | yield | - | later | 1 | stability | now |
| north#1560 | Pre-queue checks hit ETXTBSY on the shared north build | yield | - | later | 1 | stability | now |
| north#1561 | Offload reattach test ends a job early under load | yield | - | later | 1 | stability | next |
| north#1562 | Lander's shadow tick never waits on a build host queue | lander | - | later | 1 | stability | now |
| north#1563 | Concurrent launches run a claim before hosts are live | dispatch | - | later | 3 | stability | now |
| north#1564 | Ledger spool test fails lanes it does not cover | yield | - | later | 1 | stability | next |
| north#1570 | Regenerate step finds no build-ahead artifact for north-docs | lander | - | later | 1 | stability | now |
| north#1575 | A finished lane lands without a session | yield | - | later | 1 | feature | now |
| north#1578 | Offloaded land checks reach no verdict (exit 75) | yield | - | later | 1 | stability | now |
| north#1589 | Route and pace read one run row per provider | usage | - | later | 4 | feature | now |
| north#1590 | Buck2 remote execution (rewrite of north#1115) | slots | - | later | 1 | feature | - |
| north#1593 | Sandboxed cargo never offloaded (absolute path) | yield | - | later | 3 | stability | next |
| north#1594 | Fleet send over 3 kB fails as read-only ledger | yield | - | later | 3 | stability | next |
| north#1596 | Review-accept judges south lanes by south's paths | review | - | later | 1 | stability | now |
| north#1598 | Brief guard refuses the word login as a sign-in plan | dispatch | - | later | 2 | stability | next |
| north#1600 | Every unlanded lane over a day landed or archived | yield | - | later | 1 | stability | next |
| north#1604 | A message during a hand-in stops the hand-in | yield | - | later | 3 | stability | now |
| north#1606 | Compacted session flagged as ending without a report | yield | - | later | 3 | stability | next |
| north#1609 | spare-fill --host fails with no --account | dispatch | - | later | 4 | stability | next |
| north#1610 | A landed commit costs half the account usage | usage | - | later | 3 | feature | now |
| north#1619 | carry_matrix cases fail when a land check holds the queue | yield | - | later | 1 | stability | now |
| north#1621 | A silent 'working' session is reported as a stall | yield | - | later | 3 | stability | now |
| north#1623 | carry_matrix cannot pass inside a worker session | yield | - | later | 3 | stability | next |
| north#1643 | Ledger records no review-call runs | review | - | later | 1 | feature | - |
| north#112 | Daily tune tick that changes decisions from the ledger | not throughput (eval) | - | none | 4 | feature | next |
| north#525 | Experiments from the mattpocock-skills review | not throughput (eval) | north#112 | none | 2 | feature | later |
| north#641 | Proxy files its recommendations as issues (original of north#1580) | not throughput (hygiene) | - | none | 3 | feature | next |
| north#643 | Decisions as pure strategy, verified by replay | not throughput (hygiene) | - | none | 4 | feature | next |
| north#709 | Concerns registry and supervisor views (original of north#1558) | not throughput (hygiene) | north#1279 | none | 4 | feature | next |
| north#722 | After-landing gate debt (original of north#1571) | not throughput (hygiene) | north#1247 | none | 1 | stability | later |
| north#865 | Code-health reports a code comment with no named constraint | not throughput (hygiene) | - | none | 1 | feature | - |
| north#1041 | Every guarantee in a provider-independent layer | not throughput (security) | - | none | 2 | feature | next |
| north#1042 | Model-check the riskiest state machines | not throughput (hygiene) | - | none | 1 | feature | next |
| north#1048 | Coordinator window Tom can type into | not throughput (product) | north#1054 | none | 3 | stability | now |
| north#1137 | Work tracked as sub-issues closed by their landing | not throughput (hygiene) | north#1281, north#1635 | none | 4 | feature | now |
| north#1147 | Brief carries graph grounding; eval arm measures it | not throughput (eval) | north#1279 | none | 2 | feature | later |
| north#1279 | North improves itself from its own records | not throughput (eval) | - | none | 4 | feature | now |
| north#1281 | Ready to be shown widely: docs and a path to a landing | not throughput (docs) | north#1484, north#1485, north#1429 | none | none | feature | now |
| north#1306 | Estimates drifted outside the calibrated band | not throughput (eval) | - | none | 4 | stability | - |
| north#1335 | Global rule: add a guard only with evidence | not throughput (hygiene) | - | none | 2 | feature | - |
| north#1366 | Parked by gate 242: large work outside the roads | not throughput (parked) | - | none | none | feature | later |
| north#1384 | Lander counts a session name only with the broker's proof | not throughput (security) | - | none | 1 | stability | next |
| north#1427 | Coordinator take-over opens a viewer, not a client | not throughput (product) | - | none | 3 | stability | now |
| north#1429 | Second frozen set of five large tasks (Buck2) | not throughput (eval) | - | none | none | feature | later |
| north#1431 | Broker ops hotspot at complexity 95 | not throughput (hygiene) | - | none | 3 | stability | next |
| north#1435 | Split north-ops into domain crates | not throughput (hygiene) | - | none | none | feature | now |
| north#1439 | Eval leads start blocks without the coordinator | not throughput (eval) | - | none | 3 | feature | later |
| north#1441 | Land watch miscounts a running land check | lander | - | none | 1 | stability | next |
| north#1484 | Install and fleet screen launch-ready | not throughput (product) | - | none | 3 | feature | now |
| north#1487 | north-ops is gone | not throughput (hygiene) | - | none | none | feature | now |
| north#1531 | Warm-spare launch reaches its first request under 1 s | dispatch | - | none | 3 | feature | now |
| north#1539 | After-landing gate debt | not throughput (hygiene) | - | none | 1 | stability | later |
| north#1542 | Every comparison cell reads | not throughput (eval) | north#1570, north#1563, north#1560 | none | none | feature | later |
| north#1544 | Every shadow gate enforces or is deleted | not throughput (hygiene) | north#1557 | none | 1 | feature | now |
| north#1545 | Laptop home disk holds 3 TB free | not throughput (hygiene) | - | none | none | stability | now |
| north#1546 | Later roadmap stages open on a measurement | not throughput (product) | - | none | none | feature | later |
| north#1547 | Graded outcomes are machine facts | not throughput (eval) | - | none | 4 | feature | next |
| north#1552 | Name each account by its sign-in email | not throughput (product) | - | none | 3 | feature | now |
| north#1558 | Concerns registry (rewrite of north#709) | not throughput (hygiene) | - | none | 4 | feature | - |
| north#1569 | Eval arms write their own worktree | not throughput (eval) | - | none | none | feature | later |
| north#1571 | After-landing gate debt (rewrite of north#722) | not throughput (hygiene) | - | none | 1 | stability | - |
| north#1574 | One capability record per session | not throughput (security) | - | none | 3 | feature | now |
| north#1576 | Every alarm names its action and owner | not throughput (hygiene) | - | none | 4 | stability | now |
| north#1577 | Refuse the coordinator's second manual unblock | not throughput (hygiene) | - | none | 2 | feature | now |
| north#1579 | Retire north tune's effort cells | not throughput (hygiene) | - | none | 4 | feature | next |
| north#1580 | Proxy files its recommendations (rewrite of north#641) | not throughput (hygiene) | - | none | 3 | feature | - |
| north#1581 | Privacy judge renamed disclosure judge | not throughput (hygiene) | - | none | 1 | feature | later |
| north#1582 | Review the debugging skill | not throughput (docs) | - | none | 2 | feature | - |
| north#1584 | Issue creation fills implied labels | not throughput (product) | - | none | 4 | feature | next |
| north#1585 | Review the addyosmani-agent-skills source | not throughput (docs) | - | none | 2 | feature | - |
| north#1587 | No closed issue holds an unticked box | not throughput (hygiene) | - | none | 4 | feature | next |
| north#1588 | Every no-gain prior read from kept rows | not throughput (eval) | - | none | 4 | feature | next |
| north#1591 | Move trust-boundary lists out of north-base | not throughput (hygiene) | - | none | 1 | feature | later |
| north#1595 | Typed answer refused by relayed-turn check | not throughput (product) | - | none | 3 | stability | now |
| north#1597 | Work split reads crate cones at the base commit | yield | - | none | 4 | feature | later |
| north#1599 | Work split's planning call through the broker | yield | - | none | 3 | feature | now |
| north#1601 | Spelled-out option closes its gate | not throughput (product) | - | none | 3 | stability | next |
| north#1602 | Finish tool refuses main and a local session's lane | not throughput (security) | - | none | 1 | stability | next |
| north#1603 | Stop the stale-issues service closing at 24 h | not throughput (hygiene) | - | none | 4 | stability | next |
| north#1607 | Review the repo-safety skill | not throughput (docs) | - | none | 2 | feature | - |
| north#1614 | Strategy log retain unit fails on the laptop | not throughput (hygiene) | - | none | 4 | stability | - |
| north#1617 | A plan-usage gate opened as a cost question | not throughput (product) | - | none | 3 | stability | - |
| north#1626 | Coordinator lands the config change it made | not throughput (product) | - | none | 3 | feature | next |
| north#1627 | lane gc hides the failing liveness step | not throughput (hygiene) | - | none | 1 | stability | now |
| north#1633 | north tune --dry-run fails in a worker session | not throughput (hygiene) | - | none | 4 | stability | next |
| north#1635 | Dry run refuses mktemp's empty manifest | not throughput (hygiene) | - | none | 4 | stability | now |
| north#1636 | Gate debt: code-comment habit on 7fc83b3fb333 | not throughput (hygiene) | north#1539 | none | 1 | stability | - |
| north#1637 | Review the machine-capacity skill | not throughput (docs) | - | none | 2 | feature | - |
| north#1638 | Review the makgora skill | not throughput (docs) | - | none | 2 | feature | - |
| north#1639 | Review the planning skill | not throughput (docs) | - | none | 2 | feature | - |
| north#1642 | Gate debt: crate-lines on 7e48742b78c6 | not throughput (hygiene) | north#1539 | none | 1 | stability | - |
| north#1644 | Gate debt: rust line budget on a3d2d5bd477f | not throughput (hygiene) | north#1539 | none | 1 | stability | - |
| north#1645 | Gate debt: whole-tree habits on a3d2d5bd477f | not throughput (hygiene) | north#1539 | none | 1 | stability | - |
| north#1646 | Run the mechanism-discovery turn | not throughput (eval) | - | none | 4 | feature | - |
| north#1647 | Review the workers skill | not throughput (docs) | - | none | 2 | feature | - |
| north#1648 | A proven core and opted-in layers, each with a verdict | not throughput (product) | - | none | none | feature | next |
| north#1632 box 1 | Proof run: stand-in sessions show the pipeline holds the rate | all (measurement) | boxes 39, 2 | limiting | 1 | feature | now |
| north#1632 box 5 | Fleet tick on the dispatch host finishes within a minute | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 8 | Only stop-class after-landing failures hold or revert | after-landing | - | limiting | 4 | stability | now |
| north#1632 box 12 | Every build host takes workers; dispatch host keeps disk | slots | - | limiting | 4 | stability | now |
| north#1632 box 16 | Dispatch launch from nexus starts its session host | dispatch | - | limiting | 3 | stability | now |
| north#1632 box 30 | A session listed working is attached and fed | yield | - | limiting | 3 | stability | now |
| north#1632 box 32 | cpx62's shared build cache keeps its memory | slots | - | limiting | 3 | stability | now |
| north#1632 box 36 | A box with closed predecessors staffed within 30 min | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 39 | Build hosts have CPU left for sessions | slots | - | limiting | 4 | stability | now |
| north#1632 box 41 | Landing CPU measured per check line; largest cut | slots | - | limiting | 1 | stability | now |
| north#1632 box 42 | Dispatch holds a box only for a file another lane edits | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 43 | After-landing test line never falls back to the workspace | slots | - | limiting | 1 | stability | now |
| north#1632 box 46 | Land doctor's disk remedy frees what fills the disk | slots | - | limiting | 4 | stability | now |
| north#1632 box 47 | Dispatch launches stop failing after 2026-11-16 changes | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 48 | Dispatch host's state backup yields to the fleet | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 49 | Dispatch fills every free slot and names what held the rest | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 51 | Dispatch host frees its state before the disk freeze | dispatch | - | limiting | 4 | stability | now |
| north#1632 box 9 | A session's GitHub call through the broker is answered | yield | - | stability | 3 | stability | now |
| north#1632 box 25 | South catalog owner checks run only on the laptop | after-landing | - | stability | 1 | stability | now |
| north#1632 box 26 | The after-landing run is green again | after-landing | - | stability | 1 | stability | now |
| north#1632 box 28 | Hand-in tool integration test passes after landing | after-landing | - | stability | 1 | stability | now |
| north#1632 box 50 | Hosted Cargo test job ends on its tests, not its clock | after-landing | - | stability | 1 | stability | now |
| north#1632 box 52 | After-landing compile builds against its own tree | after-landing | - | stability | 1 | stability | now |
| north#1632 box 58 | Lander follows a promotion where leaving loses no verdict | lander | - | stability | 4 | stability | now |
| north#1632 box 2 | Following a landing costs the lander's host a bounded amount | after-landing | - | later | 4 | stability | now |
| north#1632 box 3 | Pending review waits in the landing queue, not the worker's turn | yield | - | later | 1 | stability | now |
| north#1632 box 4 | Several reviewers, each patch taken by one | review | - | later | 1 | feature | now |
| north#1632 box 6 | A fixable pre-queue refusal is fixed by its author | yield | - | later | 2 | stability | now |
| north#1632 box 11 | A worker never waits on its own landing | yield | - | later | 2 | feature | now |
| north#1632 box 15 | Finished work that never landed lands | yield | - | later | 1 | stability | now |
| north#1632 box 19 | Lanes over 24 h on open issues landed or archived | yield | - | later | 1 | stability | now |
| north#1632 box 20 | Lanes under 24 h landed by owners or archived | yield | north#1600 | later | 1 | stability | now |
| north#1632 box 22 | A parked session released within 10 min of its wait ending | reaper | - | later | 4 | stability | now |
| north#1632 box 23 | A DeepSeek lane landed or refused within an hour | yield | - | later | 3 | stability | now |
| north#1632 box 24 | A DeepSeek session finishes without a refused call | yield | - | later | 2 | stability | now |
| north#1632 box 27 | A check timing out for a slot reaches no verdict | yield | - | later | 1 | stability | now |
| north#1632 box 33 | A route that lands nothing is flagged within an hour | yield | - | later | 4 | feature | now |
| north#1632 box 34 | cpx62 starts the OpenCode client | slots | - | later | 3 | stability | now |
| north#1632 box 37 | Go route's cap sized from what it lands | slots | box 53 | later | 4 | feature | now |
| north#1632 box 45 | A dispatched session's name fits the review mirror | review | - | later | 4 | stability | now |
| north#1632 box 53 | Go cap counts sessions that can call now, not parked | slots | - | later | 4 | stability | now |
| north#1632 box 57 | A confined worker's Split files its parts | yield | - | later | 3 | stability | now |
| north#1632 box 7 | This ranking (throughput-dag.md) | all (measurement) | - | none | none | feature | now |
| north#1632 box 10 | The headline number: today's landed commits in north work | all (measurement) | - | none | 4 | feature | now |
| north#1632 box 13 | Dispatch host's disk readable without a session | dispatch | - | none | 3 | feature | now |
| north#1632 box 14 | Issue creation never refused for a missing label | not throughput (product) | - | none | 4 | feature | now |
| north#1632 box 17 | Superseded lanes archived with their reason | not throughput (hygiene) | - | none | 1 | stability | now |
| north#1632 box 18 | Uncommitted lanes on closed issues archived | not throughput (hygiene) | - | none | 1 | stability | now |
| north#1632 box 21 | lane remove --rejected keeps names over 64 characters | not throughput (hygiene) | - | none | 1 | stability | now |
| north#1632 box 29 | nexus takes each south deploy | not throughput (hygiene) | - | none | 4 | stability | now |
| north#1632 box 31 | After-landing verification stays one run at a time | after-landing | - | none | 4 | stability | now |
| north#1632 box 44 | Dispatch host's CPU readable without a session | dispatch | - | none | 3 | feature | now |

Counts: of the 148 issues, 10 limiting, 28 stability, 38 later, 72 none; of the 52 boxes of north#1632, 17 limiting, 7 stability, 18 later, 10 none.

## 4. The graph

Only the edges that decide the order, written `from -> to`:

- build-host CPU -> session slots -> dispatch starts -> working sessions -> landed commits (the chain that binds now; C slots.rs, B box 39 and box 49)
- landed commits -> after-landing runs, reruns and culprit searches -> build-host CPU (the loop: each landing takes CPU that would be a slot, so the rate pushes itself down; B box 39)
- north#1641 -> north#1632 box 8 (a red main starts culprit searches on every run; box 8 stops them for fix-forward lines)
- north#1632 box 39, box 41, box 43, box 8, north#1559, north#1583 -> session slots
- north#1632 box 49, box 5, box 36, box 47, box 48, box 51, north#1625 -> dispatch starts
- north#1632 box 30, box 6, box 3, box 11, north#1575 -> session yield
- north#1632 box 4, north#1610 -> review
- north#1610, north#1233, north#1438 -> model usage
- north#1632 box 39 -> north#1632 box 1 (a proof run on saturated hosts re-measures the CPU limit, not the pipeline)
- north#1632 box 53 -> north#1632 box 37; north#1600 -> north#1632 box 20
- north#1635 -> north#1137; north#1484, north#1485, north#1429 -> north#1281; north#1570, north#1563, north#1560 -> north#1542; north#1557 -> north#1544; north#1634 -> north#1549; north#1619, north#1608 -> north#1572 (GitHub blocked-by relations)

## 5. The first twenty, with claims

Each claim says which reading changes and what would refute it. "Rate" is landed commits an hour on north main by the `git log` count of section 1.

1. **north#1641, main is red.** Stage 0 outranks everything (standing order of 2026-11-14). Claim: the newest Harness and Cargo runs on main read success, and the after-landing culprit searches that a red run starts stop. Refuted if the runs stay red with no lane in the culprit range.
2. **north#1632 box 39, build hosts have CPU left.** Reruns and culprit searches yield, `disk keep` runs one copy. Claim: `north fleet dispatch --dry-run` on the dispatch host goes from 0 free slots on every build host to free slots on at least two, and working sessions from 9 to about 20 (G: that the freed cores, about 4 on gex45, become slots at 0.5 a session). Refuted if free slots appear and working sessions stay under 15: then dispatch, not CPU, binds.
3. **north#1632 box 8, only stop-class after-landing failures hold or revert.** Claim: culprit searches for fix-forward lines go from one per red run (553 s for six lanes in the landing profile) to 0, and good commits reverted by a search (017268abc7 on 2026-11-15) to 0. Refuted if `north land profile` still shows searches for lines classed fix-forward.
4. **north#1632 box 41 with box 43, landing CPU per check line, no workspace fallback.** Claim: after-landing cores on gex45 go from about 4.6 to under 2 (G: the share of whole-workspace runs is not yet read, and box 41's per-line reading settles it). Refuted if box 39's per-class reading shows after-landing cores unchanged.
5. **north#1632 box 49 with box 5, box 36 and box 47, dispatch fills every free slot and says why it held the rest.** Partly in f7ea2d64e6 (rounds outlive the tick) and e94f5e2413 (the round line). Claim: `north fleet held --since 60m | grep -c 'unchecked box with every predecessor closed'` goes from 15 issues (2026-11-15) to 0, and starts per round from about one per 20 min to the free-slot count. Refuted if the round line shows free slots and ready boxes with no start.
6. **north#1632 box 30, a session listed working is fed.** Claim: of sessions listed working, those spending tokens in a 10-minute window go from 2 of 61 (2026-11-15) to nearly all, so the session count of section 2 is real. Refuted if `north fleet tree` working and token-spending counts still differ by more than a few.
7. **north#1632 box 6, a fixable refusal is fixed by its author.** Claim: `north ledger duplicate` runs on a box another run still worked go to 0, and sessions that stop at their first refused hand-in from 4 a morning to 0. Refuted if the ledger shows the same refusal ending sessions.
8. **north#1632 box 3, a pending review waits in the queue.** Claim: hand-ins refused for a missing accept go from 72 a day to 0, and each reviewed lane's worker turn ends about 3.4 min sooner (median first commit to accept). Refuted if `north land profile --since 6h` still shows missing-accept refusals.
9. **north#1632 box 4, several reviewers.** Claim: the review ceiling goes from about 58/h to about 116/h with two reviewers (G: that each keeps a peak of 35 accepts an hour when they no longer read the same patches). Refuted if two reviewers together peak under 50 accepts an hour.
10. **north#1610, a landed commit costs half the usage.** One-shot review calls and lead wakes held for digest. Claim: commits per weekly-window point go from 19 to about 25 (G: reviewers took 15.8% and lead relays 13.4 points of a day; halving both), lifting the usage ceiling from about 56/h to about 75/h. Refuted if the `Usage after:` profile shows points per landing unchanged.
11. **north#1632 box 11, a worker never waits on its own landing.** Claim: sessions landing exactly one commit go from 248 of 311 to under half, and landed per working session-hour from about 2 to about 3 (G). Refuted if `north work` reads under 2 for three hours with the hand-in returning at once.
12. **north#1632 box 1, the proof run.** Stand-in sessions with no model spend, started by dispatch, for one hour. Claim: it settles the lander's capacity, now a guess of about 120/h, by queue-to-main p90 at 210 hand-ins an hour. Refuted as a proof if the p90 exceeds a minute or any stage drops hand-ins; then that stage is next.
13. **north#1559, no test runs twice on unchanged inputs** (with its original north#1247). Claim: tests run after a documents-only commit go from about 825 to near 0 once 745 of 1,896 tests have read sets, freeing after-landing CPU. Refuted if the after-landing test count for a docs-only commit stays in the hundreds.
14. **north#1583, tests that do not earn their time are removed.** Claim: after-landing verification p50 651 s and p90 2,345 s fall (G: by the share of the culled tests' time). Refuted if the p50 does not fall after the cull lands.
15. **north#1508 with north#1036, builds run on build hosts, placed by measured free capacity.** Claim: cpx62 and cpx62b read free slots while gex45 is full, instead of all three at 0. Refuted if the dry run reads every host 0 with load spread evenly.
16. **north#1612, a break on main is caught, blamed and reverted.** Claim: minutes of red main a day go from 45 (2026-11-16) toward 0 for breaks of the stop class. Refuted by a red interval in `north land train red` records with no revert of its culprit.
17. **north#1631, the lander's stall is flagged.** Claim: a stall like the 25 minutes of 2026-11-15 is flagged within 5 minutes instead of reading healthy. Refuted by any interval of queued tickets with no landing over 5 minutes and no page.
18. **north#1632 box 15 with north#1575 and north#1600, finished work lands.** Claim: `north lane gc --unlanded` lanes with commits ahead of main go from 106 (2026-11-15) to 0: a one-time gain of about 100 commits, not a rate change. Refuted if the count stays above 20 a day after.
19. **north#1632 box 53 with box 22, the Go cap counts callable sessions, parked sessions are released.** Claim: Go-route sessions working go from 0 (21 parked, 23 exited against a cap of 16) to up to 16. It widens slots only if the route lands (box 33): it landed 0 of 22 lanes on 2026-11-15. Refuted if the route fills its slots and lands nothing.
20. **north#1632 box 46 with box 51 and north#1615, disk on the build and dispatch hosts.** Claim: no dispatch freeze for a host's disk floor (15 minutes with 41 ready issues and 11 free slots on 2026-11-16), and gex45's free disk back above its bound after the doctor's remedy. Refuted by a freeze line naming disk in the round line.

## 6. What to stop or park

This ranking says to stop staffing these until the limiting stage moves; acting on it is the coordinator's call.

- Marked now, touching no throughput stage, and features: north#1137, north#1279, north#1281, north#1435, north#1484, north#1487 (59 open boxes), north#1531 (session start in seconds, against hours of session work), north#1544, north#1552, north#1574, north#1577, north#1599. Of these, north#1574 is security, which this note does not weigh against throughput.
- Marked now, touching no throughput stage, but stability fixes, so north#1648 lets them go on: north#1048, north#1427, north#1545, north#1576, north#1595, north#1627, north#1635.
- Features in layers 2 to 4 marked now or next, which north#1648's rule parks until their layer's verdict pays: north#112, north#259, north#641, north#643, north#709, north#1036, north#1041, north#1137, north#1233, north#1279, north#1438, north#1484, north#1531, north#1547, north#1552, north#1574, north#1577, north#1579, north#1584, north#1587, north#1588, north#1589, north#1599, north#1610, north#1626. Two conflict with this ranking: north#1036 (placement by free capacity, rank 15) widens the limiting stage, and north#1610 (rank 10) widens model usage. The recommendation is to class their remaining boxes as stability, since each fixes a measured stall rather than adding a capability.
- Originals still open beside their 24-hour rewrites, so each unit of work shows twice: north#641 and north#1580, north#709 and north#1558, north#722 with north#1571 and north#1539, north#1115 and north#1590, north#1247 and north#1559.
- north#1572 moves the control plane onto a build host, which is the CPU that limits slots; its load there is not measured, and box 39's per-class reading should include it once it lands.

## 7. How this stays current

At the next revision, re-take each reading with its command and replace the mark:

- Landed rate: `git log origin/main --since='24 hours ago' --first-parent --format=%H | wc -l`, and per hour.
- Lander: `north ledger tempo --window 1440` and `--window 60`.
- Refusals: `north land gate-stats --since 48h` (gate refusals and enqueue refusals inside the worker's turn).
- Sessions: `north fleet tree` (working, parked, idle), and the round line in `north work --since 1h`.
- Slots and CPU: `north fleet dispatch --dry-run` on the dispatch host, and box 39's per-class CPU reading.
- Review and after-landing: `north land profile --since 24h --repo north`.
- Usage: `north accounts --json` and north#1610's profile.
- Backlog: `gh issue list -R tompassarelli/north --state open --limit 400 --json number,title,labels,body`, and GraphQL `blockedBy` and `parent` for the edges.

Revise when box 39's dry-run reading shows free slots, since the limit then moves to review and usage and the first twenty change; otherwise weekly. The per-issue table is written by hand; no tool regenerates it.
