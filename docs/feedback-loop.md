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
| replay sweep-*.jsonl | socrates/scripts/policy-replay.py:484 | socrates/scripts/agent-config-check.sh:201 | read | |
| socrates/model-catalog.toml | hand | socrates/lib/usage.py:31; socrates/lib/ledger_context.py:138 | read | |
| S:incident-baseline.toml | hand | S:routines/tick.md step 3 one-shot recount after 30 sessions | read (tick prose) | |

## Still unread

- capacity_runs.command, cpuSeconds: held for north#17's resource estimator, which needs 7 days of command data
  (about 18 Oct); north#17 wires or drops them.

Thresholds and priors in these loops, and the 350k handoff decision, belong to north#35 lane D.
