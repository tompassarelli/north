# Scoreboard: north's weekly numbers against published figures

Tom asked on 2026-11-06 for a weekly scoreboard that shows, by measurement rather than by feeling, how north compares with the best published systems (north#657). `north scoreboard` prints seven numbers for the last 7 days and the 7 days before them, each read from a record north already keeps; this note sets the first reading beside the best published figure for a comparable system and says which comparisons share their estimand closely enough to read as a gap and which are only context (north#641 external review, 2026-11-06).

## What each number reads

- Tom's corrections per active hour: grounding-ledger rows of kind `correction` whose message is one of Tom's typed messages, over the clock hours that hold at least one typed message. Typed messages are the human turns of the proxy conversations in the convo store, as `north flow`'s outcome lines read them.
- Tom's ask to its first landed commit: from a typed message whose turn created or edited an issue (as the `recommend-acts` hook reads the turn from its transcript) or staffed one with `north fleet launch|send` to the first worker-ledger run landing that issue on main. Asks with no landing yet are counted, not timed.
- Landing queue to main: `queued_at` to `landed_at` of each landed lane in the lander's landings ledger.
- Verification after the push: each landing's after-push check spans, as `north land profile` reads them from the land queues' `spans.jsonl`.
- Rework within 48 h: worker landings reverted, fixed forward or named a red-main culprit within 48 h, by the rework judge the tier loop already uses; landings younger than 48 h are shown apart.
- Worker first-try success by category: worker runs with no `Follows:` that reported Done with no later run following them; split, idle and running runs are left out.
- Proxy recommendations filed as an issue the same turn: proxy turns whose final reply recommends or asks, by the rule of the `recommend-acts` Stop hook (ask-once decisions excluded), that created or edited an issue the reply cites; staffing alone does not count, as in the hook.

## First reading

`north scoreboard` at 6:45 AM on 2026-11-06 (laptop time), last 7 days | the 7 days before:

```
- Tom's corrections per active hour: 0.14 (5 in 37 active h) | 0.03 (3 in 108 active h)
- Tom's ask to its first landed commit: median 2.6 h, p90 16.8 h (n=19, 8 not landed yet) | median 56 min, p90 13.6 h (n=69, 129 not landed yet)
- landing queue to main: median 5 min, p90 110 min (n=935) | no record
- verification after the push: median 10 min, p90 54 min (n=99) | no record
- rework within 48 h per landing: 19% (234/1210, 270 still inside 48 h) | 9% (63/734)
- worker first-try success, all categories: 55% (872/1596) | 49% (626/1290)
- proxy recommendations filed as an issue the same turn: 4% (24/537) | 7% (21/323)
```

The landings ledger starts on 2026-11-04 and the after-push spans on 2026-11-06, so the earlier week has no record for those two and the last week covers only those days.

## Against published figures

- Corrections per active hour, 0.14: no comparable figure. The nearest is Anthropic's count of human turns per Claude Code transcript, 6.2 in February 2025 and 4.1 in August 2025 over 200,000 internal transcripts ([anthropic-work](../references.md#anthropic-work)); it counts every human turn per task, not corrections per hour, so the two do not compare.
- Ask to first landed commit, median 2.6 h and p90 16.8 h: context only. Google's median for the review step alone is under 4 hours over about 9 million changes ([google-code-review](../references.md#google-code-review)), and DORA's 2024 elite cluster takes under 24 hours from commit to production ([dora-2024](../references.md#dora-2024)); north's figure starts earlier, at the ask, and includes writing the change, so neither figure ranks it. The earlier week's 129 unlanded asks show the median leaves out the work that never landed.
- Landing queue to main, median 5 min and p90 110 min: north leads at the median and trails in the tail. Google's TAP reports an average submit wait of about 11 minutes ([google-tap](../references.md#google-tap)); Uber's SubmitQueue reports P95 waits of 9.9 to 18.6 minutes per monorepo after its 2025 change ([uber-submitqueue](../references.md#uber-submitqueue)), where north's p90 alone is 110 minutes. GitHub reports a 33% cut in average wait with merge queue but no absolute time ([github-merge-queue](../references.md#github-merge-queue)).
- Verification after the push, median 10 min and p90 54 min: no comparable figure. None of these sources publishes a post-submit verification time.
- Rework within 48 h, 19%: context only. DORA's 2024 elite cluster has a 5% change failure rate and its high cluster 20% ([dora-2024](../references.md#dora-2024)), and Google reports that a change passing presubmit passes the rest of its tests 95% or more of the time ([google-tap](../references.md#google-tap)). North's unit is a landing on main, not a deployment, and its failure is a later revert or fix that its own judge finds, not an incident in production. This reading also predates the change to the named-issue rule below, which moved north's 7-day rework from 37% to 3% over nearly the same landings.
- Worker first-try success, 55%: no comparable figure. Benchmarks score a model and scaffold on curated tasks whose tests are given; METR's time-horizon study puts the frontier's 50% success at tasks of about 50 minutes for Claude 3.7 Sonnet as of March 2025 ([metr-horizon](../references.md#metr-horizon)), which measures task length, not a fleet's first-try rate. No current SWE-bench leaderboard figure could be read from its primary page on 2026-11-06, so none is quoted.
- Proxy recommendations filed as an issue the same turn, 4%: no comparable figure. The `recommend-acts` Stop hook that blocks an unacted recommendation landed at 5:25 AM on 2026-11-06 (north#641), after most of this window.

## Where north stands

Each number describes north's own records, a week against the week before under one rule; none ranks north against another system. Only the landing queue measures nearly the quantity a published figure does, a change's wait from queue to merge, and there north's median of 5 min is under TAP's average of about 11 while its p90 of 110 min is several times SubmitQueue's P95. The other published figures count a different span, unit or failure, so they serve as context for setting a threshold, as the ready bar's are. A claim that north beats a baseline needs the same estimand, unit and population on both sides, and a change's effect is judged by an experiment's arms (docs/rationale/experiments.toml), not by the scoreboard.

## What the numbers rest on

- Rework is the judge in `crates/north-threads/src/threads/calibration/rework.rs`. A change to its rules bumps its `JUDGE_VERSION`, which judges every stored verdict again, and the weekly record measures both weeks on the day it records them, so the two weeks always share a rule. The sample of 20 below measured the named-issue rule's false positives only; how many real reworks the current judge misses is unmeasured until a blind-labelled sample scores it (north#641), and the ready bar's 5% and the climber's kept boxes both read this judge.
- Worker first-try success counts a worker's own Done report, and the ledger's closed box is an issue closed on GitHub, whose boxes its workers tick. `north task check --done-when` is the one tick no worker writes; a box on north#636 makes the climber's reward split each kept box into checker-verified and self-ticked, so a candidate that rewrites briefs cannot raise its own reward.

## The named-issue rule (north#657)

On 2026-11-06 the named-issue rule made 171 of north's 185 reworked landings over 7 days (37% of 494 judged). The rule called a landing fixed forward when any commit within 48 h named the landing's issue and said "fixes", "fix forward" or "Follows:" anywhere in its message. North's issues close box by box, so the next box of the same issue names it too, and "fixes" matched as a substring: "prefixes", "red fixes", "a round of fixes" from the trigger's own review.

A sample of 20 of the 171, drawn with `shuf --random-source=<(yes 657) -n 20` over the named-issue lines of the 7-day output, read commit against commit, with the trigger's changed lines blamed at its parent to see whether it touched lines the landing wrote:

- a0b83f6f8 (fence native leases, north#259) by b3d1886b6 (crate split box 4-12, north#259): not rework; no shared file, its "Fixes after the verb" are its own moved paths.
- f4771806c (fleet store, north#429 box 1a) by 930ad18df (fleet tick, the next box): not rework; of 9 shared files it changes 4 of the landing's lines, a usage string and imports it extends.
- 6aec1fadd (lane-gc moves idle lanes, north#523) by 53cdfd58a (refuse the proxy implementation work, north#523): not rework; no shared file.
- 7151960a3 (skip the install build in land checks, north#360) by d380ce323 (workers queue on infrastructure refusals, north#360): not rework; no shared file.
- 975af79c4 (prompt-grounding recall, north#523) by 53cdfd58a: not rework; no shared file.
- 8424e2772 (fleet waits in the store, north#429 box 3a) by eaeb7a955 (fleet invariant in the tick, box 3b): not rework; 4 of the landing's lines changed are re-exports and a signature the next box widens, and "a round of fixes" is the trigger's own review.
- 2be8a0bf2 (router admission tests, north#88) by cea43e2f0 (threads verbs reland, north#88 slice 1): not rework; no shared file.
- 3f1d25c48 (cloud placement by headroom) by d380ce323: not rework; no shared file.
- 315d1823f (capacity watchdog port, north#88) by cea43e2f0: not rework; no shared file.
- 7f596e9ca (register launches in the fleet store, north#429 box 4b) by 6d7daa7aa (box 4c): not rework; its 62 blamed lines are the relay and outbox that box 4c was planned to delete, and its three named fixes are to box 3b.
- 3f80b3356 (split north-core box 2) by d0ba66afa (lander fast lane, north#360): not rework; no shared file, "red fixes" matched.
- c7d367180 (one integration test binary) by 055a4a24a (conflict-free landing plan, one new doc): not rework; no shared file.
- 7fb890020 (land cloud handbacks) by d0ba66afa: not rework; no shared file.
- 7833ab399 (sheriff re-verifies before reverting, north#360) by d380ce323: not rework; no shared file.
- 6e4143c79 (gh-app-token switch, north#88) by cea43e2f0: not rework; no shared file.
- f9f1cd052 (pull remote outboxes per candidate, north#360) by 055a4a24a: not rework; no shared file.
- 230a3c190 (goal-eta fixture move, north#88) by cea43e2f0: not rework; one shared file, none of the landing's lines changed.
- 9b7022a77 (delete unreferenced bin files, north#88) by cea43e2f0: not rework; no shared file.
- 214dd7074 (CI watchers port, north#88) by cea43e2f0: not rework; one shared file, none of the landing's lines changed.
- b24fb3367 (land check deadlines, north#360) by c879ceb5e (parallel culprit search, north#360): not rework; one shared file, none of the landing's lines changed, "fixes" matched inside "prefixes".

None of the 20 is true rework. Every trigger was progress on the same issue, often a later box, or unrelated work by a run whose item was the same umbrella issue; 14 share no file with the landing, and the 3 that change the landing's lines extend them rather than repair them.

The rule now asks three things of the trigger besides naming the issue: its subject says it repairs something (it begins Fix, Repair, Restore or Correct, or says fix forward), it changes a file the landing changed, and, as `git blame` reads the lines it removes or rewrites at its first parent, at least one of those lines is the landing's. The fix words of the named-commit rule match as words, so "prefixes" no longer counts; the Fix-subject rule is unchanged. A verdict stored by the earlier judge carries no judge version and is judged again on the next read.

`north scoreboard --rework --since 7d` for north, before the change (at 9:22 AM on 2026-11-06) and after it (at 10:16 AM the same day, so the window moved by about an hour):

- all landings: 185 of 494 reworked (37%) before, 17 of 495 (3%) after
- by rule: named issue 171 before, 2 after; named commit 8 before, 9 after; Fix subject with file overlap 6 before, 6 after; revert and red-main culprit 0 both times
- control-plane code: 99 of 325 before, 10 of 324 after
- gates, guards and hooks: 56 of 92 before, 6 of 93 after
- docs: 20 of 58 before, 1 of 59 after
- tests: 10 of 15 before, 0 of 15 after
- tables: 0 of 4 both times

The two named-issue reworks left are both real: 020e57e22 restores a test that 1355312d0 broke, and 6ba11bbb9 restores two client calls that 62b04d092 deleted while they still had callers. The other repositories' counts did not move (makgora 49, wisp 15, firn and south 0).

## Weekly run

The fleet tick runs `north scoreboard --record` when the last recording is a week old, writing `north/scoreboard/week.json` and a one-line score under the state root; the `scoreboard` loop in `harness/loops.toml` declares it, so `north loops check` fails when the record is older than two weeks. Each recording compares every number's headline figure (the rate, the median, the share) with the week before. A number counts as worse when both weeks hold at least five observations and it moved more than 10% in its worse direction (up for corrections, waits and rework, down for first-try success and acted recommendations). The proxy hears the worse numbers once per recording, in the fleet tick; the margin and the sample floor are guesses to be tuned once a few weeks of recordings show how much a number moves without cause. `north scoreboard rework` splits the rework number by the class of change and by what the rework fixed, so the fixes can aim at the class that reworks most.

## The ready bar (north#641)

Tom asked on 2026-11-06 for one point at which he goes back to game work and the loops keep the harness up without active management. `north scoreboard --ready` prints one table, a row per bar and a column per repository (north and makgora), each cell holding the 7-day value, its sample and pass or fail against the threshold in the first column, with the days in a row on which every bar passed at the bottom. The thresholds are the `ready` class of north's goals.toml (`ready-landing-p90` and its eight siblings); a ready goal has no oracle because the scoreboard reads it, and `north goals check` skips the class.

- Landing queue to main p90 at most 60 s, verification after the push p50 at most 60 s and p90 at most 5 min, rework within 48 h at most 5%, worker first-try success at least 70%, and Tom's ask to first landed commit median at most 1 h are the weekly scoreboard's figures split by repository: a landing by its ledger repo, verification by the land queue that serves the repo, a worker run and an ask by the repo its item or issue names.
- Tom's corrections per active hour at most 0.05 and the longest account idle while priority:now work waits at most 30 min are fleet-wide, so both columns show the same figure. Idle minutes are runs of consecutive pace-control.jsonl samples of a Claude account that is not held or over, has a worker allowance and runs no worker, and a gap over 10 minutes between samples ends a run.
- priority:now open issues at most 5 reads the GitHub mirror. GitHub keeps no past count, so every `--ready` run and fleet tick appends the counts to `north/scoreboard/ready-priority.jsonl` (at most every half hour); a day with no record fails its bars instead of passing silently.
- Days in a row walks back from now over 24 h windows, one per day up to 7, and stops at the first window in which a bar fails. A window with nothing to observe fails the bar except for the two that depend on Tom being at work (his corrections and his asks).

The proxy moves the fleet to game work when both repositories show three days in a row, and north then gets only the boxes a regressing bar files itself.
