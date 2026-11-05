# Learned routing and goal ETA measurements

Dated measurements behind two routing decisions: the learned tier-routing backtest that kept the cell-rate rule, and the replay that set how the goal ETA reads box durations. Moved from docs/feedback-loop.md on 2026-11-04 so that doc lists only live signals.

## Learned routing backtest

Tried, no durable gain (north#35, 2026-10-12). The one-off routing-backtest script that produced this table
(stdlib only, about 35 s on a copy of the snapshot), was deleted unported on 2026-10-30 (cbe96542f) because nothing called it; it is in
git history before this change. The 2026-10-12 snapshot copy gave: 2434 finished worker runs over
2026-10-01..12, 46% ok by `run_ok`, 1118 with a known category, 549 priced, 981 with an ETA, 144 with facets, 7
with an arm, 39 of 67 (category x tier) cells at `min_runs`. That is enough for a cell-level model of the tier
pick but thin for retry vs abort, where only 113 failed runs have a resolved `Follows:` run.

Each prediction trains only on runs that ended before it started. (a) scores the pick over 806 first-assignment
decisions against `recommend` with held incumbents, using the same threshold, `min_runs` and $-per-success
structure. Arms replace the cell success rate with the model's probability. Accuracy is the predicted outcome of
the tier actually run; the rule predicts from its backoff cell rate at 50%. Escalations (restaffed or escalated)
and wasted runs (not ok) per 100 decisions are measured on the runs whose actual tier matched each pick. (b) scores
retry vs abort against the 2-failed-fixes rule (retry at depth 1): a futile retry is a wasted run, and an abort
whose retry succeeded is a needless escalation. Proportions use Wilson 95% intervals, and differences use a 2000-draw
paired bootstrap.

| task | arm | n | arm accuracy | rule accuracy | accuracy diff, pts | escalations saved | wasted runs saved | unit |
|---|---|---|---|---|---|---|---|---|
| (a) tier | logistic regression | 806 | 65% [61%, 68%] | 62% [59%, 65%] | +2.6 [+0.0, +5.3] | +0.0 [-1.6, +1.7] | -5.1 [-8.8, -1.1] | /100 decisions |
| (a) tier | matrix factorisation | 806 | 63% [59%, 66%] | 62% [59%, 65%] | +0.7 [-1.7, +3.3] | -0.2 [-1.6, +1.3] | -5.7 [-9.5, -2.3] | /100 decisions |
| (b) retry | logistic regression | 113 | 52% [43%, 61%] | 54% [45%, 63%] | -1.8 [-10.6, +7.1] | +1.0 [-6.0, +7.0] | -3.0 [-11.0, +5.0] | runs |
| (b) retry | matrix factorisation | 113 | 57% [47%, 65%] | 54% [45%, 63%] | +2.7 [-7.1, +12.4] | +3.0 [-4.0, +9.0] | +0.0 [-8.0, +8.0] | runs |

Logistic regression predicts outcomes slightly better than the cell rate. But its picks waste more runs (42% vs
37% on matched runs), and no saving CI excludes 0 in either arm's favour, so nothing enters the spawn gate.
LightGBM was not tried: logistic regression's training accuracy (67%) sits 2 points above its walk-forward accuracy,
so features and data limit it, not model capacity. The BERT arm and Arch-Router-1.5B were skipped because the ledger
holds no brief text at decision time; `runs.report` is written after the run. The matrix-factorisation arm
reimplements the scoring idea of RouteLLM (lm-sys/RouteLLM, Apache-2.0; Ong et al. 2024, arXiv:2406.18665)
pointwise on (category x tier) and (category x tier|depth) ledgers, with no RouteLLM code copied. Re-run it when
facets cover most runs or (b) passes about 300 labelled cases.


### Similar-task retrieval: keep current routing (2026-10-15)

The narrow `--retrieval-only --boot 2000 --seed 7` replay completed on a frozen read-only snapshot with 344 finished worker runs satisfying the exact `flow.brief_coverage` contract, above the >=300 retrieval trigger, among 3600 finished workers. It evaluated 138 targets. The script holding both replays was deleted on 2026-10-30 (cbe96542f), and the `north flow` trigger line went with it on 2026-11-05.

The new arm scores the same source-qualified first-assignment targets for k=10 and k=20. Original brief token-count cosine similarity uses no fitted vocabulary or corpus statistics. Training runs must finish strictly before each target starts; decisions and recommend incumbents advance in target-start order. Success uses the existing `succeeded` rule, with Follows labels and required landings available before the decision. Invalid historical tiers are excluded, never coerced. The baseline calls the corrected pure `recommend` with its existing sparse-evidence rules.

Within the k nearest positive-similarity neighbors, each tier's price per success is the sum of positive timestamped allocations for priced neighbors divided by successful priced neighbors. A tier needs a priced success; missing prices cannot supply free successes. Allocation rows must end strictly before the decision; untimestamped aggregate run prices are excluded. These sums may be partial. Allocation-window endpoints support this exclusion but cannot establish historical ingestion time or price-configuration availability. Missing retrieval evidence abstains.

Evaluation reports each arm's actual executed-tier matches, abstentions, and waste/escalation rates. A paired target-row bootstrap uses seed 7 and 2000 draws for both k values. Empty support or any unsupported resample yields an unavailable CI and no adoption candidate. Matched arms can select different populations; these observational comparisons cannot establish causal improvement. Only a strictly positive waste-saving CI can produce a candidate for independent validity review. Production recommend remains unchanged. Three collected synthetic scenarios pass and kill recorded scratch faults for future allocation leakage, ignoring the k limit, and treating empty matches as zero loss. Private frozen input, source identities, exact commands and mutation receipts are held in the owning review handoff; no briefs or native run identifiers enter Git.

Savings below are current-rule rate minus retrieval rate, in percentage points; positive favors retrieval. Both arms use 2000 supported bootstrap draws. The current rule matched 45 executed tiers, abstained on 28 targets, and had 66.67% waste and 8.89% escalation among matches.

| Neighbors | Retrieval matches / abstentions | Waste / escalation | Waste saving, 95% CI | Escalation saving, 95% CI |
| --- | --- | --- | --- | --- |
| 10 | 48 / 38 | 60.42% / 8.33% | +6.25 pp [-9.31, +22.97] | +0.56 pp [-7.66, +8.57] |
| 20 | 56 / 14 | 69.64% / 12.50% | -2.98 pp [-17.37, +11.59] | -3.61 pp [-12.40, +4.03] |

Neither waste-saving interval excludes zero, so neither arm qualifies for adoption. Keep current routing. Matches and abstentions are separate subsets of the 138 targets; unsupported or unmatched predictions account for the remainder. The different matched populations and partial historical cost availability limit this observational comparison.

## Goal ETA

`agents plan` prints one line per `[projects.P] target`: the critical path over the goal's open Done-when boxes
(`crates/north-ops/src/ops/agents/goal/`). The goal's issues are `[projects.P] goal = ["P#N", ...]` when set, else the open
`priority:now` issues. A box's ETA is a box duration, not one run: `threads box_chains` chains an item's worker runs
in staffed order while each starts within `box_chain_gap_min` (30) minutes of the chain's latest end, and a chain
with a landed run lasts from its first staffed to its last landed, so it holds the worker run, follow-ups, the lead's
review and landing, and retries. Chains are by `Item:`, because runs carry no box and `Follows:` is free text in the
ledger (most values are words like `Codex` or `none`); parallel boxes of one item share a chain. The box ETA is the
p75 chain duration for the box's category (at least `eta_min_samples` chains), else over every category, printed
with its n; with no chains it falls back to the flagged project median brief-to-landed. `recommend_eta` stays the
per-run ETA for briefs. `threads calibrate` writes a `box` row (walk-forward actual/predicted per chain, cell `all`
since a chain mixes providers), applied once it holds `box_calibration_n` (200) chains. Boxes depend on each other
through `depends on box N` / `after #N` lines and GitHub blockedBy, and are otherwise independent. Width is the
measured peak of parallel landed workers while the spawn gate admits the project (capacity under its CPU and pressure
limits, at least one account without a usage refusal), else 1. A chain ends at its landing, so no landing overhead is added.
Every run first scores the goal, then appends a `goal_eta` calibration row through `threads calibrate --write --goal-row`.
The `goal_eta` source names each open box and its minutes. Scoring takes the first `goal_eta` since the project's last
`goal_actual` and appends a `goal_actual` row (actual/predicted) when a goal closes, scored against its last issue
close. Partial goals are scored per landed box: the boxes the prediction named that have since ticked or closed,
against the landed share of its predicted box minutes. `agents plan --score` scores the same way after the target.
`threads calibrate` refuses a `goal_eta` whose target is already past, and `agents plan` records no prediction then,
so a goal ETA is always quoted against a current target. `agents report` shows the newest of each. `agents flow` raises the projected-miss decision only when the newest `goal_eta` prediction for the current
target lands after it, not from the whole-repo open-issue count. `agents plan --quote` uses the same schedule for N
independent boxes (`--category` for the box duration) and keeps its $ part.

Replay at each goal's start, from ledger runs ended before it, landings after it dropped, no box calibration
(`goals_of_12_oct_replayed_from_their_start` in `crates/north-ops/src/ops/agents/goal/tests.rs`, fixture `crates/north-ops/src/ops/agents/goal/fixtures/goal-eta-2026-10-12.json`):

| goal | start | predicted (box duration) | lane M (per-run p75) | actual | serial hand-sum | hand estimate then |
|---|---|---|---|---|---|---|
| policy-lead harness goal (26 planned boxes) | 05:24Z | 12:54Z (7h30m, width 22) | 07:43Z (2h19m) | 06:35Z last planned box; 07:50Z with added scope | 21:42Z (16h18m) | 2026-10-13 12:00Z (planner) |
| north#35 (boxes 1-5) | 07:05Z | 08:07Z (62m, all-category box p75, n=564) | 07:15Z (10m, uncalibrated) | 08:10Z box 5 landed (boxes 1, 3, 4 by 07:52Z; box 2 open at 08:20Z) | 12:15Z (5h10m) | 2026-10-14 15:59Z ("Wednesday") |

North#35 moves from 55 minutes early to 3 minutes early. The harness goal moves away, from 68 minutes late to 6h19m
late against its planned boxes: its 45 runs carried no `Item:`, so no chain describes them, and each planned box was
one 1-12 minute run that the worker landed itself under a lead watching the lane. The box p75 is learned from
`Item:` chains (makgora, wisp and north issues with review, retries and lead landings), and the plan's 9-deep
dependency path multiplies that per-box gap. A goal run that way is better estimated per run; the box duration fits
goals whose boxes are tracked issues.

