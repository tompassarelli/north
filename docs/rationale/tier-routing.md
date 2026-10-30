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
(`north ledger summary`, cost per closed box) plus the spend the work later caused:
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

Implemented: arm-stamped run counts (a box on a tier arm carries that one `Arm:` line, because the ledger records only the first), alternation, the cost per closed box decision and the Sonnet effort pin.
Not yet implemented: the rework terms; routing currently decides on cost per closed box alone.

Alternatives that lost:
- Per-run done rate: counts a retried or reworked box as several chances and rewards cheap runs that need a follow-up.
- Cost alone: stretches usage but hides quality loss that surfaces as later fix commits.

Trade-off accepted: the rework terms lag a landing by 48 h, so a new category decides on cost per closed box first and
its rework only once follow-ups have had time to appear.

Overturned by: rework terms that do not separate the tiers on any category after a month of data, or a tier whose
rework is high on a category while its cost per closed box still wins.

Evidence: north#247 (runs per closed item); threads summary --since 2026-10-29 (feature 99 Sonnet to 10 Opus-arm runs
before the fix).
