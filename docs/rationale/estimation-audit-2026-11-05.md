# Estimation audit, 2026-11-05: what north predicts about work, and whether it learns

Tom asked on 2026-11-05 for an audit of the estimates north makes about work: which model and effort can do which category, how long a box takes, what it costs, what CPU and memory a command needs, how long a check may run, when a goal finishes, and how much account room a launch needs. Each should rest on measured outcomes, be checked against what happened, and correct itself without a human edit. Owner: north#255.

## Method and data

All figures cover the 7 days to 11:50 AM +07 on 2026-11-05. They were read from read-only copies, with no builds and nothing written to live state:

- **Ledger:** `~/.local/state/threads/snapshot.db`, copied to `/tmp`. Its newest run ends at 2026-11-05T04:50Z and its newest `calibration` row is at 04:12Z, because the ledger host was down after that. The window is 2,210 worker runs.
- **Capacity:** `~/.local/state/agents/machine-capacity/capacity.db`, copied. It holds 5,405 command runs.
- **Land-check timings:** `~/.local/state/safe-push/check-timings/`, holding 1,416 timed check runs.
- **Usage:** the ledger's `usage` table, 124,452 window readings.

Field coverage over those runs:

- model and effort 100%
- cost 99%
- wall minutes 83%
- ETA 75%
- landed sha 52% (6 of 40 sampled are wrong, north#567)
- turns, tool calls and active minutes 27%
- brief text 21%

A calibration figure here pairs each estimate with the estimate north had *before* the outcome (walk-forward), unless it says otherwise.

## Summary

Each item gives the loop status, then the measured result.

- **Tier success per category cell:** in a loop. It has no skill: its Brier score of 0.271 is worse than the 0.248 from always predicting the 46% base rate (n = 1,828). Dispatch does not route on it.
- **Box ETA:** in a loop, but not scaled. 56% of runs finish inside the ETA, which is meant as a p75. The actual/ETA ratio has a p90 of 4.3x (n = 1,422).
- **Expected spend (Budget line):** the median moves with the ledger, but the 2x multiplier is a constant. It is not stored and has never been measured (n = 0).
- **Per-run cost prediction (`recommend_usd`):** in a loop. The actual/predicted ratio has a median of 1.06 and a p75 of 2.60 (n = 1,959).
- **Capacity: cores, memory, duration:** in a loop. 79% of runs finish inside the predicted p90 wall time and 88% stay inside the predicted memory, but only 13% of runs carry a prediction (592 of 5,405 scored).
- **Land-check deadline:** in a loop on passes. 95% of checks finish inside their pass p90 and 99.9% inside the deadline, with 4 timeouts (n = 1,416).
- **Goal ETA:** never scored. There was 1 prediction in 7 days and no `goal_actual` row exists, ever.
- **Placement headroom (burn):** the burn rate updates from readings but its projection is never scored. Over 1 hour it misses by 2.6 points on average and lands within ±10 points 93.5% of the time (n = 2,141). The weekly projection never refuses a launch, and all four accounts reached 100% of their week.

## Tier success per category cell

**Inputs.** First runs (`follows IS NULL`, not split) by category and tier. A run succeeds when it was done, was not followed by another run, and landed if it was expected to land (`threads/calibration/read.rs:74`). There are two consumers:

- `threads recommend` shrinks each tier's rate toward the category rate with 15 pseudo-runs: `(ok + 15·pooled)/(n + 15)` (`recommend.rs:51`). It backs off across the facets until a tier has 5 runs, then picks the cheapest tier above 80% or the incumbent (`recommend.rs:88-142`). It writes only the chosen tier into `tier_choice`.
- Dispatch does not route on these rates.
  - It applies fixed rules first: art goes to Sonnet, mechanical to Haiku, and any category outside the five armed ones to Opus.
  - For the armed categories, it alternates Sonnet-high and Opus-medium until each arm holds 15 arm runs and 15 closed arm boxes.
  - It then keeps the arm with the lower cost per closed, unreworked box (`ops/dispatch/tier.rs:120-176,256-284`).
  - A `north tune` route cell under 36 hours old overrides both (`tuned.rs:7-16`).

**Updates.** `recommend` recomputes on every call, and dispatch calls it at each survey. `threads calibrate --write` stores a `tier` row (raw rate and $ per success per category and tier) about every hour: 172 snapshots in the window.

**Calibration.** I joined each first run to the newest stored `tier` success row for its category and tier from before the run started (n = 1,828 of 1,844):

- predicted 0-20%: 110 runs, observed 39%
- predicted 20-40%: 662 runs, observed 39%
- predicted 40-60%: 612 runs, observed 45%
- predicted 60-80%: 214 runs, observed 64%
- predicted 80-100%: 230 runs, predicted mean 89%, observed 51%

The Brier score is 0.271. Always predicting the 46% base rate would score 0.248. A success estimate that does no better than the base rate cannot tell a competent tier from an incompetent one. The 80-100% band is mostly small cells at full success. The 15-run shrinkage in `recommend` reduces that problem, but the stored rows and the "why" text keep the raw rate.

**Self-correction.** Yes, in the sense that new runs move the rate with no human edit. Nothing scores the rate as a prediction, though. No predicted probability is stored per run, and no check would notice that the estimator has no skill.

**Worst failure.** A cell with few, recent successes reads 100%. Example: `balance-tuning claude:opus-medium success` stored 100 at n = 1 on 2026-11-05. `recommend` then names a tier on a figure with no predictive value. `tier_choice` also holds `codex:luna-low` for mechanical (since 2026-11-05) and `codex:sol-medium` for tooling, against the Claude-only order. Dispatch does not read `tier_choice`, so this has not misrouted a launch, but `recommend` prints those Codex tiers as picks.

## Box ETA

**Inputs.** The brief line `Ledger ETA: N min (p75 of n=…)` is the p75 of wall minutes (ended − started) for runs on items closed in the last 7 days. Runs are pooled by category and model family (`ledger/closed.rs:233-245`, `ops/dispatch/grounds.rs:295-392`). There is no n floor. Without data the line falls back to the issue's declared ETA, else to `45 min (uncalibrated)`. The value reaches `runs.eta_min` through a regex that takes the first `ETA` in the brief (`providers/claude/runs/row.rs:11-63`), so a declared ETA wins over the ledger ETA.

**Updates.** Recomputed for every brief at dispatch and at launch.

**Calibration.** n = 1,422 runs with both an ETA and wall minutes.

Actual/ETA ratio:

- p10: 0.13
- p50: 0.83
- p75: 1.91
- p90: 4.29

Share of runs:

- inside the ETA: 56%, against 75% for a p75
- within 2x: 76%
- within 0.5-2x: 41%

Share inside the ETA, by category:

- feature: 57%, n = 550
- tooling: 50%, n = 303
- bug-known-cause: 49%, n = 174
- debugging-unknown-cause: 46%, n = 125
- research: 81%, n = 53
- docs-policy: 81%, n = 32

Against active minutes the ETA is about six times too long. The active/ETA ratio has a median of 0.17 and a p90 of 0.49 (n = 549). `north worker-ledger --eta-calibration` shows the same thing: the median |ln error| is 1.0-2.4 per cell, and 0-40% of runs fall within 0.5-2x. The ETA is a wall estimate, and wall time is mostly waiting.

**Self-correction.** Partly.

- New closed boxes move the p75 with no edit.
- `threads calibrate` stores an `eta_pred` scale: the walk-forward actual/predicted ratio at p75, 1.70 at n = 1,611. Only `threads recommend` applies it (`verbs.rs:159`).
- The brief ETA, which is the one that reaches workers and placement, is never scaled.

**Worst failure.** Tooling and bug runs exceed their ETA half the time, and 1 in 10 runs takes more than 4.3 times it. Placement charges an account `burn × ETA` (`ops/dispatch/plan.rs:70-78`), so an ETA that is short by 4x admits a launch the window cannot carry.

## Expected spend E (the Budget line)

**Inputs.** The Budget amount is `BUDGET_TIMES = 2.0` times the cell's median API-equivalent dollars per closed box over 7 days, priced from tokens with `router/prices.json`. Its floor is the median closed box across all cells (`ops/dispatch/grounds.rs:296,363-374`, `ledger/closed.rs:132-159,226-229`).

**Updates.** Recomputed for every brief. The median follows the ledger. The 2x multiplier is a hand-set constant.

**Calibration.** None is possible from the record. The budget is not stored in any table, only in brief text. None of the 21% of runs that carry brief text had the API-equivalent Budget line in the window, so n = 0. The nearest measured figure is the per-run cost prediction of `recommend_usd`: the walk-forward actual/predicted ratio has a p25 of 0.38, a median of 1.06 and a p75 of 2.60 (n = 1,959). It is centred, but a quarter of runs cost more than 2.6 times the prediction.

**Self-correction.** The median does. Whether 2x is the right stop point is never checked.

**Worst failure.** When the multiplier is too tight, a box that is spending normally gets the `Split:` instruction mid-work. When it is too loose, a runaway box spends past twice the median with nothing reporting it. Neither case would show up anywhere.

## Capacity: cores, memory and duration per command

**Inputs.**

- **Live sizing data:** `capacity_runs` and `capacity_model` in `~/.local/state/agents/machine-capacity/capacity.db`, plus the usage JSONL.
- **Cores and memory:** the p90 of the newest 20 runs at the first of five match levels that has at least 3 runs, from exact command down to family (`capacity/host/estimate.rs:160-206`).
- **Duration:** the p75 of the newest 20 exact runs that exited 0. Below 3 such runs it reports "uncalibrated" (`estimate.rs:133-146`).
- **Charge:** memory is p90 × 1.2. Cores are p90, capped at 1.5x the declared figure (`policy.rs:173-189`).
- **Memory floor:** `capacity_model` sizes memory once a key has at least 5 runs. Memory-capped runs are censored and raise the floor to 1.5x their reservation (`model.rs:310-438`).

**Updates.** Recomputed on every admission. Each finished lease writes its outcome and updates the model in the same transaction (`run.rs:396-416`).

**Calibration.** 5,405 runs in the window.

- **Prediction coverage:** 693 runs (13%) carried a wall-time prediction and 667 carried a memory prediction. 297 had basis `unknown`; the rest had no basis at all.
- **Scored runs:** of the 592 successful runs with both predictions, 79% finished inside the predicted p90 wall time and 88% stayed inside the predicted memory.
- **Reservations, all runs:** they covered peak memory 80% of the time and p90 cores 95% of the time.
- **Drift:** `north capacity calibration --since 168h` flags DRIFT on the busiest keys.
  - `heavy (cargo test`: wall error p50/p90 of 84%/282%, memory error 60%/2622%.
  - `heavy bun test`: wall error 636%/4997%.
  - `heavy cargo build`: wall coverage 25%.

**Self-correction.** Yes for successful runs. There are gaps:

- Failed runs, 1,392 or 26%, teach the core and memory estimates but not duration or the model.
- A timeout never lengthens anything.
- DRIFT is report-only: nothing reads it to widen a margin or change the match level.

**Worst failure.** A key whose shape drifts keeps admitting at the old size. On `heavy (cargo test`, the memory p90 error is 26x, so admission either over-reserves and serialises the box, or under-reserves and the run stalls at `MemoryHigh`.

## Land-check deadlines

**Inputs.** The deadline is clamp(3 × p90 of the last 20 passing runs, 300 s, 2,100 s) per command and class. Below 5 passes it is 2,100 s (`check_timings.rs:7-40`). Passes go to `check-timings/passes/<key>.tsv`, and per-tree `finished`/`exceeded` lines go to `check-timings/<tree>/<key>.tsv`. Other check deadlines are constants: lander dispatch 180 s and queued 600 s, red-check offload 1,800 s, flake retry 120 s and mutant trees 1,800 s.

**Updates.** Computed fresh for each land check. Each pass appends its time.

**Calibration.** n = 1,416 tree-level outcomes, scored against the current pass p90 rather than walk-forward. 1,412 finished and 4 exceeded the deadline. 95.2% of finished checks fell inside the pass p90, against a 90% target, and 99.9% inside the deadline. The ledger's `land_checks` table has 42 checks since 2026-11-04: 39 passed, 3 failed, p50 3 s, p90 26 s, maximum 4.97 min.

**Self-correction.** Yes on passes. An `exceeded` run only decides whether a hang is the lane's fault or the infrastructure's, by retrying on the base tree. It never moves the deadline. `land_checks` records neither the deadline nor a timeout, so a timeout looks the same as a failure: `landings.detail` reads `failed /tmp/tmp.…` for 51 of the 62 failed landings.

**Worst failure.** A check that slows down steadily, for example through a growing test suite, is cut at 3 × the old p90 before enough slow passes move the p90. The lane then reads as failed, not slow.

## Goal ETA

**Inputs.** Each box is estimated at its category's p75 box-chain minutes (3 chains at least, else the all-category p75). That is multiplied by the `box` calibration median when its n is at least 200 and the row is under 7 days old. The whole goal is scheduled at the spawn gate's width (`threads/calibration/boxes.rs:103-112`, `box_eta.rs:62`, `ops/agents/goal/estimate.rs:57-63`). Predictions are stored as `calibration` rows of kind `goal_eta`, with `predicted_finish` in the source.

**Updates.** On `agents status` and plan runs. The `box` scale is refreshed with the hourly calibrate write: median 0.36 and p75 1.05 at n = 436 chains.

**Calibration.** None.

- There was 1 `goal_eta` row in 7 days (2026-11-04 00:53Z, 9 boxes, 28 min). It still carried the target `2026-10-16T14:00:00Z`.
- No `goal_actual` row exists in the ledger at all. The writer (`agents plan --score`, `goal/actual.rs:7-30`) fires only when every goal issue is closed, and no goal has closed since it landed.
- Per-box chain predictions are scored walk-forward (the `box` row): the actual chain takes a median of 0.36 times the prediction. The per-box ETA therefore runs about three times long.

**Self-correction.** The box scale would correct it, but the goal-level estimate has never been compared with an outcome.

**Worst failure.** A goal ETA is quoted against a stale target and never scored, so a goal that is days late reads as on time. The only prediction in the window was quoted against a target 19 days in the past.

## Placement headroom

**Inputs.**

- **Burn rate:** the five-hour usage percentage per account, from `usage` readings. Burn is the change in used percentage over the last 60 minutes (`pace/trail.rs:16-19`).
- **Refusal:** a launch is refused at 95% of the five-hour window, at a burn above 50%/h (the rule that the window must last two hours), or at 100% of the week (`exhaustion.rs:19-33`).
- **Dispatch:** an account takes a box when its remaining five-hour room exceeds `burn × box ETA` (`ops/dispatch/plan.rs:70-78`).
- **Weekly projection:** an "empty in" time (`usage/burn.rs:36-48`). It is display and soft pace only.

**Updates.** On every usage refresh and at every placement decision.

**Calibration.** Walk-forward over 5-minute buckets inside each five-hour window, projecting the used percentage 1 hour ahead from the last hour's burn (n = 2,141):

- mean absolute error 2.6 points
- 93.5% of projections within ±10 points
- actual at or below the projection 70% of the time

The 50%/h refusal never fired in the window; the highest hourly burn was under 50 points. Only account d reached 95% of a five-hour window, in 4 windows. Accounts a, b, c and d all reached 100% of their weekly window inside the 7 days.

**Self-correction.** The burn rate is live, so it corrects itself as readings come in. The projection is never stored at launch, and so is never compared with the window it was made about.

**Worst failure.** The binding limit was the week, and the weekly projection never refuses. Every account ran to 100% of its week with placement still ranking it. The five-hour check would also admit a box whose real duration is 4x its ETA, as described in the box ETA section.

## Boxes filed

Each estimator above that is uncalibrated, unmeasured or outside a loop now has a box on north#255 under "Estimation audit (2026-11-05)", each with a Done when:

- tier success scored as a prediction, and Codex tiers removed from `recommend`
- the brief ETA scaled by its own walk-forward calibration
- the Budget stored and scored
- goal ETA scored on every goal close, with a current target
- capacity DRIFT fed back into sizing
- land-check timeouts recorded and moving the deadline
- the weekly burn projection stored at launch and scored

The existing box for a standing estimate-against-actual report in `north tune` covers the daily report for ETA and cost.
