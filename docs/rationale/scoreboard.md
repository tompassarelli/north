# Scoreboard: north's weekly numbers against published figures

Tom asked on 2026-11-06 for a weekly scoreboard that shows, by measurement rather than by feeling, how north compares with the best published systems (north#657). `north scoreboard` prints seven numbers for the last 7 days and the 7 days before them, each read from a record north already keeps; this note sets the first reading beside the best published figure for a comparable system and says where north leads, trails or has nothing to compare with.

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
- Ask to first landed commit, median 2.6 h and p90 16.8 h: north leads. Google's median for the review step alone is under 4 hours over about 9 million changes ([google-code-review](../references.md#google-code-review)), and DORA's 2024 elite cluster takes under 24 hours from commit to production ([dora-2024](../references.md#dora-2024)); north's figure starts earlier, at the ask, and includes writing the change. The earlier week's 129 unlanded asks show the median leaves out the work that never landed.
- Landing queue to main, median 5 min and p90 110 min: north leads at the median and trails in the tail. Google's TAP reports an average submit wait of about 11 minutes ([google-tap](../references.md#google-tap)); Uber's SubmitQueue reports P95 waits of 9.9 to 18.6 minutes per monorepo after its 2025 change ([uber-submitqueue](../references.md#uber-submitqueue)), where north's p90 alone is 110 minutes. GitHub reports a 33% cut in average wait with merge queue but no absolute time ([github-merge-queue](../references.md#github-merge-queue)).
- Verification after the push, median 10 min and p90 54 min: no comparable figure. None of these sources publishes a post-submit verification time.
- Rework within 48 h, 19%: north trails. DORA's 2024 elite cluster has a 5% change failure rate and its high cluster 20% ([dora-2024](../references.md#dora-2024)), and Google reports that a change passing presubmit passes the rest of its tests 95% or more of the time ([google-tap](../references.md#google-tap)). North's unit is a landing on main, not a deployment, and its rework counts fixes forward as well as reverts, so it is the stricter count; it is still about four times the elite rate.
- Worker first-try success, 55%: no comparable figure. Benchmarks score a model and scaffold on curated tasks whose tests are given; METR's time-horizon study puts the frontier's 50% success at tasks of about 50 minutes for Claude 3.7 Sonnet as of March 2025 ([metr-horizon](../references.md#metr-horizon)), which measures task length, not a fleet's first-try rate. No current SWE-bench leaderboard figure could be read from its primary page on 2026-11-06, so none is quoted.
- Proxy recommendations filed as an issue the same turn, 4%: no comparable figure. The `recommend-acts` Stop hook that blocks an unacted recommendation landed at 5:25 AM on 2026-11-06 (north#641), after most of this window.

## Where north stands

North leads on how fast an ask becomes a landed commit and on the median wait in the landing queue. It trails on rework (19% against DORA's elite 5%), on the landing queue's tail (p90 110 min against SubmitQueue's P95 under 19 min), and on acting on its own recommendations (4%). Corrections per hour, verification after the push and first-try success have no published figure to set them against.

## Weekly run

The fleet tick runs `north scoreboard --record` when the last recording is a week old, writing `north/scoreboard/week.json` and a one-line score under the state root; the `scoreboard` loop in `harness/loops.toml` declares it, so `north loops check` fails when the record is older than two weeks. Each recording compares every number's headline figure (the rate, the median, the share) with the week before. A number counts as worse when both weeks hold at least five observations and it moved more than 10% in its worse direction (up for corrections, waits and rework, down for first-try success and acted recommendations). The proxy hears the worse numbers once per recording, in the fleet tick; the margin and the sample floor are guesses to be tuned once a few weeks of recordings show how much a number moves without cause. `north scoreboard rework` splits the rework number by the class of change and by what the rework fixed, so the fixes can aim at the class that reworks most.
