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

Until each arm holds 15 runs stamped `Arm: <tier>` in the category, `north dispatch` and `north fleet launch` alternate
the arms (the count is stamped runs, not all runs on the tier, because a tier's unstamped runs carry the earlier
routing's selection bias and counting them settled every category on 31 Oct with the Opus arm at 8 to 11 runs). Sonnet
runs only at high effort: a launch without an effort takes the account default, medium, so the session registry pins
Sonnet to high and refuses any other effort.

`north tune` writes each settled category's route to `~/.local/state/north/tune/cells.json` once a day, and dispatch
follows that cell while the file is under 36 h old (one missed daily tick plus slack); with no cell for the category or
an older file it settles the same rule live. The decision is `Evidence::settled` in both places, so the cell is a dated
copy of it, not a second rule.

Implemented: arm-stamped run counts (a box on a tier arm carries that one `Arm:` line, because the ledger records only the first), alternation, the cost per closed box decision and the Sonnet effort pin.
Implemented 2026-11-04: the rework signal (`threads/calibration/rework.rs`). A landed run is reworked when, within 48 h of its `landed_sha` reaching main, a main commit reverts it (a `This reverts commit <sha>` body or a `Revert "<subject>"` subject), a later commit names its item or sha with fix intent (`Follows:`, `fixes`, `fix forward`) or has a subject starting `Fix` and touches at least half its files, or red-main-notify recorded it as a red issue's first failing commit. A reworked run is not kept, so it lowers `ok` and the success rate recommend uses, and `north health` fails when a repository's 7-day rework rate passes `rework_alarm_pct`. This is the quality watch the landing-gate retirement of 2026-11-04 relied on: a sustained alarm is the evidence for bringing a gate back.
Not yet implemented: a rework term inside the cost per closed box decision; routing still decides on cost per closed box alone.

Alternatives that lost:
- Per-run done rate: counts a retried or reworked box as several chances and rewards cheap runs that need a follow-up.
- Cost alone: stretches usage but hides quality loss that surfaces as later fix commits.

Trade-off accepted: the rework terms lag a landing by 48 h, so a new category decides on cost per closed box first and
its rework only once follow-ups have had time to appear.

Overturned by: rework terms that do not separate the tiers on any category after a month of data, or a tier whose
rework is high on a category while its cost per closed box still wins.

Evidence: north#247 (runs per closed item); threads summary --since 2026-10-29 (feature 99 Sonnet to 10 Opus-arm runs
before the fix).

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

Trade-off accepted: `recommend --planner` still prints a band, but nothing routes on it; planner effort stays high by
default and rises only from an issue's own Not dones.

Overturned by: a planning-time input (box count, files named, issue text) that lifts the held-out check above chance
at alpha 0.05 on a later ledger.

## Offline model replay for tier arms is superseded by ledger routing

Reasoning: north#38 asked for a 120-run `policy-replay` sweep over 40 Test-kind scenarios with worker, worker-sonnet and
Haiku arms, then a 20-box confirmation set, to choose the middle tier. The live arms above now make that choice from
the ledger's cost per closed box on real boxes, so the replay would spend about 120 model runs to answer a question
already answered on the work itself. Its two boxes were retired on north#242 on 2026-11-01 with the planner boxes.

Overturned by: a category whose live arms cannot reach 15 stamped runs each in a month, where a replay is the only
way to compare tiers.

Evidence: north#242 comment of 2026-11-01 01:55 UTC (planner-band reading); north#38 and north#54 for the original
questions.
