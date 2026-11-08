# Scoreboard: north's weekly numbers against published figures

Tom asked on 2026-11-06 for a weekly scoreboard that shows, by measurement rather than by feeling, how north compares with the best published systems (north#657). `north dash --weekly` prints seven numbers for the last 7 days and the 7 days before them, each read from a record north already keeps; this note sets the first reading beside the best published figure for a comparable system and says which comparisons share their estimand closely enough to read as a gap and which are only context (north#641 external review, 2026-11-06).

## What each number reads

- Tom's corrections per active hour: grounding-ledger rows of kind `correction` whose message is one of Tom's typed messages, over the clock hours that hold at least one typed message. Typed messages are the human-origin turns in the convo store plus what Tom types into a fleet-hosted conversation (the fleet TUI proxy), which the transcript records as an sdk prompt: an sdk user turn after the conversation's opening prompt that carries no delivery prefix (`[from `, `[gate `, `[urgent message`) and no origin is Tom's. Before 2026-11-07 only turns in a session opened in his home directory counted, so the proxy conversation read 0.00 corrections per hour while he corrected the fleet daily (north#641).
- Tom's ask to its first landed commit: from a typed message whose turn created or edited an issue (as the `recommend-acts` hook reads the turn from its transcript) or staffed one with `north fleet launch|send` to the first worker-ledger run landing that issue on main. Asks with no landing yet are counted, not timed.
- Landing queue to main: `queued_at` to `landed_at` of each landed lane in the lander's landings ledger.
- Verification after the push: each landing's after-push check spans, as `north land profile` reads them from the land queues' `spans.jsonl`.
- Rework within 48 h: worker landings reverted, fixed forward or named a red-main culprit within 48 h, by the rework judge the tier loop already uses; landings younger than 48 h are shown apart.
- Worker first-try success by category: worker runs with no `Follows:` that reported Done with no later run following them; split, idle and running runs are left out.
- Proxy recommendations filed as an issue the same turn: proxy turns whose final reply recommends or asks, by the rule of the `recommend-acts` Stop hook (ask-once decisions excluded), that created or edited an issue the reply cites; staffing alone does not count, as in the hook.
- Cost per verified landing and Tom minutes per verified landing: the window's API-equivalent run cost, and the minutes of Tom's conversations (first to last message per conversation), each divided by the window's landings that the rework judge cleared after 48 h (landed, not reworked, not unjudged, not still inside 48 h), printed with that count. The same figures `north ledger flow` prints as cost per surviving landing and landed per hour of attention, on the weekly table so each week is set against the last (north#641).

## First reading

`north dash --weekly` at 6:45 AM on 2026-11-06 (laptop time), last 7 days | the 7 days before:

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

- Rework is the judge in `crates/north-calibration/src/threads/calibration/rework.rs`. A change to its rules bumps its `JUDGE_VERSION`, which judges every stored verdict again, and the weekly record measures both weeks on the day it records them, so the two weeks always share a rule. The sample of 20 below measured the named-issue rule's false positives; the blind-labelled sample of 64 landings in `rework_labels.json` measures its precision and recall together (north#641, north#711), and the ready bar's 5% and the climber's kept boxes both read this judge.
- Worker first-try success counts a worker's own Done report, and the ledger's closed box is an issue closed on GitHub, whose boxes its workers tick. `north task check --done-when` is the one tick no worker writes; a box on north#636 makes the climber's reward split each kept box into checker-verified and self-ticked, so a candidate that rewrites briefs cannot raise its own reward.

## The named-issue rule (north#657)

On 2026-11-06 the named-issue rule made 171 of north's 185 reworked landings over 7 days (37% of 494 judged). The rule called a landing fixed forward when any commit within 48 h named the landing's issue and said "fixes", "fix forward" or "Follows:" anywhere in its message. North's issues close box by box, so the next box of the same issue names it too, and "fixes" matched as a substring: "prefixes", "red fixes", "a round of fixes" from the trigger's own review.

A sample of 20 of the 171, drawn with `shuf --random-source=<(yes 657) -n 20` over the named-issue lines of the 7-day output, read commit against commit, with the trigger's changed lines blamed at its parent to see whether it touched lines the landing wrote:

- a0b83f6f8 (fence native leases, north#259) by b3d1886b6 (crate split box 4-12, north#259): not rework; no shared file, its "Fixes after the verb" are its own moved paths.
- f4771806c (fleet store, north#429 box 1a) by 930ad18df (fleet tick, the next box): not rework; of 9 shared files it changes 4 of the landing's lines, a usage string and imports it extends.
- 6aec1fadd (lane gc moves idle lanes, north#523) by 53cdfd58a (refuse the proxy implementation work, north#523): not rework; no shared file.
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

`north dash --rework --since 7d` for north, before the change (at 9:22 AM on 2026-11-06) and after it (at 10:16 AM the same day, so the window moved by about an hour):

- all landings: 185 of 494 reworked (37%) before, 17 of 495 (3%) after
- by rule: named issue 171 before, 2 after; named commit 8 before, 9 after; Fix subject with file overlap 6 before, 6 after; revert and red-main culprit 0 both times
- control-plane code: 99 of 325 before, 10 of 324 after
- gates, guards and hooks: 56 of 92 before, 6 of 93 after
- docs: 20 of 58 before, 1 of 59 after
- tests: 10 of 15 before, 0 of 15 after
- tables: 0 of 4 both times

The two named-issue reworks left are both real: 020e57e22 restores a test that 1355312d0 broke, and 6ba11bbb9 restores two client calls that 62b04d092 deleted while they still had callers. The other repositories' counts did not move (makgora 49, wisp 15, firn and south 0).

## The Fix-subject rule (north#711)

The blind-labelled sample (north#641) scored the judge at precision 33% and recall 8%, each landing weighted by its stratum: 24 landings the judge called reworked, all drawn, and 40 of the 405 it called clean. Of those 40, the readers labelled 9 reworked. Two have no later commit touching their files, and one only a port that moved its docs. Five have a later commit within 48 h whose subject begins with Fix and which changes a file the landing changed, though fewer than half of its files, which the Fix-subject rule then required: 3f43bd985 fixes red main in bin/safe-push after e9aaffcdf, b087fc820 fixes safe-push's detached-HEAD exit after 58df15b56 and a717bf640, 06be079f1 changes src/cli/mod.rs after 331f32b7e, and 975405806 changes docs/features.md after afae5689a.

The rule now calls a landing fixed forward when a later commit's subject begins with Fix and it changes any file the landing changed. Among the 40 drawn as clean it fires on those 5 and on 2 the readers labelled clean (6ce8eea71 and e4b94f160, both through docs/features.md), and it adds nothing among the 24, which the judge already called reworked. `cargo test -p north-threads rework_labels` now reads precision 62% and recall 59%, and its floors are 33% and 50%. The 16 of the 24 the readers labelled clean, among them landings named by the stale-base restore 0908757e0, still count against precision.

## Weekly run

The fleet tick runs `north dash --record` when the last recording is a week old, telling the figures to north's graph (writer `scoreboard`) and writing a one-line score under the state root; the `scoreboard` loop in `harness/loops.toml` declares it, so `north loops check` fails when its newest tick in the graph is older than two weeks. Each recording compares every number's headline figure (the rate, the median, the share) with the week before. A number counts as worse when both weeks hold at least five observations and it moved more than 10% in its worse direction (up for corrections, waits and rework, down for first-try success and acted recommendations). The proxy hears the worse numbers once per recording, in the fleet tick; the margin and the sample floor are guesses to be tuned once a few weeks of recordings show how much a number moves without cause. `north dash rework` splits the rework number by the class of change and by what the rework fixed, so the fixes can aim at the class that reworks most.

Two `product share` rows print the share of API-equivalent worker spend and of landed worker runs that went to a product repository, this week against the week before, with each repository's count and `bar 70%` beside it (north#871). A product is every repository except north, south and firn (the harness, its private policy and the machines it runs on), so a new game repository counts with no edit; a run weighs by the repositories its item names. The 70% comes from Gemini's moats review (2026-11-08). It is a printed measurement: the exit code is unchanged and nothing refuses or reorders work on it, since Tom chose on 2026-11-07 to keep building north features (north#719). The rows join the weekly record, so the tick tells the proxy when either falls by more than 10%.

Below the numbers, `north dash --weekly` lists the harness lanes of the last 7 days (north#706): how many worker landings reached north's main, how many name an `Expected effect:` in their commit message, and each lane that caused an incident within 48 h, by the rework judge (reverted, fixed forward or a red-main culprit, with the triggering commit) and by incident rows whose subject or detail names its sha. A lane that caused nothing is only counted, so the list stays the size of the week's harness regressions.

## The ready bar (north#641)

Tom asked on 2026-11-06 for one point at which he goes back to game work and the loops keep the harness up without active management. `north dash --bars` prints one table per repository whose project is not paused in orchestration.toml (every repository with `--all`), so it fits a 120-column terminal: a first line with the verdict, how many bars fail on the last 1 h, then the ready count (days in a row on which every bar passed on its 24 h window, of the 3 needed), then a row per bar with its target, its pass or FAIL mark on the last 1 h, and the last 10 min, 1 h, 3 h, 6 h and 24 h. `--explain` adds each cell's sample and the sources (Tom found the single wide table wrapping on 2026-11-07, north#697). Only the 1 h window passes or fails against the target, so a bar answers for the newest hours and a recovery shows within the afternoon; the 10 min, 3 h, 6 h and 24 h cells are context. Tom set the 24 h verdict on 2026-11-07 after 7-day cells hid a day of recovery, then moved it to 3 h the same day and dropped the 12 h and 7 d cells, since he judges the day'"'"'s work on hours, and on 2026-11-08 moved it to 1 h so a recovery shows within the hour, keeping the 3 h cell as context; the weekly numbers keep their 7 days under `--weekly`. The thresholds are the `ready` class of north's goals.toml (`ready-landing-p90` and its eight siblings); a ready goal has no oracle because the scoreboard reads it, and `north check goals` skips the class.

- Landing queue to main p90 at most 60 s, verification after the push p50 at most 60 s and p90 at most 5 min, rework within 48 h at most 5%, worker first-try success at least 70% (a read-only reviewer or test-keep judge is no worker, a report that names `Split:` in its opener paragraph is a split, and a failed box that waits on a dated check or stated live traffic is left out unless the check recorded on its issue missed), and Tom's ask to first landed commit median at most 1 h are the weekly scoreboard's figures split by repository: a landing by its ledger repo, verification by the land queue that serves the repo, a worker run and an ask by the repo its item or issue names.
- Tom's corrections per active hour at most 0.05 and the longest account idle while priority:now work waits at most 30 min are fleet-wide, so every table shows the same figure. Idle minutes are runs of consecutive pace-control.jsonl samples of a Claude account that is not held or over, has a worker allowance and runs no worker, and a gap over 10 minutes between samples ends a run.
- Landing to live on every host p90 at most 120 s (north#731) shows on north only. The fleet settler probes each host every runtime tick, reads when its `current` link was last swapped, and appends one row per landing and host to `~/.local/state/north/runtime-live.jsonl` the first time that host runs a runtime containing it; a landing counts once every host active in the last day has a row, at the latest host's time. A window with no landing has nothing to promote and passes.
- priority:now open issues at most 5 reads the GitHub mirror. GitHub keeps no past count, so every `north dash` run and fleet tick appends the counts to `north/scoreboard/ready-priority.jsonl` (at most every half hour); a day with no record fails its bars instead of passing silently.
- Days in a row walks back from now over 24 h windows, one per day up to 7, and stops at the first window in which a bar fails. A window with nothing to observe fails the bar except for the two that depend on Tom being at work (his corrections and his asks).

The proxy moves the fleet to game work when both repositories show three days in a row, and north then gets only the boxes a regressing bar files itself.

## Usage per landing and stranded usage (north#793)

Tom asked on 2026-11-07 for a second standing goal beside harness loss, the efficiency half of the project's focus: how much model usage the fleet spends per landed first try. Both bars read mechanical sources only (the ledger's usage readouts, runs and landings, the api spend and the credit grants), never a report's text (north#791).

- `ready-usage-per-landing` is points per worker first try that landed over the last 1 h, where a point is one percent of an account's weekly window or one api list-price dollar. The subscription part sums each Claude account's weekly window rise from the `usage` readouts: a window is keyed by its account and its reset to the hour, and its spend over a window of time is the highest readout by the end minus the highest by the start, so two usage sources that read the same window a point apart never count a drop and a rise twice. The api part is the list-price dollars of the runs on `api:` accounts that started in the window, every host's report counted once. A first try that landed is a worker run with no `Follows:` that no later run follows and whose outcome the bars count as done, which is the landing on origin/main. The bar has no record in a window with no landed first try.
- `ready-stranded-usage` is points that reset or expired unspent while ready work waited: the percent of a weekly window left at its reset, the credit a grant still held on the day it expired, and the credit left on every unexpired grant of an organization no provider refusal for credit has closed (held back; a refusal is the mechanical truth that the organization is spent, whatever the counted spend left of it, north#792), which counts in windows that end now. It counts only where `north/scoreboard/ready-priority.jsonl` noted a `priority:now` issue open inside the window.
- `north ledger flow` prints both over the last 24 h on one line after the harness losses, with each part named so the proxy sees which source stranded the usage. Account sizes are not normalised: a point is one percent of that account's own window.
