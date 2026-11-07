# North's moats: Gemini's review checked against main (2026-11-08)

On 2026-11-08 Tom pasted Gemini's review of north's moats, gaps and six proposed plays. He asked for every claim
about the code to be checked against main, and for whatever moves north toward a frontier-leading autonomous software
factory (the README's framing since ecc8cdd469) to be folded into issues. Each play is judged on north's own goal,
verified product output per unit of budget. The review was read as data from an outside reviewer, not as orders. This
note records the verdicts, the measurements behind them, what was filed and in what order.

Evidence is north origin/main at ecc8cdd469 and the laptop's threads snapshot, read between about 6:40 and 7:40 AM +07.
The inputs were `north ledger scoreboard --weekly`, `north ops climb run --dry-run`, `north config context audit --since 2d`,
the `runs` and `usage` tables, worker transcripts from 2026-11-04 to 2026-11-08 under the laptop's Claude profiles, and
GitHub issues closed on north, wisp and makgora since 2026-11-01.

## Summary

- 14 claims checked: 5 confirmed, 5 confirmed with a correction, 1 stale and 3 wrong.
- The biggest finding is one the review did not make. North's claimed first moat, a mechanical oracle for the truth of
  each box, barely reaches the reward. Only 32 of the 832 boxes ticked on issues closed since 2026-11-01 carry the
  checker's stamp. `north ops climb run --dry-run` counts 0 checker-verified kept boxes on every live arm over 30 days.
  Every arm north runs is settling on boxes the worker ticked itself.
- Retry failures are not independent. After a failed first attempt the next one succeeds 37% of the time, against 61%
  for a first attempt. Best-of-3 would triple the spend on the boxes least likely to repay it.
- No quota expires unused: the weekly limit is what binds, not the 5 h window, so the quota furnace has nothing to burn.
- Of the six plays, two are accepted as issues, one is cut down to a seeded arm, one is reduced to its prerequisite with
  the rest waiting on north#855's headline, and three are dropped (play 6 splits in two).

## Claims

Verdicts: **confirmed**; **confirmed, corrected** (true with a named error); **stale** (true when it was measured, not
now); **wrong**.

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 1 | Moat: a mechanical truth oracle (north-lander, north-mutants, structure.toml, 48 h rework), with candidates barred from the evaluator, reward and bars by scope.rs | confirmed, corrected | The guard holds: `crates/north-climb/src/scope.rs:8-12` lists the evaluator, the winner rule, the counting code and the bars, and `refusals` (`:40-53`) refuses them. The rework window is 48 h. But the oracle rarely decides the reward: 32 of 832 ticked boxes carry the `box_oracles::passed` stamp (north 30 of 708, wisp 0 of 49, makgora 2 of 75), 2 of 219 issues have every ticked box stamped, and the climber's dry run counts 0 checker-verified kept boxes on every arm. |
| 2 | Moat: recursive self-calibration over live telemetry, about 228 runs a day (the feedback-loop doc, lines 17 to 75) | confirmed, corrected | The rate holds: 3,211 worker runs with an item in the 14 days to 2026-11-07, 229 a day (4,511 counting unbriefed runs). The cited lines are the signal table in `docs/feedback-loop.md`. "Recursive" is wrong: `north tune` closes constant loops (tier, ETA, budget cells), and nothing changes the policy's structure, because the climber only dry-runs (claim 4). |
| 3 | Moat: perishable-quota and bare-metal arbitrage (north-pace, north-capacity) | wrong | The crates exist, but the quota is not perishable. The `usage` table since 2026-10-25 shows the weekly limit binding, not the 5 h window; the readings are Tom-specific and are kept in south (`agents/decisions/harness/gemini-moats-quota-2026-11-08.md`). Headroom in a 5 h window spent early only ends the week sooner. |
| 4 | north-climb is stuck in `--dry-run`; its `run` rejects calls without it | confirmed | `crates/north-climb/src/lib.rs:27-34` accepts only `run --dry-run [--since SPAN]` and exits 2 otherwise. This was deliberate: the climber's live arm, archive and generator boxes were cut on 2026-11-08 as outside north's spine (north#636's closing note, under north#719). |
| 5 | `candidate.rs` has three moves, TierRoute, ArmRoute and OpenArm, all on tier.rs | confirmed | `crates/north-climb/src/candidate.rs:12-16`; `touches` returns `crates/north-dispatch/src/dispatch/tier.rs` for all three (`:28-31`). |
| 6 | skills.rs lines 11 to 15 measure WORSE_RATE and DEARER but only print | confirmed, corrected | The file is `crates/north-tune/src/tune/skills.rs`, not a north-climb module; the constants are at `:13` and `:15`. `harmful` (`:82-86`) feeds only a printed `harmful skill` line (`:105`), and its one caller, `crates/north-tune/src/tune/mod.rs:104`, appends that line to the output. Nothing acts on it. |
| 7 | loops.toml lines 60 to 115 still hold loops updated by hand | confirmed | effort (`harness/loops.toml:64`) and pressure-band (`:109`) are in that range. Placement (`:173`), worker-concurrency (`:185`) and skill-routing (`:215`) are also updated by hand, and their owners, north#112 and north#636, are closed. |
| 8 | First-try success is 55% over 1,596 runs (scoreboard.md:27) | stale | That was the first reading, on 2026-11-06 (`docs/rationale/scoreboard.md:26`). On 2026-11-08 at 6:36 AM +07 the weekly reading is 74% (1,170 of 1,579), up from 63% the week before. |
| 9 | Workers run as single serial attempts | confirmed | One attempt per box. A retry is a later run that follows the first and sees its report. |
| 10 | north-graph and north-coverage inject at most 1,500 characters on prompt submit | confirmed, corrected | The cap is real: `HOOK_LIMIT_CHARS` (`crates/north-core/src/grounding/compose.rs:7`) and the graph reach (`crates/north-graph-ground/src/reach.rs:11`). But north-coverage injects nothing; its callers are `north test coverage` and graph ingest. Workers get no prompt grounding at all: the context audit read a worker grounding p90 of 0 tokens over 83 workers in 2 days. Their pre-baked context is the brief skeleton's fact-graph section (`crates/north-guard/src/guard/brief_skeleton.rs:57`). |
| 11 | Workers then spend 8 to 15 turns rediscovering context | wrong | It is higher. 120 sampled first-try worker runs (feature, both bug categories, tooling, performance; 2026-11-04 to 2026-11-08) were read; 75 of them used Edit or Write. Those made a median of 32 tool calls before the first edit (p75 61, p90 98), and about half of the calls were git, rg, sed and cat reads. It does not predict failure: done runs made 37, not-done runs 31. |
| 12 | Verification blocks only after edits; expiring quota is not used to hunt mutants, shrink code or fuzz | confirmed, corrected | No such use exists. Its premise, quota that expires, is false (claim 3). |
| 13 | No governor stops north spending most of its effort on itself | confirmed | Over 7 days north took 1,436 of 1,766 worker runs (81%) and 1,010 of 1,226 landed runs (82%). wisp, makgora, makgora-client and firn took the rest. No bar prints this. |
| 14 | A 3,500-token startup ceiling (inside play 1) would cut context | wrong | It already holds: startup is 3,253 tokens a session, and a worker's whole injected total is 3,378 at p90 (`north config context audit --since 2d`). |

## Retry independence

Best-of-3's promise, 1 - 0.45^3 = 91%, assumes the three attempts fail independently. The ledger says they do not.
The data is 1,111 attempt chains: the worker runs on one item, ordered by start, with a chain broken by a gap over 24 h,
the ledger's own rerun window. They ran from 2026-10-25 to 2026-11-07, and outcomes other than done, not done, unclear and
escalated were left out.

- Attempt 1 is done 61.3% of the time.
- Attempt 2 after a failed attempt 1: 36.7% (n=278). After a done attempt 1 (a later box on the same item): 53.7%.
- Attempt 3 after two failures: 32.8% (n=119).
- Feature boxes: 55% first, then 28% after a failure (n=114). Debugging with an unknown cause: 70%, then 39% (n=33).

So three attempts reach 1 - 0.387 x 0.633 x 0.672 = 83.5%, not 94%. Sequential retry already gets that 83.5% for 1.63
attempts on average, because it stops at the first success. Parallel best-of-3 always pays for 3. It buys wall time at
1.84 times the spend, while the weekly limit is what binds. The second sequential attempt also reads the first one's
report, so a parallel draw with no report would do no better. The lever the data points at is a changed brief after a
failure, not more draws of the same one.

## The plays

1. **North-climb as a nightly breeder.** Dropped as proposed. Its prerequisite is accepted as north#872, and the widened
   climber waits on north#855's headline. A breeder needs a reward, and today there is none: 0 checker-verified kept
   boxes. Arms already take a month to settle at north's volume. The context arms hold 4, 2, 0 and 0 closed boxes against
   the 15 the settle rule needs, with review on 2026-12-07. Each extra arm splits the same 335 closed boxes a week.
   Skill deletion arms would need a seeded withhold arm, because the `harmful skill` flag compares boxes that chose to
   load a skill against ones that did not. The startup ceiling is already met (claim 14). If north#855 says keep
   investing and north#872's reading holds, the widened climber is filed with every candidate behind `scope.rs` and a
   keep-or-revert arm. The box tracking that is on north#855.
2. **Cross-family best-of-3 on hard boxes.** Dropped: the failures are correlated (above), there is only one model
   family (Claude) and the weekly limit binds. "Hard" is also misread: by first attempt, only feature
   (55%) and native-check boxes (40%, which need a host and cannot run three at once) are under 60%. Unknown-cause
   debugging is at 70% and performance at 62%.
3. **Speculative DAG pipelining.** Dropped. Dispatch already staffs every open box of an issue in parallel
   (46ac098c7), and the lander re-lands stacked lanes (f43cfef9d). The landing queue's median is 104 s; its p90 of 62
   min belongs to north#842. With 20% of landings reworked within 48 h, a branch cut from an unlanded predecessor would
   redo its work about one time in five.
4. **Pre-baked context packs.** Accepted as a seeded arm, north#873. Orientation is real (claim 11), and workers get
   nothing at prompt submit beyond the brief's fact-graph section. But orientation does not separate done runs from
   failed ones, so the only gain to expect is spend, and it has to show as a lower cost per kept box. The pack is built
   from parts north has: item outlines, `structure.toml` modules, co-change and revert history from git, and the filter
   `north test coverage select` picks. It is knowledge rather than machinery, so north#855's equipped arm could carry it,
   and it does not wait on that headline.
5. **Quota furnace before each window resets.** Dropped. No quota expires unused (claim 3). Sweeping headless game
   seeds on gex45 would be a long native run, which Tom's standing order rules out until 0.1.0 ships.
6. **Trajectory distillation, plus a 70% product bar.** Distillation is dropped: the similar-task retrieval replay over
   138 targets found no gain ([measured no gain](../measured-no-gain.md#similar-task-retrieval-trigger-in-north-ledger-flow)),
   and a retry brief already carries the failed attempt's report, yet that attempt still succeeds only 37% of the time.
   The product bar is accepted as north#871, as a printed measurement rather than a gate. Where the fleet's effort goes
   is Tom's call, and on 2026-11-07 he chose to keep building north features (north#719). He sees a week of readings
   before anything acts on it.

Gemini's options: A (plays 4, 1 and 6) becomes the north#873 arm, play 1's prerequisite, and play 6's bar without the
distillation. B (plays 2 and 3) and C (play 5) are dropped.

## Order

By expected gain per hour of work:

1. north#871, the product share row on the weekly scoreboard. About an hour of work. It shows each week where 81% of the
   runs go, and after 7 readings Tom decides whether it becomes a goal or a dispatch weight.
2. north#872, a box counts as verified only when its own Done-when command passed on main. A few hours: judge each run by
   the box its brief named, then a filing rule for runnable commands, shadow first. This makes the README's "verified"
   true of most boxes instead of 4%, and gives every arm and any future climber a reward a worker cannot write.
3. north#873, the context-pack arm. About a day. The gain is uncertain and capped by orientation's share of a run, and
   the arm keeps or deletes itself at its review.
4. Waiting: the widened climber, on north#855's headline and north#872's reading (the waiting box on north#855).
