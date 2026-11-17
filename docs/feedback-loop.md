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
| runs.repeats | none | none | dropped | 4aecdaf9 |
| runs.role reviewer, judge | crates/north-base/src/platform/run_roles.rs ingest_role (stored at ingest for a finished worker; ROLE_SQL still reads rows stored before 2026-11-04) | calibration, ledger/closed_source.rs | read | |
| runs.note, report, session, role | crates/north-threads/src/threads/runs | `threads show`, role filters | read | |
| run_usage | crates/north-threads/src/threads/runs | crates/north-threads/src/threads/runs/costs/mod.rs allocate into run_costs | read | |
| run_costs | crates/north-threads/src/threads/runs | crates/north-threads/src/threads/runs/costs/mod.rs rolls into runs.cost_usd | read | |
| usage | crates/north-pace/src/usage/refresh/ | crates/north-pace/src/usage/store.rs latest, routing and the pace gate | read | |
| claims.eta_min | crates/north-threads/src/threads/claims/ledger.rs take | crates/north-threads/src/threads/claims/mod.rs stale rule, verbs.rs OVERDUE; src/cli/work_items.rs:155 | read | |
| events kind=run | none | none | dropped | 4aecdaf9 |
| run_events | none | none | dropped | 4aecdaf9 |
| usage log cores, memory, cwd | crates/north-capacity-host/src/capacity/host/run/outcome.rs record | crates/north-capacity-host/src/capacity/host/estimate.rs estimate_capacity through crates/north-capacity-host/src/capacity/host/usage_log.rs history | read | |
| usage log wallSeconds | crates/north-capacity-host/src/capacity/host/run/outcome.rs usage_entry | crates/north-capacity-host/src/capacity/host/estimate.rs duration p75 | read | 4aecdaf9 |
| usage log io, diskBusyShare | crates/north-capacity-host/src/capacity/host/run/outcome.rs record | crates/north-capacity-host/src/capacity/host/estimate.rs estimate output | read | |
| usage log exitCode | crates/north-capacity-host/src/capacity/host/run/outcome.rs usage_entry | crates/north-capacity-host/src/capacity/host/estimate.rs duration p75 counts only exit 0 | read | 99f4618d |
| usage log command, censored | crates/north-capacity-host/src/capacity/host/run/outcome.rs usage_entry | crates/north-capacity-host/src/capacity/host/usage_log.rs history signs a run by its command; crates/north-capacity-host/src/capacity/host/sizing.rs censored_floor raises a censored command's memory charge | read | |
| job_runs | crates/north-threads/src/threads/runs | crates/north-threads/src/threads/jobs/mod.rs job_runs | read | |
| machine-capacity-usage.jsonl | crates/north-capacity-host/src/capacity/host/usage_log.rs record_usage | crates/north-capacity-host/src/capacity/host/sizing.rs size_from_usage, charge_from_usage | read | |
| spawn-queue.json | none (harness/lib/spawn.py and spawn_queue.py no longer exist on main) | none | dropped | |
| land-checks.tsv | none | none | dropped | 4aecdaf9 |
| S:costs.toml | hand | crates/north-pace/src/usage/settings.rs services | read | |
| costs/*.json | crates/north-pace/src/usage/refresh/digitalocean.rs | crates/north-pace/src/usage/settings.rs service lines | read | |
| incidents (class, noticed_by, started_at, detected_at, fixed_at, guard, recurred_after_guard) | north ledger incident record from the lander (crates/north-landing/src/land/incidents.rs, every verdict but landed and every held candidate) and north land withdraw | crates/north-threads/src/threads/incidents/report.rs, printed by north ledger flow per 7 days and north ledger incident report | read | |
| incident_guards (class, guard, landed_at, issue) | north ledger incident guard | crates/north-threads/src/threads/incidents/mod.rs insert marks a later row of the class recurred_after_guard | read | |
| harness/model-catalog.toml | hand | crates/north-pace/src/usage/settings.rs CATALOG | read | |
The definitions of the activity columns, attempt chains, rework and unjudged runs, with the dated measurements behind them, are in [learned routing and goal ETA measurements](rationale/learned-routing-and-goal-eta.md#signal-definitions-and-measurements-moved-from-the-feedback-loop).

## File signals

State files the loops above write, relative to `~/.local/state`. `north health` compares each file's mtime with its reader stamp (a file the reader rewrites after consuming it) and fails when the file is more than 7 days newer. `unverifiable` means the reader leaves no stamp; `north health` lists those rows rather than guessing, and the reader's owner adds a stamp to make one checkable. Each reader below touches its stamp through `platform::reader_stamp::touch` (crates/north-base/src/platform/reader_stamp.rs, 2026-11-05): the spawn gate (guard/spawn_capacity/usage.rs), `north fleet report` (ops/report/mod.rs), capacity sizing (capacity/host/usage_log.rs recent_usage and history) and the usage reset reader (usage/web.rs). The writer and cadence columns feed the silent-signal detector of `north ledger flow detect`: a cadence such as `5m`, `1h` or `6h` is how often the writer should write, `event` means it writes only when something happens and is never judged silent, and the owner is the issue its box is filed on.

| signal file | reader stamp | writer | cadence | owner |
|---|---|---|---|---|
| `agents/usage-gate.json` | `north/reader-stamps/agents/usage-gate.json` | crates/north-pace/src/usage/gate.rs | 5m | north#35 |
| `agents/machine-capacity-usage.jsonl` | `north/reader-stamps/agents/machine-capacity-usage.jsonl` | crates/north-capacity-host/src/capacity/host/usage_log.rs | event | north#35 |

## Recorded readings

Rows a writer records into a database, judged like the file signals above. The database `ledger` is the threads ledger of the host (`threads.db`, or its snapshot on a mirrored host); the query returns the newest write time. A host that reads a mirrored snapshot of the ledger judges each reading as of 15 minutes before the snapshot's pull, not the present, because the snapshot is pulled about every 15 minutes from an image the ledger host may have built 15 minutes earlier, and a pull of 2026-11-14 held rows no newer than 15 minutes before it, which made a 5-minute writer that wrote every minute look silent (north#35, 2026-11-14).

| reading | database | query | writer | cadence | owner |
|---|---|---|---|---|---|
| concern samples of the goal tick | `ledger` | `SELECT max(at) FROM concern_samples WHERE sampler = 'concerns-tick'` | crates/north-concerns/src/tick.rs | 10m | north#709 |

See [learned routing and goal ETA measurements](rationale/learned-routing-and-goal-eta.md) for the dated routing backtest and the goal ETA replay.

Every mechanism that wakes an agent turn, its 7-day wake and no-op counts, and the cut to one event path and one silence check: [wake paths](rationale/wake-paths.md) (2026-11-04).

What the measurements above found no gain from, and what still runs it: [measured no gain](rationale/measured-no-gain.md) (2026-11-04).
