# Spend rule: one reserve, one concurrency cap, one tripwire

Decided 2026-11-07 under north#702: north spends account capacity as fast as useful work allows, and only three
rules may stop a new worker. Before this, about 35 files read pace state and each derived its own limit from a
usage projection; the gates disagreed, and on 2026-11-07 dispatch saw 18 ready boxes and 10 free slots and
launched none. This note states the rule, names every gate that existed on that date, and gives each one its
fate.

## The rule

One module, `north_pace::spend`, holds the rule. Dispatch, `north fleet launch`, the spawn hook and
follows-release call it, and nothing else may refuse a worker on usage grounds. A refusal names the rule that
fired: `spend reserve`, `spend cap`, `spend tripwire`, `provider limit` or `no usage reading`.

1. **Pooled reserve.** One reserve covers all accounts together. The pool is the sum, over the accounts
   spend counts, of each account's unspent share of its week (all of it once the week has reset), scaled by
   its plan's size from the model catalog's weekly multipliers. The reserve is
   `spend_reserve` percent of the largest counted account's week, in the same units, so it means "one
   account's tenth" whatever the plans are. A worker starts only if the pool, less `spend_worker_charge` for
   it and for each worker this run has already placed, stays above the reserve. Workers may drain any single
   account to the provider's limit. An account that refills first returns its week to the pool. The proxy,
   scheduled routines (a `[routine:...]` brief) and Tom's own sessions are not held by the reserve and go to
   whichever account still has headroom.
2. **Concurrency cap.** At most `spend_worker_cap` (12) worker sessions may be active at once. Active means a
   fleet session in status `starting` or `working`, other than the proxy and adopted windows, counted once per
   name across every registry the caller surveys (dispatch and `north fleet launch` survey every host's
   `north fleet list --json`), plus this run's own placements. An idle or parked session spends nothing and
   lands nothing, so it does not count. A survey is read before its run launches, so two spawners on two
   hosts could both pass it: a worker launch therefore also carries the cap to the ledger host, which counts
   the fleet table's starting and working rows (less the proxy and adopted windows) in the same transaction
   that registers the name, and refuses it there with `spend cap`. The same registration counts the rows on
   the machine the worker runs on against that machine's `resident_cap` and refuses with `resident cap`.
   The cap is set from the hosts' resident capacity and tuned from
   measured landing contention (conflicts, slot waits, rework), never from usage.
3. **Tripwire.** No new worker starts on an account while either holds:
   - the account's five-hour window rose more than `spend_trip_fleet_points` within `spend_trip_fleet_min`
     minutes (readings that share one reset);
   - one session spent more than `spend_trip_session_points` of that window within `spend_trip_session_min`
     minutes.

   The proxy is told once per trip, with the top spenders. Running sessions continue. A trip lapses once no
   tick confirms it within the longer of the two spans. Trip events go to `agents/spend/trips.jsonl` so a week
   of them can tune the thresholds.

The provider's own limit is not a north rule. An account at the end of either window cannot run anything, so
placement skips it and the refusal names `provider limit`. Besides the reserve, that is the only usage stop.
The money refusal and its ceiling stay; they are cost policy, not pacing.

Nothing else refuses: no burn projection, no week spread over the hours to reset, no pace line, no hold,
grant, reservation or per-account `allowed_workers`, no stale-sample hold, and no requirement that a named
account match placement's first pick.

**Readings.** Spend reads the last sampled used percent of each account's five-hour and week windows from
`usage-gate.json`. A window whose reset has passed reads 0. An account counts toward the pool only while a
host is signed in to it and its reading is younger than `spend_reading_max_h`; an older or signed-out
account adds nothing, so a dead account cannot hold the pool open. A stale reading still lets its account
start work under the pool the fresh accounts give, because the provider's limit stops it when it runs out.
An account with no reading at all starts nothing.

**Charges between readings.** A run charges each of its own placements against the pool. Starts by other
spawners between two samples are not charged; the sampler reads every few minutes, so at most a few starts
fall in that gap, and the reserve absorbs them.

