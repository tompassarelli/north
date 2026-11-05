# Feedback loop

Every recorded signal, who writes it and what reads it on main (north#35), and the experiments run on
those signals: the learned routing backtest and the goal ETA replay. Reader cells come from
`rg -n -g '!*.test.*'` on main (2026-10-12); schema lines and the writer itself don't count. S: is the
operator directory in south, mc/ is `harness/skills/machine-capacity/scripts`.

Every threshold and prior these loops apply lives in `harness/defaults.toml` with its value, source
(measured, a dated Tom decision, a named standard, or a prior with a `settles_when` count the weekly `north defaults-measure --priors` check reads) and thin-data fallback.
`--priors` prints `WAIT` below the count's `need`, `MEASURABLE-NOT-MEASURED` once the rows are there but nobody has measured
the value, and `SETTLED` only for a prior whose `settles_when.measured` records the dated sweep result that measured it;
enough rows alone never settle a prior. Consumers read each default
there and the `defaults_measure::lint` cargo test fails on a hard-coded value or a blank source. The weekly review
re-runs the measured sources with `north defaults-measure <copy of snapshot.db> <default>`, which
sweeps one default over a grid by walk-forward replay and prints n and the score per value.

| signal | writer | reader on main | verdict | commit |
|---|---|---|---|---|
| runs.eta_min | north worker-ledger | crates/north-threads/src/threads/calibration/forward.rs calibrate_eta scales recommend_eta | read | ac79788c |
| runs.minutes | north worker-ledger | crates/north-threads/src/threads/calibration/recommend.rs recommend_eta (the wall p75 printed after the active-minute figures) | read | |
| runs.outcome, follows, landed, expects_landing | north worker-ledger | crates/north-threads/src/threads/calibration/read.rs succeeded; crates/north-pace/src/usage/ctx.rs routing success | read | 4a58f21e |
| runs.tier | north worker-ledger | crates/north-threads/src/threads/calibration/recommend.rs recommend, every provider | read | 4b71ac63 |
| runs.category and facets | bin/threads brief_fields | crates/north-threads/src/threads/calibration/recommend.rs recommend, recommend_eta | read | |
| runs.arm | bin/threads:967 | crates/north-ops/src/ops/report/arms.rs, Wilson 95% per arm | read | lane H |
| runs.lens, flaws | north worker-ledger | crates/north-threads/src/threads/calibration/summary.rs lens table | read | |
| runs.repeats | none | none | dropped | 4aecdaf9 |
| runs.tokens | north worker-ledger | crates/north-threads/src/threads/calibration/summary.rs tokens_med, tokens_per_closed | read | |
| runs.cost_usd | bin/threads:989 | crates/north-threads/src/threads/calibration/recommend.rs recommend_usd median $ line (tier choice prices runs.tokens at API-equivalent rates); crates/north-ops/src/ops/agents/goal/metrics.rs delivery_metrics; crates/north-ops/src/ops/agents/goal/score.rs | read | 4b71ac63 |
| runs.staffed, review_requested, landed_at | north worker-ledger | crates/north-ops/src/ops/agents/goal/metrics.rs delivery_metrics | read | |
| runs.landed_sha | north worker-ledger | crates/north-core/src/ledger/outcome/read.rs staffed joins each worker run to its landed commit for `north flow`; crates/north-threads/src/threads/calibration/rework.rs landings judges the commit on main; crates/north-core/src/ledger/closed_source.rs reworked_agents feeds the cost per closed box | read | |
| landings.landed_sha, first_commit_at | north land-train record (the lander) | crates/north-core/src/ledger/outcome/read.rs landed, first-commit-to-main p50 in `north flow`; crates/north-threads/src/threads/landing/tempo.rs | read | |
| land_tempo (landed, queued->main and first commit->main p50 and p90, refused alone, conflicts, queued now; one row per repo, hour and window of 60 or 1440 minutes) | `north tempo --record` (crates/north-ops/src/ops/tempo/mod.rs over the lander's `land-queue/<key>/done/` tickets, hourly by the nixos-config north-tempo timer) | the Payback line of every harness box (harness/skills/workers/references/payback.md, Landing tempo): the 1440-minute first commit->main p50 is the harness outcome number, target 10 minutes, and a landing-path box prices its saving against it | read | |
| Tom's human turns (convo store index-v4.db, transcript origin human) | Claude Code transcripts, indexed by north convo | crates/north-ops/src/ops/agents/presence/live.rs typed_messages feeds `north flow` attention, corrections per active day and who noticed each incident first (crates/north-core/src/ledger/outcome/attention.rs) | read | |
| north/grounding/ledger.jsonl tom correction, tom direction | north grounding distill | crates/north-core/src/ledger/outcome/read.rs corrections, counted only when the row's convo message is one of Tom's human turns | read | |
| runs.first_commit | none (tally-internal, crates/north-core/src/providers/claude/runs/landing.rs tally) | none | dropped | 4aecdaf9 |
| runs.turns, tool_calls, active_min, wait_min | crates/north-core/src/providers/claude/runs/activity.rs (Claude workers; recorded by crates/north-threads/src/threads/runs/record.rs; `threads run-ids --settled` leaves out a Claude run with a null active_min so the next ingest re-reads its transcript and fills the four columns, and Codex and Gemini rows stay out of that re-read) | crates/north-threads/src/threads/calibration/recommend.rs recommend_eta: the `eta` line of `north worker-ledger --recommend` prints `eta active <median>m (p25 <a>–p90 <b>, n=<N>) wall p75 <W>m wait median <X>m` from the cell's runs that recorded active minutes (at least `eta_min_samples` of them), and `eta wall <W>m (p75 of n=<N>, ...; too few runs with active minutes)` otherwise; active minutes are work, the wall p75 includes the waits | read | |
| runs.skills (JSON list of the skills a run loaded by the Skill tool or the file-skill hook; null on rows recorded before 2026-11-05) | crates/north-core/src/providers/claude/runs/telemetry.rs skills (Claude; recorded by crates/north-threads/src/threads/runs/record.rs) | crates/north-dispatch/src/tune/skills.rs: `north tune` prints each skill's closed-box rate and cost per closed box over 14 days against same-category boxes that did not load it, and flags installed skills no run loaded and skills whose boxes closed 20 points less often or cost 25% more per closed box | read | |
| runs.peak_ctx, handoff_fires, handoff_ignored | crates/north-core/src/providers/claude/runs/telemetry.rs (Claude); crates/north-core/src/providers/codex/runs/usage.rs (Codex) | bin/capacity-watchdog:465 | read | |
| runs.compactions, input_tokens, cache_read_tokens, idle_gap_misses | crates/north-core/src/providers/claude/runs/telemetry.rs (Claude); crates/north-core/src/providers/codex/runs/usage.rs (Codex) | crates/north-threads/src/threads/calibration/summary.rs context_summary | read | |
| runs.provider, model, effort | crates/north-core/src/providers/claude/runs/mod.rs effort, telemetry.rs run_usage (Claude rows: the assistant message `model`; effort from the request's `output_config.effort` in the transcript, else the subagent's recorded effort, its harness/claude/agents/<type>.md `effort:`, its tier, a fleet session's launch effort, then the catalog default; provider `claude`); crates/north-core/src/fleet/watch.rs identify (fleet turn ledger runs.model, effort) | crates/north-threads/src/threads/runs/mod.rs run_ids --settled (a Claude row missing any of the three is re-read on the next ingest); tier routing and ETAs keyed on what ran | read | |
| runs.role reviewer, judge | crates/north-base/src/platform/run_roles.rs ingest_role (stored at ingest for a finished worker; ROLE_SQL still reads rows stored before 2026-11-04) | calibration, ledger/closed_source.rs | read | |
| runs.account, fast | bin/threads:902 | crates/north-threads/src/threads/calibration/summary.rs delivery_summary | read | |
| runs.note, report, session, role | bin/threads | `threads show`, role filters | read | |
| calibration | bin/threads calibrate | crates/north-threads/src/threads/calibration/forward.rs calibrate_eta; harness/lib/calibration.py:23 | read | ac79788c |
| run_usage | bin/threads:897 | crates/north-threads/src/threads/runs/costs.rs allocate into run_costs | read | |
| run_costs | bin/threads:987 | crates/north-threads/src/threads/runs/costs.rs rolls into runs.cost_usd | read | |
| usage | crates/north-pace/src/usage/refresh/ | crates/north-pace/src/usage/store.rs latest, routing and the pace gate | read | |
| claims.eta_min | crates/north-threads/src/threads/claims/ledger.rs take | crates/north-threads/src/threads/claims/mod.rs stale rule, verbs.rs OVERDUE; crates/north-threads/src/work_items.rs:82 | read | |
| tier_choice | bin/threads:1431 | crates/north-threads/src/threads/calibration/verbs.rs incumbent | read | |
| events kind=run | none | none | dropped | 4aecdaf9 |
| run_events | none | none | dropped | 4aecdaf9 |
| capacity_runs cores, memory, signature | crates/north-capacity-host/src/capacity/host/run.rs record | crates/north-capacity-host/src/capacity/host/estimate.rs estimate_capacity | read | |
| capacity_runs.wallSeconds | crates/north-capacity-host/src/capacity/host/run.rs usage_entry | crates/north-capacity-host/src/capacity/host/estimate.rs duration p75 | read | 4aecdaf9 |
| capacity_runs io, diskBusyShare | crates/north-capacity-host/src/capacity/host/run.rs record | crates/north-capacity-host/src/capacity/host/estimate.rs estimate output | read | |
| capacity_runs.exitCode | crates/north-capacity-host/src/capacity/host/run.rs usage_entry | crates/north-capacity-host/src/capacity/host/estimate.rs duration p75 counts only exit 0 | read | 99f4618d |
| capacity_runs.command, cpuSeconds | crates/north-capacity-host/src/capacity/host/run.rs usage_entry | crates/north-capacity-host/src/capacity/host/model.rs Subject::levels keys the model by command; update folds cpu_seconds into each level | read | |
| job_runs | bin/threads:1748 | crates/north-threads/src/threads/jobs/mod.rs job_runs; crates/north-runtime/src/job_watch/mod.rs | read | |
| model_events | crates/north-runtime/src/model_watch | crates/north-pace/src/usage/ctx.rs successors, routing hold | read | 4a58f21e |
| machine-capacity-usage.jsonl | crates/north-capacity-host/src/capacity/host/database.rs record_usage | crates/north-capacity-host/src/capacity/host/sizing.rs size_from_usage, charge_from_usage | read | |
| usage-gate.json | crates/north-pace/src/usage/gate.rs | crates/north-guard/src/guard/spawn_capacity/usage.rs | read | |
| spawn-gate-denials.jsonl | crates/north-guard/src/guard/spawn_capacity/logs.rs | crates/north-ops/src/ops/report/mod.rs jsonl | read | 871718bb |
| spawn-queue.json | none (harness/lib/spawn.py and spawn_queue.py no longer exist on main) | none | dropped | |
| land-checks.tsv | none | none | dropped | 4aecdaf9 |
| goal-scores.jsonl | crates/north-ops/src/ops/agents/goal/score.rs | crates/north-ops/src/ops/report/mod.rs jsonl | read | 871718bb |
| S:costs.toml | hand | crates/north-pace/src/usage/settings.rs services | read | |
| costs/*.json | crates/north-pace/src/usage/refresh/digitalocean.rs | crates/north-pace/src/usage/settings.rs service lines | read | |
| S:operating-point.toml | north policy-replay promote | crates/north-core/src/agent_config_check/gate.rs; crates/north-runtime/src/timers/skill_prune.rs | read | |
| injected context per session (startup, SKILL bodies, skill references, grounding, other hook context; grounding precision and notification-triggered injections), derived from Claude transcripts | Claude Code transcripts under ~/.local/state/north/profiles/claude-*/projects | crates/north-ops/src/ops/context_audit/scan.rs audit_file; `north context audit` measures the skill and hook slimming program | read | |
| S:incidents.jsonl | north incident-mine (crates/north-ops/src/ops/incident_mine) | crates/north-core/src/agent_config_check/rules.rs | read | |
| incidents (class, noticed_by, started_at, detected_at, fixed_at, guard, recurred_after_guard) | north incident record from the lander (crates/north-landing/src/land/incidents.rs, every verdict but landed and every held candidate) and north land withdraw; north incident import for backfills | crates/north-threads/src/threads/incidents/report.rs, printed by north flow per 7 days and north incident report | read | |
| incident_guards (class, guard, landed_at, issue) | north incident guard; north incident import | crates/north-threads/src/threads/incidents/mod.rs insert marks a later row of the class recurred_after_guard | read | |
| replay sweep-*.jsonl | north policy-replay sweep (crates/north-ops/src/ops/policy_replay/sweep.rs) | crates/north-core/src/agent_config_check/gate.rs | read | |
| harness/model-catalog.toml | hand | crates/north-pace/src/usage/settings.rs CATALOG; crates/north-core/src/providers/codex/runs/usage.rs run_usage; crates/north-core/src/providers/claude/runs/telemetry.rs Prices | read | |

The runs activity signal, per worker run from its transcript (north#255): turns = assistant messages;
tool_calls = tool_use blocks; active_min = the sum of gaps between consecutive transcript events inside an
assistant turn (model generation and tool execution), leaving out the gap from an assistant turn's end to the
next user, human or notice message, with any single gap capped at 10 minutes so a hung tool does not count as
work; wait_min = wall minutes minus active_min, where wall is the first to the last transcript timestamp.

Claude worker creation (north#35): foreground `SubagentStart` lacks the original prompt, and the child prompt is persisted only after that callback. The supported route is `Agent` with `run_in_background: true`: its `PostToolUse` result has `status=async_launched` and the actual child `agentId`, so `worker-handoff` records the original tool input at launch. `worker-tier-guard` requires this route before any `Follows:` exemption.

Attempt chains (2026-11-04): a Done run counts as kept only when no other worker run on its item was staffed within 24 h (`RERUN_WINDOW_MIN`, a constant apart from the 30-minute `box_chain_gap_min`) of its end, so `Follows:` words that never resolve no longer hide re-runs from `ok`, `success` and the recommend rates; `north worker-ledger --summary` adds an `attempts` column, the mean rank of a cell's runs in their item's chain (a chain breaks at a gap over 24 h). A reviewer (`Category: review`) or a 0-minute Done run with no known category (a judge or scorer) is read as role `reviewer` or `judge` by `run_roles::ROLE_SQL`, so it leaves the worker cells and appears in the context table under its own role.

Rework (2026-11-04): a Done run whose landing on main is reverted, fixed forward or named a red-main culprit within 48 h (`rework::REWORK_WINDOW_S`; definition in docs/rationale/tier-routing.md) is not kept either. History comes from `git log --first-parent origin/main` in each `~/code/<checkout>/main` (`NORTH_CODE_ROOT` overrides the root) and culprits from the red-main-notify state directory (`RED_MAIN_STATE`). `north worker-ledger --summary` adds a `rework` column, reworked landings and their share of the cell's landings found on main, and a `revert/fix/culprit` column splitting them by cause out of the landings found (2026-11-05), and `north health` prints `health: rework 7d:` with each repository's rate and fails above `rework_alarm_pct` (prior 20%).

Unjudged runs (north#35 FX5): an `unclear` run with no `Item:` is an unbriefed session and `ledger::outcome` (`unjudged`) keeps it out of every success denominator (370 of 533 `unclear` worker runs over 14 days); on the 2026-10-12 snapshot, `replay-staffing --since 7d` goes from 830 decisions, 146 changed, old pick 86 (39 ok) to 817, 132 changed, old pick 72 (35 ok), and 7-day `summary` ok moves unknown/sol-high 51% of 658 to 57% of 580, unknown/sol-medium 61% of 276 to 66% of 254, unknown/medium 45% of 87 to 67% of 59, tooling/medium 63% of 138 to 69% of 126, and unknown/luna-medium (0% of 13) and unknown/gemini-pro-high (0% of 17) drop out.

## File signals

State files the loops above write, relative to `~/.local/state`. `north health` compares each file's mtime with its reader stamp (a file the reader rewrites after consuming it) and fails when the file is more than 7 days newer. `unverifiable` means the reader leaves no stamp; `north health` lists those rows rather than guessing, and the reader's owner adds a stamp to make one checkable. Each reader below touches its stamp through `platform::reader_stamp::touch` (crates/north-base/src/platform/reader_stamp.rs, 2026-11-05): the spawn gate (guard/spawn_capacity/usage.rs), `north report` (ops/report/mod.rs), capacity sizing (capacity/host/database.rs recent_usage) and the usage reset reader (usage/web.rs). The writer and cadence columns feed the silent-signal detector of `north flow detect`: a cadence such as `5m`, `1h` or `6h` is how often the writer should write, `event` means it writes only when something happens and is never judged silent, and the owner is the issue its box is filed on.

| signal file | reader stamp | writer | cadence | owner |
|---|---|---|---|---|
| `agents/usage-gate.json` | `north/reader-stamps/agents/usage-gate.json` | crates/north-pace/src/usage/gate.rs | 5m | north#35 |
| `agents/spawn-gate-denials.jsonl` | `north/reader-stamps/agents/spawn-gate-denials.jsonl` | crates/north-guard/src/guard/spawn_capacity/logs.rs | event | north#35 |
| `agents/goal-scores.jsonl` | `north/reader-stamps/agents/goal-scores.jsonl` | crates/north-ops/src/ops/agents/goal/score.rs | event | north#35 |
| `agents/machine-capacity-usage.jsonl` | `north/reader-stamps/agents/machine-capacity-usage.jsonl` | crates/north-capacity-host/src/capacity/host/database.rs | event | north#35 |
| `agents/usage/claude-web.json` | `north/reader-stamps/agents/usage/claude-web.json` | crates/north-runtime/src/usage_web_read | 6h | north#35 |

## Recorded readings

Rows a writer records into a database, judged like the file signals above. The database `ledger` is the threads ledger of the host (`threads.db`, or its snapshot on a mirrored host); the query returns the newest write time.

| reading | database | query | writer | cadence | owner |
|---|---|---|---|---|---|
| usage header rows | `ledger` | `SELECT max(ts) FROM usage WHERE source LIKE 'Messages response%'` | crates/north-core/src/providers/claude/token_quota.rs | 5m | north#35 |

See [learned routing and goal ETA measurements](rationale/learned-routing-and-goal-eta.md) for the dated routing backtest and the goal ETA replay.

Every mechanism that wakes an agent turn, its 7-day wake and no-op counts, and the cut to one event path and one silence check: [wake paths](rationale/wake-paths.md) (2026-11-04).

What the measurements above found no gain from, and what still runs it: [measured no gain](rationale/measured-no-gain.md) (2026-11-04).
