# Feedback loop

Every recorded signal, who writes it and what reads it on main (north#35), and the experiments run on
those signals: the learned routing backtest and the goal ETA replay. Reader cells come from
`rg -n -g '!*.test.*'` on main (2026-10-12); schema lines and the writer itself don't count. S: is the
operator directory in south, mc/ is `harness/skills/machine-capacity/scripts`.

Every threshold and prior these loops apply lives in `harness/defaults.toml` with its value, source
(measured, a dated Tom decision, a named standard, or a prior with a `settles_when` count the weekly `north defaults-measure --priors` check reads) and thin-data fallback; consumers read it
there and `harness/lib/defaults.test.py` fails on a hard-coded value or a blank source. The weekly review
re-runs the measured sources with `north defaults-measure <copy of snapshot.db> <default>`, which
sweeps one default over a grid by walk-forward replay and prints n and the score per value.

| signal | writer | reader on main | verdict | commit |
|---|---|---|---|---|
| runs.eta_min | bin/worker-ledger | src/threads/calibration/forward.rs calibrate_eta scales recommend_eta | read | ac79788c |
| runs.minutes | bin/worker-ledger | bin/threads:1243 recommend_eta | read | |
| runs.outcome, follows, landed, expects_landing | bin/worker-ledger | bin/threads:1408 run_ok; harness/lib/plan_usage.py:153 routing success | read | 4a58f21e |
| runs.tier | bin/worker-ledger | bin/threads:1191 recommend, every provider | read | 4b71ac63 |
| runs.category and facets | bin/threads brief_fields | bin/threads:1191 recommend, :1243 recommend_eta | read | |
| runs.arm | bin/threads:967 | src/ops/report/arms.rs, Wilson 95% per arm | read | lane H |
| runs.lens, flaws | bin/worker-ledger | bin/threads:1113 summary lens table | read | |
| runs.repeats | none | none | dropped | 4aecdaf9 |
| runs.tokens | bin/worker-ledger | bin/threads:1097 tokens_med, :1526 tokens_per_closed | read | |
| runs.cost_usd | bin/threads:989 | bin/threads:1184 recommend $ per success; harness/lib/ledger_context.py:259; harness/lib/project_score.py:51 | read | 4b71ac63 |
| runs.staffed, review_requested, landed_at | bin/worker-ledger | harness/lib/ledger_context.py:248 delivery_metrics | read | |
| runs.landed_sha | bin/worker-ledger | none yet (north#51 box 2) | unread | |
| runs.first_commit | none (tally-internal, bin/worker-ledger:110) | none | dropped | 4aecdaf9 |
| runs.turns | none | none | dropped | 4aecdaf9 |
| runs.peak_ctx, handoff_fires, handoff_ignored | src/providers/claude/runs/telemetry.rs (Claude); harness/lib/ledger_context.py (Codex) | bin/capacity-watchdog:465 | read | |
| runs.compactions, input_tokens, cache_read_tokens, idle_gap_misses | src/providers/claude/runs/telemetry.rs (Claude); harness/lib/ledger_context.py (Codex) | bin/threads:1057 context_summary | read | |
| runs.account, model, effort, fast | bin/threads:902 | bin/threads:1131 delivery_summary | read | |
| runs.note, report, session, role | bin/threads | `threads show`, role filters | read | |
| calibration | bin/threads calibrate | bin/threads:1395; harness/lib/calibration.py:23 | read | ac79788c |
| run_usage | bin/threads:897 | bin/threads allocation into run_costs | read | |
| run_costs | bin/threads:987 | bin/threads:989 rolls into runs.cost_usd | read | |
| usage | src/usage/refresh/ | src/usage/store.rs latest, routing and the pace gate | read | |
| claims.eta_min | src/threads/claims/ledger.rs take | src/threads/claims/mod.rs stale rule, verbs.rs OVERDUE; src/work_items.rs:82 | read | |
| tier_choice | bin/threads:1431 | bin/threads:1427 incumbent | read | |
| events kind=run | none | none | dropped | 4aecdaf9 |
| run_events | none | none | dropped | 4aecdaf9 |
| capacity_runs cores, memory, signature | src/capacity/host/run.rs record | src/capacity/host/estimate.rs estimate_capacity | read | |
| capacity_runs.wallSeconds | src/capacity/host/run.rs usage_entry | src/capacity/host/estimate.rs duration p75 | read | 4aecdaf9 |
| capacity_runs io, diskBusyShare | src/capacity/host/run.rs record | src/capacity/host/estimate.rs estimate output | read | |
| capacity_runs.exitCode | src/capacity/host/run.rs usage_entry | src/capacity/host/estimate.rs duration p75 counts only exit 0 | read | 99f4618d |
| capacity_runs.command, cpuSeconds | src/capacity/host/run.rs usage_entry | none | held for north#17 | |
| job_runs | bin/threads:1748 | bin/threads:1753 job-runs; bin/job-watch | read | |
| model_events | bin/model-watch | harness/lib/plan_usage.py:134 successors, routing hold | read | 4a58f21e |
| machine-capacity-usage.jsonl | src/capacity/host/database.rs record_usage | src/capacity/host/sizing.rs size_from_usage, charge_from_usage | read | |
| usage-gate.json | src/usage/gate.rs | harness/hooks/lib/spawn_capacity.py:280 | read | |
| spawn-gate-denials.jsonl | harness/hooks/lib/spawn_capacity.py | src/ops/report/mod.rs jsonl | read | 871718bb |
| spawn-queue.json | harness/lib/spawn_queue.py from harness/lib/spawn.py | harness/lib/spawn.py release at the end of pace_sampler.sample; oldest eligible durable worker launches automatically | read | |
| land-checks.tsv | none | none | dropped | 4aecdaf9 |
| goal-scores.jsonl | harness/lib/project_score.py | src/ops/report/mod.rs jsonl | read | 871718bb |
| S:costs.toml | hand | harness/lib/plan_usage.py:79; harness/lib/usage.py:267 | read | |
| costs/*.json | harness/lib/billing_*.py | harness/lib/usage.py:264 service lines | read | |
| S:operating-point.toml | harness/scripts/policy-replay.py | harness/scripts/agent-config-check.sh:246; src/ops/timers/skill_prune.rs | read | |
| S:incidents.jsonl | north incident-mine (src/ops/incident_mine) | harness/scripts/agent-config-check.sh:296 | read | |
| replay sweep-*.jsonl | harness/scripts/policy-replay.py:590 | harness/scripts/agent-config-check.sh:201 | read | |
| harness/model-catalog.toml | hand | harness/lib/usage.py:31; harness/lib/ledger_context.py run_usage; src/providers/claude/runs/telemetry.rs Prices | read | |
| S:incident-baseline.toml | hand | S:routines/tick.md step 3 one-shot recount after 30 sessions | read (tick prose) | |

Claude worker creation (north#35): foreground `SubagentStart` lacks the original prompt, and the child prompt is persisted only after that callback. The supported route is `Agent` with `run_in_background: true`: its `PostToolUse` result has `status=async_launched` and the actual child `agentId`, so `worker-handoff` records the original tool input at launch. `worker-tier-guard` requires this route before any `Follows:` exemption.

Unjudged runs (north#35 FX5): an `unclear` run with no `Item:` is an unbriefed session and `run_outcome.unjudged` keeps it out of every success denominator (370 of 533 `unclear` worker runs over 14 days); on the 2026-10-12 snapshot, `replay-staffing --since 7d` goes from 830 decisions, 146 changed, old pick 86 (39 ok) to 817, 132 changed, old pick 72 (35 ok), and 7-day `summary` ok moves unknown/sol-high 51% of 658 to 57% of 580, unknown/sol-medium 61% of 276 to 66% of 254, unknown/medium 45% of 87 to 67% of 59, tooling/medium 63% of 138 to 69% of 126, and unknown/luna-medium (0% of 13) and unknown/gemini-pro-high (0% of 17) drop out.

## Still unread

- capacity_runs.command, cpuSeconds: held for north#17's resource estimator, which needs 7 days of command data
  (about 18 Oct); north#17 wires or drops them.

Thresholds and priors in these loops, and the 350k handoff decision, belong to north#35 lane D.

## Learned routing backtest

Tried, no durable gain (north#35, 2026-10-12). `python3 harness/scripts/routing-backtest.py <copy of snapshot.db>`
(stdlib only, about 35 s) reproduces this table from the 2026-10-12 snapshot copy: 2434 finished worker runs over
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

The narrow `--retrieval-only --boot 2000 --seed 7` replay completed on a frozen read-only snapshot with 344 finished worker runs satisfying the exact `flow.brief_coverage` contract, above the >=300 retrieval trigger, among 3600 finished workers. It evaluated 138 targets. The broad model/retry replay still waits for >500; its CLI and arms are preserved.

The new arm scores the same source-qualified first-assignment targets for k=10 and k=20. Original brief token-count cosine similarity uses no fitted vocabulary or corpus statistics. Training runs must finish strictly before each target starts; decisions and recommend incumbents advance in target-start order. Success uses the existing `succeeded` rule, with Follows labels and required landings available before the decision. Invalid historical tiers are excluded, never coerced. The baseline calls the corrected pure `recommend` with its existing sparse-evidence rules.

Within the k nearest positive-similarity neighbors, each tier's price per success is the sum of positive timestamped allocations for priced neighbors divided by successful priced neighbors. A tier needs a priced success; missing prices cannot supply free successes. Allocation rows must end strictly before the decision; untimestamped aggregate run prices are excluded. These sums may be partial. Allocation-window endpoints support this exclusion but cannot establish historical ingestion time or price-configuration availability. Missing retrieval evidence abstains.

Evaluation reports each arm's actual executed-tier matches, abstentions, and waste/escalation rates. A paired target-row bootstrap uses seed 7 and 2000 draws for both k values. Empty support or any unsupported resample yields an unavailable CI and no adoption candidate. Matched arms can select different populations; these observational comparisons cannot establish causal improvement. Only a strictly positive waste-saving CI can produce a candidate for independent validity review. Production recommend remains unchanged. Three collected synthetic scenarios pass and kill recorded scratch faults for future allocation leakage, ignoring the k limit, and treating empty matches as zero loss. Private frozen input, source identities, exact commands and mutation receipts are held in the owning review handoff; no briefs or native run identifiers enter Git.

Savings below are current-rule rate minus retrieval rate, in percentage points; positive favors retrieval. Both arms use 2000 supported bootstrap draws. The current rule matched 45 executed tiers, abstained on 28 targets, and had 66.67% waste and 8.89% escalation among matches.

| Neighbors | Retrieval matches / abstentions | Waste / escalation | Waste saving, 95% CI | Escalation saving, 95% CI |
| --- | --- | --- | --- | --- |
| 10 | 48 / 38 | 60.42% / 8.33% | +6.25 pp [-9.31, +22.97] | +0.56 pp [-7.66, +8.57] |
| 20 | 56 / 14 | 69.64% / 12.50% | -2.98 pp [-17.37, +11.59] | -3.61 pp [-12.40, +4.03] |

Neither waste-saving interval excludes zero, so neither arm qualifies for adoption. Keep current routing. Matches and abstentions are separate subsets of the 138 targets; unsupported or unmatched predictions account for the remainder. The different matched populations and partial historical cost availability limit this observational comparison.

## Goal ETA (north#35 box 7)

`agents plan` prints one line per `[projects.P] target`: the critical path over the goal's open Done-when boxes
(`harness/lib/goal_eta.py`). The goal's issues are `[projects.P] goal = ["P#N", ...]` when set, else the open
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
Every run appends a `goal_eta` calibration row through `threads calibrate --write --goal-row`; `agents plan --score`
appends a `goal_actual` row (actual/predicted) once the goal's issues are closed; `agents report` shows the newest of
each. `agents flow` raises the projected-miss decision only when the newest `goal_eta` prediction for the current
target lands after it, not from the whole-repo open-issue count. `agents plan --quote` uses the same schedule for N
independent boxes (`--category` for the box duration) and keeps its $ part.

Replay at each goal's start, from ledger runs ended before it, landings after it dropped, no box calibration
(`harness/lib/goal_eta.test.py`, fixture `harness/lib/fixtures/goal-eta-2026-10-12.json`):

| goal | start | predicted (box duration) | lane M (per-run p75) | actual | serial hand-sum | hand estimate then |
|---|---|---|---|---|---|---|
| policy-lead harness goal (26 planned boxes) | 05:24Z | 12:54Z (7h30m, width 22) | 07:43Z (2h19m) | 06:35Z last planned box; 07:50Z with added scope | 21:42Z (16h18m) | 2026-10-13 12:00Z (planner) |
| north#35 (boxes 1-5) | 07:05Z | 08:07Z (62m, all-category box p75, n=564) | 07:15Z (10m, uncalibrated) | 08:10Z box 5 landed (boxes 1, 3, 4 by 07:52Z; box 2 open at 08:20Z) | 12:15Z (5h10m) | 2026-10-14 15:59Z ("Wednesday") |

North#35 moves from 55 minutes early to 3 minutes early. The harness goal moves away, from 68 minutes late to 6h19m
late against its planned boxes: its 45 runs carried no `Item:`, so no chain describes them, and each planned box was
one 1-12 minute run that the worker landed itself under a lead watching the lane. The box p75 is learned from
`Item:` chains (smashcraft, wisp and north issues with review, retries and lead landings), and the plan's 9-deep
dependency path multiplies that per-box gap. A goal run that way is better estimated per run; the box duration fits
goals whose boxes are tracked issues.
