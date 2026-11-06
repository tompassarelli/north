# Tier routing rationale

Cold-path reasoning behind which model tier a box starts on. Agents load the rule from the workers skill's routing
paragraph; this file holds why, the measure and what would overturn it.

## Route to the cheapest tier whose landed work does not raise rework

Reasoning: Tom (31 Oct 2026) wants usage stretched as far as possible, but not at a significant cost to code quality
and maintainability where a cheaper model is clearly costing it. Weekly headroom is the binding limit, so the Sonnet arm
matters. A per-run success rate cannot carry that: a Sonnet run that lands and is fixed twice costs more than an Opus
run that lands once, and the ledger's per-run and per-closure figures disagreed (feature: Sonnet 1.3 to 1.6 runs per
closed item, Opus medium 2.9, while its per-run rate looked worse).

The unit is a landed, kept outcome in a category. Its cost is the API-equivalent spend of every run that closed the box
(`north ledger summary`, cost per closed box, which also carries the spend of boxes started in the window that did not close, 2026-11-04) plus the spend the work later caused:
- follow-up fix commits on the same files within 48 h of the landing;
- reverts of the landing;
- landings named as the culprit of a red main;
- review findings against the landing;
- code-health signal on the files it touched.

A tier is eligible in a category while its rework per landed outcome is not above the other arm's by a margin the
sample supports; among eligible tiers the lowest cost per landed, kept outcome wins, Sonnet on a tie.

Until each arm holds 15 runs stamped `Arm: <tier>` in the category and 15 closed arm boxes (boxes whose first run
carried that arm) in the last 30 days, `north dispatch` and `north fleet launch` alternate the arms, then route to the
arm with the lower spend per kept closed arm box. The count is stamped runs and the cost is arm boxes, not all runs and
boxes on the tier, because a tier's unstamped work carries the earlier routing's selection bias: counting those runs
settled every category on 31 Oct with the Opus arm at 8 to 11 runs, and pricing those boxes made Opus look 2.4 times
dearer on feature than its own arm boxes show (Arm result, 2026-11-05). Sonnet
runs only at high effort: a launch without an effort takes the account default, medium, so the session registry pins
Sonnet to high and refuses any other effort.

`north tune` writes each settled category's route to `~/.local/state/north/tune/cells.json` once a day, and dispatch
follows that cell while the file is under 36 h old (one missed daily tick plus slack); with no cell for the category or
an older file it settles the same rule live. The decision is `Evidence::settled` in both places, so the cell is a dated
copy of it, not a second rule.

Implemented: arm-stamped run counts (a box on a tier arm carries that one `Arm:` line, because the ledger records only the first), alternation, the cost per closed box decision and the Sonnet effort pin.
Implemented 2026-11-04: the rework signal (`threads/calibration/rework.rs`). A landed run is reworked when, within 48 h of its `landed_sha` reaching main, a main commit reverts it (a `This reverts commit <sha>` body or a `Revert "<subject>"` subject), a later commit names its item or sha with fix intent (`Follows:`, `fixes`, `fix forward`) or has a subject starting `Fix` and touches at least half its files, or red-main-notify recorded it as a red issue's first failing commit. A reworked run is not kept, so it lowers `ok` and the success rate recommend uses, and `north health` fails when a repository's 7-day rework rate passes `rework_alarm_pct`. This is the quality watch the landing-gate retirement of 2026-11-04 relied on: a sustained alarm is the evidence for bringing a gate back.
Implemented 2026-11-05: the rework term inside the cost per closed box decision. `north ledger summary` marks a closed box reworked when any of its landed runs is reworked by the rule above (`closed_source.rs` reworked_agents), and dispatch and `north tune` divide a tier's whole spend in the category (closed and unclosed boxes) by its kept boxes (an arm's boxes only, from the Arm result of 2026-11-05), the closed boxes not reworked, so a reworked box adds its spend and removes itself from the divisor; a tier whose every closed box was reworked costs infinity and loses (`a_reworked_landing_counts_against_its_tier_in_the_cost_per_closed_box_decision`). `north flow` prints the outcome loop per week, tier and arm: landed changes, rework rate, cost per surviving landing, first-commit-to-main p50 and landed changes per hour of Tom's attention (`ledger/outcome`).