**Order.** Rules decide whether a worker may start; order decides only which account takes it. Placement
puts the soonest-expiring week room first (`room::Rank`: room per plan unit per hour to reset, from the same
readings, less this run's charges), so one dispatch run spreads its launches. Order never refuses.

**Providers.** The pool, the reserve and the tripwire cover Claude accounts. Other providers have no plan
accounts in use; a launch on one meets the cap and its provider's limit.

**Agent-tool subagents** run inside their parent's session and lane, so they do not count against the cap.
The spawn hook applies the reserve and the tripwire to the account the parent bills.

**Handover.** A running session whose account reaches the provider's limit is handed to the account in room
order that spend admits as a reserved starter. It adds no session, so the cap and reserve do not hold it.

**Where the tripwire runs.** In `north fleet tick` on every host whose copy of the usage table holds a
five-hour reading from the last 30 minutes, beside the spend breaker, so each host's spawners read trips
judged on that host. The fleet burst reads the table's five-hour readings, which every host shares. The
session burst reads the spend so far of each session that host runs, which the tick records per session in
`agents/spend/spenders.json`, priced in the account's five-hour points by the calibrated points per dollar
(a session burst is not judged on an account with no calibrated rate). Current trips go to
`agents/spend/trips.json` there, where dispatch, fleet launch and the spawn hook on that host read them, and
each new one is appended to `agents/spend/trips.jsonl`. Only the host that holds the proxy tells it, so a
fleet burst every host sees reaches the proxy once. `north spend [--json]` prints the view.

**Follows-release.** A released follower is a new worker, so release asks `spend::admit`. A cap or tripwire
refusal leaves the follower waiting for the next release pass rather than failing it.

## Every gate on 2026-11-07, and its fate

`north-pace`, `pace` module:
- `control.rs` (`advance`: `allowed_workers`, `held`, `over`, the burn rates per worker-minute, the
  PACE-HELD and PACE-RESUMED phone notices): **deleted**.
- `window.rs`, `trail.rs` and `plan_stop` (projected percent, horizon, ceiling, burn trail): **deleted**.
- `projection.rs` (`project`, `week_refusal`) and `north tune`'s projection scoring, along with the launch
  record's `projection` field: **deleted**.
- `exhaustion.rs` (the five-hour stop short of the provider's, the five-hour burn cap, the exhausted week):
  **deleted**. The provider limit, rule 1 and rule 3 replace it.
- `admission.rs`: the `refusal` "Hold:" for a stale sample or exhaustion is **deleted**. `interval`, the
  sampler's read cadence, is not a gate; it **moves** into the sampler.
- `target.rs` (spend targets, `[pace."ACCOUNT"] spend_to_pct` by `by`): **deleted**. `seconds` and
  `policy_path` **move** to `north-base`.
- `limits.rs`: pace knobs **deleted**; `default`, `prior_measured` and `overtime_min` **move** with the
  harness defaults reader.
- `gate_order.rs` and the `place_spawn` account order: **deleted**.
- `pace::answer` ops (`advance`, `fresh`, `refusal`, `interval`, `target_for`): **deleted**.

`north-pace`, `router` (`north agents plan`):
- Pace lines (`under pace` and `at or over pace`, `enforce_from`, `pace_band`, `pace_target_left_claude`,
  drain grants, the `under_pace` op field) and the `pace_warning` they produce: **deleted**. The router picks
  among the accounts spend admits, in room order.
- `launch_refusal` (the exhaustion rule): **replaced** by `spend`.
- `money_refusal` and `money_ceiling`: **kept** as cost policy. The branch of `billing` that treats a stale
  snapshot as money-billed is a stale-sample hold and is **deleted**; a stale reading is used as it stands.
- Fast mode (`fast`, `fast_floor`): **kept**. It picks a billing mode for a running lead and starts nothing.
- The `pace` and `paces` readings it writes into `usage-gate.json`: **deleted**.

`north-pace`, `usage`:
- `usage-gate.json`: `controls`, `projections`, `projected_pct`, `quota_ceiling` and `burn_pct_h` are
  **deleted**. It keeps the money refusals and each account's sampled windows (used percent, reset, read
  time) under a `windows` key, which are spend's readings.
- `decide::{place, free, rank, explain}` and `north usage decide --dry-run` (a second placement with its own
  reserve tiers): **deleted**. The world types and `assess`, which hand a session over once its account
  reaches the provider's limit, are **kept**; `hand_over_target` picks its account as **Handover** says.
- Spend breaker: its start refusal for a tripped workstream (`place`'s trip check and the `tripped:` line in
  `north ledger worker --recommend`) is **deleted**. Its budget notices to running sessions are **kept**,
  since they stop no start.

`north-dispatch`, `pacing`:
- `sampling.rs`: `update_controls`, the controller, phone notices and grant retention are **deleted**.
  Reading, recording and cadence are **kept**. Its per-account history line becomes the account's spend
  verdict and active sessions, which the scoreboard's idle stretch reads.
- `grants.rs` (`north pace grant`, `grant_for`, `confirm`, `expired_grant_hold`): **deleted**.
- `reservation.rs` (spawn reservations and their TTL): **deleted**.
- `placement/gate.rs` `Verdict.available` and `exhausted` (the exhaustion rule with burn cap): **replaced**
  by `spend`. `roomiest_here` reads spend's provider limit only.
- `placement/room.rs`: `Room::week_short` (the box reserve), `Room::takes`, the spend-target `spending`
  rank and its reads of `exhaustion` and `controls` are **deleted**. `Rank` and `Charge` are **kept**, as
  order only, reading spend's windows with the provider's limit as the ceiling.
- `placement/survey.rs` and `placement/tiers.rs` (`placement_drain_margin`): **deleted**.
- The placement `Case.pace` field (in `north-strategy-types`) is **deleted**. `north fleet launch` applies the
  spend rule beside placement, reading the same view as dispatch (`dispatch::spend_now`).
- `north fleet launch` `account_refusal` (a named account other than placement's first pick needs `--pin`,
  `TIE_WEEK_POINTS`): **deleted**. `exhausted_refusal` is **replaced** by the spend refusal, which no
  `--pin` passes.

`north-dispatch`, `dispatch`:
- `plan.rs` (`takes`, `burned`, `burn_line`, `pace_held`, `week_line`, `burn_held`, the launch
  `projection`, the `pin` for a rank that disagrees with placement): **replaced** by `spend::admit` with
  this run's placed workers and charges.
- `slots.rs` (`Account.workers` and `Pools.workers`, from `allowed_workers`): **deleted**.
- `survey.rs` (the 15-minute pace-control staleness refusal, `account_refusal` with the week box reserve,
  `pace_workers`, `five_hour_burn`, `readings`): **replaced** by the spend view.

`north-guard`, spawn hook (`spawn_capacity`):
- The usage-freshness "Hold:" with its forced refresh, `pace_hold`, grants and the `reserve` op: **replaced**
  by `spend::admit` (reserve and tripwire) on the billed account.
- The money refusal, the flow gate and the CPU capacity refusal: **kept**. They concern cost, landing
  flow and machine capacity, not usage pace.
- `gate_summary` (`north hook spawn-gate`): **replaced** by the spend view line.

`north-replay`:
- The placement `quota-floor` invariant (`replay_five_hour_floor_points`, `replay_week_floor_points`), which
  replays the deleted `controls`: **replaced** by an invariant that no replayed placement lands on an account
  at its provider's limit. The GitHub floor is **kept**.

Other readers:
- `north-core` `grounding/live.rs` (the `allowed_workers` line in a session's grounding): **replaced** by
  the spend view line.
- `north-health` pace freshness and `ops/flow/sampler.rs` (failing provider reads): **kept**, reading the
  sampler's state file.
- `scoreboard/ready.rs` (an idle account read as `allowed_workers > 0 && !held && !over`): **replaced** by
  "spend admits and no session is active on it".
- `follows-release`: **replaced** by `spend::admit`, as **Follows-release** says.
- `threads/runs/hook_rules.rs` (classifying spawn-hook denials by `Hold:`): **updated** to the rule names.
- `harness/defaults.toml`: `pace_*`, `plan_stop`, `five_hour_burn_cap_pct_h`, `claude_session_target`,
  `spawn_reservation_ttl_s`, `grant_reservation_hold_s`, `placement_drain_margin`,
  `reservation_fallback_points` and the replay quota floors are **deleted**. `spend_reserve`,
  `spend_worker_charge`, `spend_worker_cap`, `spend_reading_max_h` and the four tripwire knobs are **added**.
- South's orchestration `[pace.*]` tables and the firn unit that described pace enforcement: **deleted** and
  **reworded** in their own repositories.

These are **not** usage gates and are **kept**: each host's `resident_cap` and saturation reading, the CPU
spawn gate, battery and playtest leases, the landing governor, and the cloud lane's session count. They bound
machines and landing contention. They do not bound account spend, and rule 2 does not replace them.

## Why this shape

Every deleted gate answered one question in its own way: will this account run out before its window
resets? The answers came from different projections, so they disagreed, and the most conservative one won.
The provider already enforces running out. North needs only to keep a little capacity for Tom and the proxy
(rule 1), to avoid starting more work than landing can absorb (rule 2), and to notice a runaway quickly
(rule 3). A per-account floor would strand a tenth of every account, so the reserve is pooled (Tom,
2026-11-07).

## Tier defaults (north#702, 2026-11-07)

Usage is the binding constraint, so a launch starts at the cheapest tier its category allows and escalation goes through worker-high or worker-xhigh after a failed attempt. `north dispatch` and `north fleet launch` both take the tier from `dispatch::tier::route_for`: Haiku for mechanical work (its retry on Sonnet), Opus for debugging-unknown-cause, netcode-determinism, performance, planning and review, and sonnet-high for art and every other category. The Sonnet-against-Opus arm draw, north tune's `route CATEGORY` cells and the flow alert on alternating tier arms are deleted. Both paths refuse a `Category:` outside the brief vocabulary and name the valid list, because unknown words fell through to Opus. When a dispatched worker reports `Not done:` with `Stop: reasoning`, `dispatch::tier::route` relaunches the box one rung up the ladder of docs/rationale/nearest-routing.md section 5 with `Follows:` naming the session and its report as the first `Prior report:`; other stops keep the rung.

The before counts by category and tier, and the ledger cost per closed box behind this choice, are private usage figures kept in south (agents/decisions/tier-defaults-2026-11-07.md). The ledger holds no per-run share of a five-hour window, so finished boxes per percent of a window is not readable from it yet.
