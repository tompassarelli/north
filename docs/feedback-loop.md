# Feedback loop

Every recorded signal, who writes it and what reads it on main (north#35). Reader cells come from
`rg -n -g '!*.test.*'` on main (2026-10-12); schema lines and the writer itself don't count. S: is the
operator directory in south, mc/ is `socrates/skills/machine-capacity/scripts`.

Every threshold and prior these loops apply lives in `socrates/defaults.toml` with its value, source
(measured, a dated Tom decision, a named standard, or a prior with a `settles_when` count the weekly `defaults-measure.py --priors` check reads) and thin-data fallback; consumers read it
there and `socrates/lib/defaults.test.py` fails on a hard-coded value or a blank source. The weekly review
re-runs the measured sources with `socrates/scripts/defaults-measure.py <copy of snapshot.db> <default>`, which
sweeps one default over a grid by walk-forward replay and prints n and the score per value.

| signal | writer | reader on main | verdict | commit |
|---|---|---|---|---|
| runs.eta_min | bin/worker-ledger | bin/threads:1319 calibrate_eta scales recommend_eta | read | ac79788c |
| runs.minutes | bin/worker-ledger | bin/threads:1243 recommend_eta | read | |
| runs.outcome, follows, landed, expects_landing | bin/worker-ledger | bin/threads:1408 run_ok; socrates/lib/plan_usage.py:153 routing success | read | 4a58f21e |
| runs.tier | bin/worker-ledger | bin/threads:1191 recommend, every provider | read | 4b71ac63 |
| runs.category and facets | bin/threads brief_fields | bin/threads:1191 recommend, :1243 recommend_eta | read | |
| runs.arm | bin/threads:967 | socrates/lib/report.py arm_lines, Wilson 95% per arm | read | lane H |
| runs.lens, flaws | bin/worker-ledger | bin/threads:1113 summary lens table | read | |
| runs.repeats | none | none | dropped | 4aecdaf9 |
| runs.tokens | bin/worker-ledger | bin/threads:1097 tokens_med, :1526 tokens_per_closed | read | |
| runs.cost_usd | bin/threads:989 | bin/threads:1184 recommend $ per success; socrates/lib/ledger_context.py:259; socrates/lib/project_score.py:51 | read | 4b71ac63 |
| runs.staffed, review_requested, landed_at | bin/worker-ledger | socrates/lib/ledger_context.py:248 delivery_metrics | read | |
| runs.first_commit | none (tally-internal, bin/worker-ledger:110) | none | dropped | 4aecdaf9 |
| runs.turns | none | none | dropped | 4aecdaf9 |
| runs.peak_ctx, handoff_fires, handoff_ignored | socrates/lib/ledger_context.py | bin/capacity-watchdog:465 | read | |
| runs.compactions, input_tokens, cache_read_tokens, idle_gap_misses | socrates/lib/ledger_context.py | bin/threads:1057 context_summary | read | |
| runs.account, model, effort, fast | bin/threads:902 | bin/threads:1131 delivery_summary | read | |
| runs.note, report, session, role | bin/threads | `threads show`, role filters | read | |
| calibration | bin/threads calibrate | bin/threads:1395; socrates/lib/calibration.py:23 | read | ac79788c |
| run_usage | bin/threads:897 | bin/threads allocation into run_costs | read | |
| run_costs | bin/threads:987 | bin/threads:989 rolls into runs.cost_usd | read | |
| usage | socrates/lib/usage.py | socrates/lib/usage.py:111, routing and the pace gate | read | |
| claims.eta_min | bin/threads:483 | bin/threads:608 OVERDUE; src/work_items.rs:82 | read | |
| tier_choice | bin/threads:1431 | bin/threads:1427 incumbent | read | |
| events kind=run | none | none | dropped | 4aecdaf9 |
| run_events | none | none | dropped | 4aecdaf9 |
| capacity_runs cores, memory, signature | mc/machine-capacity-host.mjs:386 | mc/machine-capacity-logic.ts estimateCapacity | read | |
| capacity_runs.wallSeconds | mc/machine-capacity-host.mjs:1123 | mc/machine-capacity-logic.ts:302 duration p75 | read | 4aecdaf9 |
| capacity_runs io, diskBusyShare | mc/machine-capacity-host.mjs | mc/machine-capacity-logic.ts:336 estimate output | read | |
| capacity_runs.exitCode | mc/machine-capacity-host.mjs:1120 | mc/machine-capacity-logic.ts:305 duration p75 counts only exit 0 | read | 99f4618d |
| capacity_runs.command, cpuSeconds | mc/machine-capacity-host.mjs:1120 | none | held for north#17 | |
| job_runs | bin/threads:1748 | bin/threads:1753 job-runs; bin/job-watch | read | |
| model_events | bin/model-watch | socrates/lib/plan_usage.py:134 successors, routing hold | read | 4a58f21e |
| worker-ledger --summary | bin/worker-ledger | socrates/lib/tick_precheck.py:258 | read | |
| stalls.jsonl | bin/worker-sweep:415 | socrates/lib/stall_recovery.py:9 via `agents report` | read | 871718bb |
| worker-sweep flags | bin/worker-sweep:148 | socrates/lib/tick_precheck.py:15 | read | |
| machine-capacity-usage.jsonl | mc/machine-capacity-host.mjs:315 | mc/machine-capacity-host.mjs:623, :660 sizing | read | |
| usage-gate.json | socrates/lib/usage.py:185 | socrates/hooks/lib/spawn_capacity.py:280 | read | |
| spawn-gate-denials.jsonl | socrates/hooks/lib/spawn_capacity.py | socrates/lib/report.py:118 | read | 871718bb |
| land-checks.tsv | none | none | dropped | 4aecdaf9 |
| goal-scores.jsonl | socrates/lib/project_score.py | socrates/lib/report.py:119 | read | 871718bb |
| S:costs.toml | hand | socrates/lib/plan_usage.py:79; socrates/lib/usage.py:267 | read | |
| costs/*.json | socrates/lib/billing_*.py | socrates/lib/usage.py:264 service lines | read | |
| S:operating-point.toml | socrates/scripts/policy-replay.py | socrates/scripts/agent-config-check.sh:246; bin/skill-prune:17 | read | |
| S:incidents.jsonl | socrates/scripts/incident-mine.py | socrates/scripts/agent-config-check.sh:296 | read | |
| replay sweep-*.jsonl | socrates/scripts/policy-replay.py:590 | socrates/scripts/agent-config-check.sh:201 | read | |
| socrates/model-catalog.toml | hand | socrates/lib/usage.py:31; socrates/lib/ledger_context.py:138 | read | |
| S:incident-baseline.toml | hand | S:routines/tick.md step 3 one-shot recount after 30 sessions | read (tick prose) | |

## Still unread

- capacity_runs.command, cpuSeconds: held for north#17's resource estimator, which needs 7 days of command data
  (about 18 Oct); north#17 wires or drops them.

Thresholds and priors in these loops, and the 350k handoff decision, belong to north#35 lane D.

## Learned routing backtest

Tried, no durable gain (north#35, 2026-10-12). `python3 socrates/scripts/routing-backtest.py <copy of snapshot.db>`
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

## Goal ETA (north#35 box 7)

`agents plan` prints one line per `[projects.P] target`: the critical path over the goal's open Done-when boxes
(`socrates/lib/goal_eta.py`). The goal's issues are `[projects.P] goal = ["P#N", ...]` when set, else the open
`priority:now` issues. Each box takes recommend's tier and calibrated p75 for its cell, backing off to its category
cell, then to the uncalibrated project median brief-to-landed (flagged `uncal` in the n list). Boxes depend on each
other through `depends on box N` / `after #N` lines and GitHub blockedBy, and are otherwise independent. Width is the
measured peak of parallel landed workers while the spawn gate admits the project (capacity under its CPU and pressure
limits, at least one account not pace-held), else 1. Each box adds the `agents flow` push-to-land median. Every run
appends a `goal_eta` calibration row through `threads calibrate --write --goal-row`; `agents plan --score` appends a
`goal_actual` row (actual/predicted) once the goal's issues are closed; `agents report` shows the newest of each.
`agents flow` raises the projected-miss decision only when the newest `goal_eta` prediction for the current target
lands after it, not from the whole-repo open-issue count. `agents plan --quote` uses the same schedule for N
independent boxes (`--category` for the calibrated cell) and keeps its $ part.

Replay at each goal's start, from ledger runs ended before it (`socrates/lib/goal_eta.test.py`,
fixture `socrates/lib/fixtures/goal-eta-2026-10-12.json`):

| goal | start | predicted at start | actual | serial hand-sum | hand estimate then |
|---|---|---|---|---|---|
| policy-lead harness goal (26 planned boxes) | 05:24Z | 07:43Z (2h19m, width 22) | 06:35Z last planned box; 07:50Z with added scope | 12:36Z (7h12m) | 2026-10-13 12:00Z (planner) |
| north#35 (boxes 1-5) | 07:05Z | 07:15Z (10m, all 5 boxes uncalibrated) | open: boxes 1, 3, 4 landed by 07:52Z; 2 and 5 open at 08:40Z | 07:55Z (50m) | 2026-10-14 15:59Z ("Wednesday") |

The harness goal's critical path over-predicted the planned boxes by 69 minutes and under-predicted the full goal by
7. North#35's boxes carry `Category: audit + fixes`, which names no category, so every box fell back to the 9-minute
project median and the estimate is far too short; give goal boxes a `Category:` line.