Alternatives that lost:
- Per-run done rate: counts a retried or reworked box as several chances and rewards cheap runs that need a follow-up.
- Cost alone: stretches usage but hides quality loss that surfaces as later fix commits.

Trade-off accepted: the rework terms lag a landing by 48 h, so a new category decides on cost per closed box first and
its rework only once follow-ups have had time to appear.

Overturned by: rework terms that do not separate the tiers on any category after a month of data, or a tier whose
rework is high on a category while its cost per closed box still wins.

Evidence: north#247 (runs per closed item); threads summary --since 2026-10-29 (feature 99 Sonnet to 10 Opus-arm runs
before the fix).

## Arm result, 2026-11-05

The first reading of the Sonnet and Opus arms since `north dispatch` began stamping them on 27 Oct (north#242). Data:
Nexus's ledger as the laptop's `~/.local/state/threads/snapshot.db` held it at 9:51 AM +07 on 5 Nov, copied to
`/tmp/n242/threads.db` and read with `THREADS_DB=/tmp/n242/threads.db`; closed state from GitHub at 10:15 AM +07.

Queries:
- Q1, stamped runs per arm, the count dispatch compares with 15 (`ledger::arm_runs`): `sqlite3 -readonly
  /tmp/n242/threads.db "SELECT category, arm, COUNT(*) FROM runs WHERE COALESCE(arm, '') IN ('sonnet-high', 'worker')
  GROUP BY 1, 2"`.
- Q2, arm boxes (each box goes to the arm its first run carried, its whole spend with it): `north ledger summary
  --since 10d --json | jq -c '.arms | map(select(.arm == "sonnet-high" or .arm == "worker")) | group_by([.category,
  .arm])[] | {category: .[0].category, arm: .[0].arm, boxes: length, closed: (map(select(.closed)) | length), reworked:
  (map(select(.reworked)) | length), usd: (map(.usd) | add)}'`. Closed-box rate is closed / boxes; spend per closed box
  is usd / (closed - reworked), the kept-box rule of `Evidence::priced`. Ten days covers every arm box.
- Q3, the tier-wide figure dispatch and `north tune` decided on until this change: `north ledger summary --since 7d
  --json | jq -c '.cells[] | {tier, category, closed, reworked, usd, unclosed_usd}'`, a tier's (usd + unclosed_usd) over
  its kept boxes summed across the cells whose tier names it.
- Q4, rework per arm and by cause: `north worker-ledger --summary`, columns `rework` and `revert/fix/culprit` (`fix` is
  a follow-up fix commit within 48 h), over worker-role runs, so its n is below Q1's.
- Q5, closed-box rates compared with Fisher's exact test, two-sided.

Of the rework terms this page lists, follow-ups, reverts and red-main culprits are measured (Q4). Review findings
against a landing and code-health signal on its files are not measured: the ledger records neither per landing.

Categories with at least 15 stamped runs per arm (Q1): feature (65 sonnet-high, 67 worker), tooling (53, 39) and
bug-known-cause (27, 19). Below 15: native-check (9, 6; at the worker arm's 0.9 runs a day since 29 Oct it reaches 15
around 16 Nov, but neither arm has closed a box, so no closed-box date can be projected) and balance-tuning (3, 3; no
stamped run since 31 Oct, so no date).

### feature

- Q2: sonnet-high 23 boxes, 14 closed (61%), 2 reworked, $123.75, so $10.31 per kept box; worker 17 boxes, 14 closed
  (82%), 0 reworked, $116.64, so $8.33 per kept box.
- Q4: run-level rework sonnet-high 7 of 34 landings found on main (20%: 0 reverted, 6 fixed forward, 1 red-main culprit), worker 8 of 40 (20%: 0, 6, 2); p = 1.00.
- Q5: closed-box rate 14 of 23 against 14 of 17, p = 0.18; arm-box rework 2 of 14 against 0 of 14, p = 0.48.
- Q3, the earlier basis: Sonnet $11.51 per kept box ((575.85 + 264.21) / 73) against Opus $27.67 ((578.93 + 40.04 +
  25.07 + 20.10) / 24). That gap is 2.4 times; the arm boxes show none. Opus's tier cells hold the unstamped hard boxes
  the earlier routing sent it (wisp#79 $83.04, makgora#666 $109.40), so the tier-wide figure measured routing, not
  the models. It had routed feature to sonnet-high since 30 Oct.
- Verdict: no difference. The arms differ by $1.98 per kept box, and dropping one open Sonnet box (makgora#735,
  $34.84) turns it to Sonnet's $7.41 against $8.33. Each arm holds 14 closed boxes; at about 1.5 closed boxes a day per
  arm, both reach 15 on 6 Nov, and the rule decides then.

### tooling

- Q2: sonnet-high 5 boxes, 3 closed (60%), 0 reworked, $338.82, so $112.94 per kept box, of which $295.11 is one open
  umbrella box (north#360, the landing train); worker 3 boxes, 3 closed (100%), 1 reworked, $17.98, so $8.99.
- Q4: sonnet-high 7 of 29 (24%: 0, 7, 0), worker 8 of 25 (32%: 0, 8, 0); p = 0.56.
- Q5: closed-box rate 3 of 5 against 3 of 3, p = 0.46.
- Q3, the earlier basis: Sonnet $26.51 ((116.96 + 333.70) / 17) against Opus $16.84 ((120.41 + 47.97) / 10). The
  next `north tune` tick on that basis would have flipped tooling from sonnet-high, its route since 1 Nov, to worker.
- Verdict: keep alternating. Three closed boxes per arm in nine days, 0.33 a day: 15 is about 36 days off (around 11
  Dec), longer than the 30-day window, so at this rate tooling alternates until it is staffed faster or north#564
  answers the window question.

### bug-known-cause

- Q2: sonnet-high 7 boxes, 5 closed (71%), 0 reworked, $27.09, so $5.42 per kept box; worker 7 boxes, 7 closed
  (100%), 0 reworked, $51.91, so $7.42.
- Q4: sonnet-high 2 of 16 (12%: 0, 1, 1), worker 5 of 17 (29%: 0, 5, 0); p = 0.40.
- Q5: closed-box rate 5 of 7 against 7 of 7, p = 0.46.
- Q3, the earlier basis: Sonnet $12.42 ((209.75 + 47.69 + 3.46) / 21) against Opus $25.43 ((180.51 + 6.75 + 395.36
  + 180.29) / 30).
- Verdict: keep alternating. At about one closed box a day per arm (sonnet-high since 31 Oct, worker since 29 Oct),
  worker reaches 15 around 13 Nov and sonnet-high around 15 Nov.

### What changed

The rule in the workers skill takes the arm with the lower spend per closed box once each arm holds 15 stamped runs.
Applied to the tier-wide figure it settled feature and tooling on sonnet-high and was about to move tooling to
worker, each on a gap the arms' own boxes do not show. Applied to arm boxes it would settle feature on worker today
and tooling on worker, on 14 and 3 closed boxes per arm, one of them an umbrella issue. Neither is a measured
difference, so the routing changed instead (`ops/dispatch/tier.rs`):
- `Evidence::priced` prices each arm from its own arm boxes in the category, closed and open, over its kept closed
  boxes; a tier's unstamped boxes no longer price it.
- `Evidence::settled` also needs 15 closed arm boxes per arm, so the 15 that guards the run count guards the unit the
  decision is made on.
- `ARM_WINDOW` is 30 days rather than 7, since a week held at most 14 closed arm boxes per arm.
- `the_feature_arms_of_5_november_with_14_closed_boxes_each_keep_alternating_whatever_the_tier_wide_cells_say` pins
  today's feature figures to alternation, and the other `tier_tests` and `tune` tests pin the 14-box hold and the
  unstamped-box exclusion.

Effect once this runtime reaches Nexus: the next tune tick drops the feature and tooling route cells ("no longer
decided"), and feature, tooling and bug-known-cause boxes alternate again until their arms hold 15 closed boxes each.
`north ledger summary --json` now lists each arm box with its category, closed and reworked flags, open boxes
included, which Q2 reads; `north worker-ledger --summary` gains the `revert/fix/culprit` column Q4 reads.

Open question, filed as north#564 for 20 Nov: tooling closes about 0.33 arm boxes a day per arm, which a 30-day window
cannot fill to 15, so it needs a longer window, a closed-box floor the spread supports, or the replay this page retired.

## Per-brief recommendations shrink success and price in API-equivalent dollars

`north worker-ledger --recommend` (the per-cell pick dispatch reads its tier rates from) decides as follows:
- Dollars are API-equivalent list-price dollars from `router/prices.json`, the definition the closed-box report, dispatch
  and tune use, not the allocated plan `cost_usd`. Haiku 5.5's $0.10/$0.50 per M tokens was checked against the cited
  pricing page on 2026-11-04; it holds for prompts up to 100k tokens, and longer prompts cost 5x.
- Each tier's success is the Beta-binomial posterior mean with a prior worth 15 runs (`ARM_RUNS`) at the category's
  pooled rate: (successes + 15 x pooled) / (runs + 15). The pass test against `success_threshold` and the incumbent
  margin use that estimate. A posterior mean was chosen over a lower credible bound because the bound penalises every
  small tier toward failure, while the mean only pulls it toward its category.
- A tier wins on cost only with 15 priced runs; cost is mean API $ per run divided by the shrunk success.
- Escalation-only tiers (Opus high and xhigh) are printed but never picked, as the workers skill reserves them for a
  second reasoning stop.
- When no tier passes and none has 15 priced runs, the pick is the workers skill's default for the category
  (mechanical on Haiku; known-cause bugs, tooling and features on sonnet-high; the rest on Opus medium), with that reason
  printed.

Evidence: on 2026-11-04 the old rule (raw rate, allocated $, 5 runs) picked Opus high for tooling/exact/cross-module on
2 successes of 6 runs over Sonnet high at 19 of 32 and Opus medium at 13 of 20; 23 of the 35 ledger cells then held
under 5 closed boxes.

## The planner's effort band is not predicted from planning-time facets

Reasoning: north#54 asked whether an item's category, scope, surface, summed box ETA, prior Not dones and repository
predict which items go off the rails (two or more Not done, an escalation, or over 2x the summed ETA), so a planner
could start on high, xhigh or max effort from them. Measured with `north ledger planner-band` on 2026-11-01 over 613
finished items: the held-out check ranks 5 of 54 off-rails items in the top decile, 9.26% against 10% chance
(binomial p=0.64), where 357 items on 2026-10-28 had read 17.14% (p=0.13); the best leave-one-out rule (band vote over
those facets) names the realized band for 366 of 613 items, 59.7% against 51.2% for always guessing high, and no item
falls in the max band. The two boxes were retired on 2026-11-01 when Tom's gate on north#242 took its default.

Overturned by: a planning-time input (box count, files named, issue text) that lifts the held-out check above chance
at alpha 0.05 on a later ledger.

## Planner and reviewer effort is a seeded random arm

Reasoning: the section above found that planning-time facets, category among them, predict which plans need more
effort no better than chance, so no category is pinned to xhigh. A run launched with `Category: planning` or
`Category: review` runs Opus high, and xhigh when the FNV-1a hash of its item and launch count falls in the lowest 50
of 100 (`dispatch::tier::XHIGH_PERCENT`); the brief and the dispatch record stamp `Arm: effort-high` or
`Arm: effort-xhigh`. Once both arms hold 15 runs and 15 closed boxes in a category, that category runs the arm with the
lower cost per kept box, high on a tie. Tom chose this over pinning top-band architecture, netcode-determinism and
planning plans to xhigh (gate 163, north#636, 2026-11-06). Tom asked on 2026-11-06 for a 50-50 split between high and xhigh, so the arm fills both sides at the same rate.

Overturned by: a planning-time input that predicts effort need above chance (the section above), or an xhigh arm that
reaches 15 closed boxes in no category within a month.

## Offline model replay for tier arms is superseded by ledger routing

Reasoning: north#38 asked for a 120-run `policy-replay` sweep over 40 Test-kind scenarios with worker, worker-sonnet and
Haiku arms, then a 20-box confirmation set, to choose the middle tier. The live arms above now make that choice from
the ledger's cost per closed box on real boxes, so the replay would spend about 120 model runs to answer a question
already answered on the work itself. Its two boxes were retired on north#242 on 2026-11-01 with the planner boxes.

Overturned by: a category whose live arms cannot reach 15 stamped runs each in a month, where a replay is the only
way to compare tiers.

Evidence: north#242 comment of 2026-11-01 01:55 UTC (planner-band reading); north#38 and north#54 for the original
questions.
