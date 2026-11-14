# Feedback loop

Every recorded signal, who writes it and what reads it on main (north#35), and the experiments run on
those signals: the learned routing backtest and the goal ETA replay. Reader cells name the code on main that reads each signal; schema lines and the writer itself don't count. S: is the
operator directory in the operator repository.

Every threshold and prior these loops apply lives in `harness/defaults.toml` with its value, source
(measured, a dated operator decision, a named standard, or a prior with a `settles_when` count the weekly `north defaults-measure --priors` check reads) and thin-data fallback.
`--priors` prints `WAIT` below the count's `need`, `MEASURABLE-NOT-MEASURED` once the rows are there but nobody has measured
the value, and `SETTLED` only for a prior whose `settles_when.measured` records the dated sweep result that measured it;
enough rows alone never settle a prior. Consumers read each default
there. The weekly review
re-runs the measured sources with `north defaults-measure <copy of snapshot.db> <default>`, which
sweeps one default over a grid by walk-forward replay and prints n and the score per value.

| signal | writer | reader on main | verdict | commit |
|---|---|---|---|---|
| runs.eta_min | north ledger worker, from the brief's `ETA:` head line (row.rs eta in the Claude runs provider), else the first ETA its text names | crates/north-calibration/src/threads/calibration/forward.rs calibrate_eta scales recommend_eta; the walk-forward `eta_pred` scale also multiplies the dispatch brief's ETA in crates/north-dispatch/src/dispatch/grounds.rs | read | ac79788c |
| runs.budget_usd | crates/north-threads/src/threads/runs/record.rs from the brief's `Budget:` line, the `at $X API-equivalent spend` amount (crates/north-threads/src/threads/runs/brief.rs), kept from launch through completion | crates/north-tune/src/tune/budgets.rs: `north tune` prints per category n, share over Budget and actual/E p50 and p90, and sets the `budget_usd` cells' multiplier from that p90 at n >= 30 (north#255) | read | |
| runs.minutes | north ledger worker | crates/north-calibration/src/threads/calibration/recommend.rs recommend_eta (the wall p75 printed after the active-minute figures) | read | |
| runs.outcome, follows, landed, expects_landing | north ledger worker | crates/north-calibration/src/threads/calibration/read.rs succeeded; crates/north-pace/src/usage/ctx.rs routing success | read | 4a58f21e |
| runs.tier | north ledger worker | crates/north-calibration/src/threads/calibration/recommend.rs recommend, every provider | read | 4b71ac63 |
| runs.category and facets | crates/north-threads/src/threads/runs/brief.rs | crates/north-calibration/src/threads/calibration/recommend.rs recommend, recommend_eta | read | |
| runs.arm | crates/north-threads/src/threads/runs | crates/north-dispatch/src/dispatch/shares.rs: each category's live arms and the share of its next boxes each launch draws, from each arm's kept boxes | read | lane H |
| runs.lens, flaws | north ledger worker | crates/north-calibration/src/threads/calibration/summary.rs lens table | read | |
| runs.repeats | none | none | dropped | 4aecdaf9 |
| runs.tokens | north ledger worker | crates/north-calibration/src/threads/calibration/summary.rs tokens_med, tokens_per_closed | read | |
| runs.cost_usd | crates/north-threads/src/threads/runs | crates/north-calibration/src/threads/calibration/recommend.rs recommend_usd median $ line (tier choice prices runs.tokens at API-equivalent rates); crates/north-flow/src/agents/goal/metrics.rs delivery_metrics; crates/north-flow/src/agents/goal/score.rs | read | 4b71ac63 |
| runs.staffed, review_requested, landed_at | north ledger worker | crates/north-flow/src/agents/goal/metrics.rs delivery_metrics | read | |
| runs.landed_sha | north ledger worker | crates/north-providers/src/ledger/outcome/read.rs staffed joins each worker run to its landed commit for `north ledger flow`; crates/north-calibration/src/threads/calibration/rework.rs landings judges the commit on main; crates/north-providers/src/ledger/closed_source.rs reworked_agents feeds the cost per closed box | read | |
| landings.landed_sha, first_commit_at | north land train record (the lander) | crates/north-providers/src/ledger/outcome/read.rs landed, first-commit-to-main p50 in `north ledger flow`; crates/north-flow/src/tempo/landing.rs | read | |
| land_tempo (landed, queued->main and first commit->main p50 and p90, refused alone, conflicts, queued now; one row per repo, hour and window of 60 or 1440 minutes) | `north ledger tempo --record` (crates/north-flow/src/tempo/mod.rs over the lander's `land-queue/<key>/done/` tickets, hourly by the nixos-config north-tempo timer) | the Payback line of every harness box (harness/skills/workers/references/payback.md, Landing tempo): the 1440-minute first commit->main p50 is the harness outcome number, target 10 minutes, and a landing-path box prices its saving against it | read | |
| The operator's human turns (convo store index-v5.db, role user, transcript origin human) | Claude Code transcripts, indexed by north convo | crates/north-flow/src/flow/typed.rs typed_messages feeds `north ledger flow` attention, corrections per active day and who noticed each incident first (crates/north-providers/src/ledger/outcome/attention.rs) | read | |
| north/grounding/ledger.jsonl tom correction, tom direction | north graph ground distill | crates/north-providers/src/ledger/outcome/read.rs corrections, counted only when the row's convo message is one of the operator's human turns | read | |
| runs.first_commit | none (tally-internal, crates/north-providers/src/providers/claude/runs/landing.rs tally) | none | dropped | 4aecdaf9 |
| runs.turns, tool_calls, active_min, wait_min | crates/north-providers/src/providers/claude/runs/activity.rs (Claude workers; recorded by crates/north-threads/src/threads/runs/record.rs; `threads run-ids --settled` leaves out a Claude run with a null active_min so the next ingest re-reads its transcript and fills the four columns, and Codex and Gemini rows stay out of that re-read) | crates/north-calibration/src/threads/calibration/recommend.rs recommend_eta: the `eta` line of `north ledger worker --recommend` prints `eta active <median>m (p25 <a>–p90 <b>, n=<N>) wall p75 <W>m wait median <X>m` from the cell's runs that recorded active minutes (at least `eta_min_samples` of them), and `eta wall <W>m (p75 of n=<N>, ...; too few runs with active minutes)` otherwise; active minutes are work, the wall p75 includes the waits | read | |
| runs.eta_source, budget_source (the sources of the launch estimates eta_min and budget_usd, each `tune`, `ledger` or `brief`; budget_usd is the Budget line's stop spend, else its p90; a brief with no Budget line takes the median cost of its category's landed worker runs in the week before it started, source `ledger`; null on rows recorded before 2026-11-05) | crates/north-threads/src/threads/runs/brief.rs Estimates, recorded by crates/north-threads/src/threads/runs/record.rs for every launch path (columns added by runs/columns.rs), `north cloud launch` and `north cloud wait` through crates/north-providers/src/providers/claude/runs/cloud.rs | crates/north-tune/src/tune/coverage.rs coverage rows; crates/north-tune/src/tune/estimates.rs cost ratios through crates/north-providers/src/ledger/closed_source.rs estimated_runs | read | |
| runs.skills (JSON list of the skills a run loaded by the Skill tool or the file-skill hook; null on rows recorded before 2026-11-05) | crates/north-providers/src/providers/claude/runs/telemetry.rs skills (Claude; recorded by crates/north-threads/src/threads/runs/record.rs) | crates/north-tune/src/tune/skills.rs: `north tune` prints each skill's closed-box rate and cost per closed box over 14 days against same-category boxes that did not load it, and flags installed skills no run loaded and skills whose boxes closed 20 points less often or cost 25% more per closed box | read | |
| runs.peak_ctx, handoff_fires, handoff_ignored | crates/north-providers/src/providers/claude/runs/telemetry.rs (Claude); crates/north-providers/src/providers/codex/runs/usage.rs (Codex) | crates/north-calibration/src/threads/calibration/summary.rs context_summary; crates/north-providers/src/ledger/closed_source.rs | read | |
| runs.compactions, input_tokens, cache_read_tokens, idle_gap_misses | crates/north-providers/src/providers/claude/runs/telemetry.rs (Claude); crates/north-providers/src/providers/codex/runs/usage.rs (Codex) | crates/north-calibration/src/threads/calibration/summary.rs context_summary | read | |
| runs.provider, model, effort | crates/north-providers/src/providers/claude/runs/mod.rs effort, telemetry.rs run_usage (Claude rows: the assistant message `model`; effort from the request's `output_config.effort` in the transcript, else the subagent's recorded effort, its harness/claude/agents/<type>.md `effort:`, its tier, a fleet session's launch effort, then the catalog default; provider `claude`); crates/north-core/src/fleet/watch.rs identify (fleet turn ledger runs.model, effort) | crates/north-threads/src/threads/runs/mod.rs run_ids --settled (a Claude row missing any of the three is re-read on the next ingest); tier routing and ETAs keyed on what ran | read | |
| runs.role reviewer, judge | crates/north-base/src/platform/run_roles.rs ingest_role (stored at ingest for a finished worker; ROLE_SQL still reads rows stored before 2026-11-04) | calibration, ledger/closed_source.rs | read | |
| runs.account, fast | crates/north-threads/src/threads/runs | crates/north-calibration/src/threads/calibration/summary.rs delivery_summary | read | |
| runs.note, report, session, role | crates/north-threads/src/threads/runs | `threads show`, role filters | read | |
| calibration | north threads calibrate | crates/north-calibration/src/threads/calibration/forward.rs calibrate_eta | read | ac79788c |
| run_usage | crates/north-threads/src/threads/runs | crates/north-threads/src/threads/runs/costs/mod.rs allocate into run_costs | read | |
| run_costs | crates/north-threads/src/threads/runs | crates/north-threads/src/threads/runs/costs/mod.rs rolls into runs.cost_usd | read | |
| usage | crates/north-pace/src/usage/refresh/ | crates/north-pace/src/usage/store.rs latest, routing and the pace gate | read | |
| claims.eta_min | crates/north-threads/src/threads/claims/ledger.rs take | crates/north-threads/src/threads/claims/mod.rs stale rule, verbs.rs OVERDUE; src/cli/work_items.rs:155 | read | |
| tier_predictions (item, tier, p, at, source) | crates/north-calibration/src/threads/calibration/verbs.rs recommend_with: `threads recommend` with an Item and no Follows line stores each listed tier's shrunk success, and `--launched TIER` from crates/north-dispatch/src/dispatch/effects.rs predict stores the launched tier's | crates/north-calibration/src/threads/calibration/predictions.rs score, printed by `north tune` as each category's walk-forward Brier against its base rate's | read | |
| tier_choice | crates/north-threads/src/threads/runs | crates/north-calibration/src/threads/calibration/verbs.rs incumbent | read | |
| events kind=run | none | none | dropped | 4aecdaf9 |
| run_events | none | none | dropped | 4aecdaf9 |
| capacity_runs cores, memory, signature | crates/north-capacity-host/src/capacity/host/run.rs record | crates/north-capacity-host/src/capacity/host/estimate.rs estimate_capacity | read | |
| capacity_runs.wallSeconds | crates/north-capacity-host/src/capacity/host/run.rs usage_entry | crates/north-capacity-host/src/capacity/host/estimate.rs duration p75 | read | 4aecdaf9 |
| capacity_runs io, diskBusyShare | crates/north-capacity-host/src/capacity/host/run.rs record | crates/north-capacity-host/src/capacity/host/estimate.rs estimate output | read | |
| capacity_runs.exitCode | crates/north-capacity-host/src/capacity/host/run.rs usage_entry | crates/north-capacity-host/src/capacity/host/estimate.rs duration p75 counts only exit 0 | read | 99f4618d |
| capacity_runs.command, cpuSeconds | crates/north-capacity-host/src/capacity/host/run.rs usage_entry | crates/north-capacity-host/src/capacity/host/model.rs Subject::levels keys the model by command; update folds cpu_seconds into each level | read | |
| job_runs | crates/north-threads/src/threads/runs | crates/north-threads/src/threads/jobs/mod.rs job_runs; crates/north-timers/src/job_watch/mod.rs | read | |
| machine-capacity-usage.jsonl | crates/north-capacity-host/src/capacity/host/database.rs record_usage | crates/north-capacity-host/src/capacity/host/sizing.rs size_from_usage, charge_from_usage | read | |
| usage-gate.json | crates/north-pace/src/usage/gate.rs | crates/north-guard/src/guard/spawn_capacity/usage.rs | read | |
| spawn-gate-denials.jsonl | crates/north-guard/src/guard/spawn_capacity/logs.rs | crates/north-ops/src/ops/report/mod.rs jsonl | read | 871718bb |
| spawn-queue.json | none (harness/lib/spawn.py and spawn_queue.py no longer exist on main) | none | dropped | |
| land-checks.tsv | none | none | dropped | 4aecdaf9 |
| goal-scores.jsonl | crates/north-flow/src/agents/goal/score.rs | crates/north-ops/src/ops/report/mod.rs jsonl | read | 871718bb |
| S:costs.toml | hand | crates/north-pace/src/usage/settings.rs services | read | |
| costs/*.json | crates/north-pace/src/usage/refresh/digitalocean.rs | crates/north-pace/src/usage/settings.rs service lines | read | |
| S:operating-point.toml | north policy-replay promote | crates/north-core/src/agent_config_check/gate.rs; crates/north-timers/src/timers/skill_prune.rs | read | |
| injected context per session (startup, SKILL bodies, skill references, grounding, other hook context; grounding precision and notification-triggered injections), derived from Claude transcripts | Claude Code transcripts under the north profiles' `projects` directories | crates/north-convo/src/context_audit/scan.rs audit_file; `north config context audit` measures the skill and hook slimming program | read | |
| S:incidents.jsonl | north incident-mine (crates/north-ops/src/ops/incident_mine) | crates/north-core/src/agent_config_check/rules.rs | read | |
| incidents (class, noticed_by, started_at, detected_at, fixed_at, guard, recurred_after_guard) | north ledger incident record from the lander (crates/north-landing/src/land/incidents.rs, every verdict but landed and every held candidate) and north land withdraw | crates/north-threads/src/threads/incidents/report.rs, printed by north ledger flow per 7 days and north ledger incident report | read | |
| incident_guards (class, guard, landed_at, issue) | north ledger incident guard | crates/north-threads/src/threads/incidents/mod.rs insert marks a later row of the class recurred_after_guard | read | |
| replay sweep-*.jsonl | north policy-replay sweep (crates/north-convo/src/policy_replay/sweep.rs) | crates/north-core/src/agent_config_check/gate.rs | read | |
| harness/model-catalog.toml | hand | crates/north-pace/src/usage/settings.rs CATALOG; crates/north-providers/src/providers/codex/runs/usage.rs run_usage; crates/north-providers/src/providers/claude/runs/telemetry.rs Prices | read | |
The definitions of the activity columns, attempt chains, rework and unjudged runs, with the dated measurements behind them, are in [learned routing and goal ETA measurements](rationale/learned-routing-and-goal-eta.md#signal-definitions-and-measurements-moved-from-the-feedback-loop).

## File signals

State files the loops above write, relative to `~/.local/state`. `north health` compares each file's mtime with its reader stamp (a file the reader rewrites after consuming it) and fails when the file is more than 7 days newer. `unverifiable` means the reader leaves no stamp; `north health` lists those rows rather than guessing, and the reader's owner adds a stamp to make one checkable. Each reader below touches its stamp through `platform::reader_stamp::touch` (crates/north-base/src/platform/reader_stamp.rs, 2026-11-05): the spawn gate (guard/spawn_capacity/usage.rs), `north fleet report` (ops/report/mod.rs), capacity sizing (capacity/host/database.rs recent_usage) and the usage reset reader (usage/web.rs). The writer and cadence columns feed the silent-signal detector of `north ledger flow detect`: a cadence such as `5m`, `1h` or `6h` is how often the writer should write, `event` means it writes only when something happens and is never judged silent, and the owner is the issue its box is filed on.

| signal file | reader stamp | writer | cadence | owner |
|---|---|---|---|---|
| `agents/usage-gate.json` | `north/reader-stamps/agents/usage-gate.json` | crates/north-pace/src/usage/gate.rs | 5m | north#35 |
| `agents/spawn-gate-denials.jsonl` | `north/reader-stamps/agents/spawn-gate-denials.jsonl` | crates/north-guard/src/guard/spawn_capacity/logs.rs | event | north#35 |
| `agents/goal-scores.jsonl` | `north/reader-stamps/agents/goal-scores.jsonl` | crates/north-flow/src/agents/goal/score.rs | event | north#35 |
| `agents/machine-capacity-usage.jsonl` | `north/reader-stamps/agents/machine-capacity-usage.jsonl` | crates/north-capacity-host/src/capacity/host/database.rs | event | north#35 |
| `agents/usage/claude-web.json` | `north/reader-stamps/agents/usage/claude-web.json` | crates/north-timers/src/usage_web_read | 6h | north#35 |

## Recorded readings

Rows a writer records into a database, judged like the file signals above. The database `ledger` is the threads ledger of the host (`threads.db`, or its snapshot on a mirrored host); the query returns the newest write time. A host that reads a mirrored snapshot of the ledger judges each reading as of 15 minutes before the snapshot's pull, not the present, because the snapshot is pulled about every 15 minutes from an image the ledger host may have built 15 minutes earlier, and a pull of 2026-11-14 held rows no newer than 15 minutes before it, which made a 5-minute writer that wrote every minute look silent (north#35, 2026-11-14).

| reading | database | query | writer | cadence | owner |
|---|---|---|---|---|---|
| usage header rows | `ledger` | `SELECT max(ts) FROM usage WHERE source LIKE 'Messages response%'` | crates/north-providers/src/providers/claude/token_quota.rs | 5m | north#35 |
| concern samples of the goal tick | `ledger` | `SELECT max(at) FROM concern_samples WHERE sampler = 'concerns-tick'` | crates/north-concerns/src/tick.rs | 10m | north#709 |

See [learned routing and goal ETA measurements](rationale/learned-routing-and-goal-eta.md) for the dated routing backtest and the goal ETA replay.

Every mechanism that wakes an agent turn, its 7-day wake and no-op counts, and the cut to one event path and one silence check: [wake paths](rationale/wake-paths.md) (2026-11-04).

What the measurements above found no gain from, and what still runs it: [measured no gain](rationale/measured-no-gain.md) (2026-11-04).
