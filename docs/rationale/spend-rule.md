# Spend rule: one reserve, one floor, one tripwire

Decided 2026-11-07 under north#702: north spends account capacity as fast as useful work allows, and only these
rules may stop a new worker. Before this, about 35 files read pace state and each derived its own limit from a
usage projection; the gates disagreed, and on 2026-11-07 dispatch saw 18 ready boxes and 10 free slots and
launched none. This note states the rule, names every gate that existed on that date, and gives each one its
fate.

## The rule

One module, `north_pace::spend`, holds the rule. Dispatch, `north fleet launch`, the spawn hook and
follows-release call it, and nothing else may refuse a worker on usage grounds. A refusal names the rule that
fired: `spend reserve`, `operational reserve`, `spend floor`, `spend tripwire`, `provider limit` or `no usage reading`.

1. **Pooled reserve.** One reserve covers all accounts together. The pool is the sum, over the accounts
   spend counts, of each account's unspent share of its week (all of it once the week has reset), scaled by
   its plan's size from the model catalog's weekly multipliers. The reserve is
   `spend_reserve` (5, restated 2026-11-09 under north#1000) percent of the combined week of all counted
   accounts, in the same units, so it means "a twentieth of everything" whatever the plans are. A worker starts only if the pool, less `spend_worker_charge` for
   it and for each worker this run has already placed, stays above the reserve. Workers may drain any single
   account to the operational floor (rule 3), and reserved starters to the provider's limit. An account that
   refills first returns its week to the pool. The proxy, scheduled routines (a `[routine:...]` brief) and
   Tom's own sessions are not held by the reserve and go to whichever account still has headroom.
   An account named in `[accounts.claude] reserve` (a plan account by label, an API organization as `api:ORG`) is the
   operational reserve (north#1049, north#1076): workers and subagents never start on it, placement and dispatch never
   offer it, its week stays out of the pool, and `north fleet accounts --plan` lists it as operational-only.
2. **Tripwire.** No new worker starts on an account while either holds:
   - the account's five-hour window rose more than `spend_trip_fleet_points` within `spend_trip_fleet_min`
     minutes (readings that share one reset);
   - one session spent more than `spend_trip_session_points` of that window within `spend_trip_session_min`
     minutes.
   - the circuit breaker: the rise of the last `spend_trip_fleet_min` minutes, held until the window resets, would take it to 100%.

   The proxy is told once per trip, with the top spenders. Running sessions continue. A trip lapses once no
   tick confirms it within the longer of the two spans. Trip events, and a `closed_at` row when a trip lapses, go to `agents/spend/trips.jsonl` so a week
   of them can tune the thresholds.
3. **Operational floor.** Each subscription account keeps `spend_floor_week` (5) percent of its week and
   `spend_floor_five_hour` (10) percent of its five-hour window for the privacy judge, the lander and the
   canaries (north#1049). A new worker refuses on `spend floor` once either window has that much or less left,
   so a busy fleet cannot spend the capacity the gates that let it land need. A reserved starter (the proxy,
   routines and Tom's own sessions) still passes inside the floor. The drain switch turns off the pooled
   reserve, not this floor.

The provider's own limit is not a north rule. An account at the end of either window cannot run anything, so
placement skips it and the refusal names `provider limit`.

**No per-account cover (2026-11-09, north#1000).** An earlier rule (north#259) refused a worker on an account whose
windows could not cover one worker run at its p90. It is deleted: a worker that reaches a provider limit
mid-run continues on another account in its own conversation (north#998), so admission does not predict
whether a session fits in one account. The reserve pools every account, the operational floor (rule 3,
north#1049) keeps the last 5% of a week and 10% of a five-hour window for operational callers, and the
provider limit is the only other account-level usage stop. The proxy, routines and Tom's sessions, and
Agent-tool subagents, meet only the provider limit.

**Drain.** `north fleet drain on [--until TIME]` turns the reserve off until `north fleet drain off` or the time
passes; every account may then be spent past the pooled reserve, to its provider limit unless it is inside the
operational floor. The tripwire and the floor stay. The switch lives in `spend/drain.json` in the agents
state directory of the host that admits the launch, and `north fleet usage` prints the mode on its first line. The proxy flips it only on Tom's own word to drain; without that word the
default reserve applies. A refusal reads "a new worker session would leave N points of week across all
accounts, not above the M-point reserve".
The money refusal and its ceiling stay; they are cost policy, not pacing.

Nothing else refuses: no burn projection, no week spread over the hours to reset, no pace line, no hold,
grant, reservation or per-account `allowed_workers` beyond the operational floor, no stale-sample hold, and no
requirement that a named account match placement's first pick.

**Readings.** Spend reads the last sampled used percent of each account's five-hour and week windows from
`usage-gate.json`. A week window whose reset has passed reads 0. A five-hour window with no reading since its reset is
unread, not 0: the provider stops sending it (a setup token's 429 on a full week carries only the 7d headers, and
accounts a and b got no 5h headers from 2026-10-31 while they worked), so spend stops nothing on it, the week
reading decides, and `north fleet accounts --plan` prints `5h unread` (north#872). An account counts toward the pool only while a
host is signed in to it and its reading is younger than `spend_reading_max_h`; an older or signed-out
account adds nothing, so a dead account cannot hold the pool open. A stale reading still lets its account
start work under the pool the fresh accounts give, because the provider's limit stops it when it runs out.
An account with no week reading starts nothing.

**Charges between readings.** A run charges each of its own placements against the pool. Starts by other
spawners between two samples are not charged; the sampler reads every few minutes, so at most a few starts
fall in that gap, and the reserve absorbs them.

**Order.** Rules decide whether a worker may start; order decides only which source takes it. Placement
ranks plan accounts and API grants earliest deadline first (`room::Rank`, north#723): first a source the fleet's
measured burn could not use up before its deadline, then the soonest deadline (a plan's weekly reset while its
week has room, a grant's expiry), then week room per plan unit per hour to reset, less this run's charges, then
five-hour room. `north fleet accounts --plan` prints the order with each source's reason. Order never refuses.

**Providers.** The pool, the reserve and the tripwire cover Claude accounts. Other providers have no plan
accounts in use; a launch on one meets its provider's limit.

**Agent-tool subagents** run inside their parent's session and lane, so they add no session.
The spawn hook applies the reserve and the tripwire to the account the parent bills.

**Handover.** A running session whose account reaches the provider's limit is handed to the account in room
order that spend admits as a reserved starter. It adds no session, so the reserve does not hold it.

**Where the tripwire runs.** In `north fleet tick` on every host whose copy of the usage table holds a
five-hour reading from the last 30 minutes, beside the spend breaker, so each host's spawners read trips
judged on that host. The fleet burst reads the table's five-hour readings, which every host shares. The
session burst reads the spend so far of each session that host runs, which the tick records per session in
`agents/spend/spenders.json`, priced in the account's five-hour points by the calibrated points per dollar
(a session burst is not judged on an account with no calibrated rate, nor on one with no five-hour reading in
the span, whose spend a rate pooled from other accounts would price in points it never reported). A rate row is an input the tripwire checks before it refuses on it: a row whose source lacks the current `rate method N` stamp (written before the method that measures it changed, or by a host still on the old binary) or one older than `spend_rate_max_h` hours prices nothing, so the session burst refuses nothing on that account, and the proxy is told once per stale row, kept in `agents/spend/stale-inputs.json` until a fresh row replaces it. On 2026-11-08 at 1:55 AM +07 the 1:15 AM pooled rate (3.53 points per dollar, measured before the 1:34 AM rate fix) froze every launch for over 20 minutes; the first row under the fixed method read 0.47 (north#838). Current trips go to
`agents/spend/trips.json` there, where dispatch, fleet launch and the spawn hook on that host read them, and
each new one is appended to `agents/spend/trips.jsonl`. Only the host that holds the proxy tells it, so a
fleet burst every host sees reaches the proxy once. `north spend [--json]` prints the view.

**Follows-release.** A released follower is a new worker, so release asks `spend::admit`. A tripwire
refusal leaves the follower waiting for the next release pass rather than failing it.

## Every gate on 2026-11-07, and its fate

`north-pace`, `pace` module:
- `control.rs` (`advance`: `allowed_workers`, `held`, `over`, the burn rates per worker-minute, the
  PACE-HELD and PACE-RESUMED phone notices): **deleted**.
- `window.rs`, `trail.rs` and `plan_stop` (projected percent, horizon, ceiling, burn trail): **deleted**.
- `projection.rs` (`project`, `week_refusal`) and `north tune`'s projection scoring, along with the launch
  record's `projection` field: **deleted**.
- `exhaustion.rs` (the five-hour stop short of the provider's, the five-hour burn cap, the exhausted week):
  **deleted**. The provider limit, rule 1 and rule 2 replace it.
- `admission.rs`: the `refusal` "Hold:" for a stale sample or exhaustion is **deleted**. `interval`, the
  sampler's read cadence, is not a gate; it **moves** into the sampler.
- `target.rs` (spend targets, `[pace."ACCOUNT"] spend_to_pct` by `by`): **deleted**. `seconds` and
  `policy_path` **move** to `north-base`.
- `limits.rs`: pace knobs **deleted**; `default`, `prior_measured` and `overtime_min` **move** with the
  harness defaults reader.
- `gate_order.rs` and the `place_spawn` account order: **deleted**.
- `pace::answer` ops (`advance`, `fresh`, `refusal`, `interval`, `target_for`): **deleted**.

`north-pace`, `router` (`north config agents plan`):
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
- `decide::{place, free, rank, explain}` and `north fleet usage decide --dry-run` (a second placement with its own
  reserve tiers): **deleted**. The world types and `assess`, which hand a session over once its account
  reaches the provider's limit, are **kept**; `hand_over_target` picks its account as **Handover** says.
- Spend breaker: its start refusal for a tripped workstream (`place`'s trip check and the `tripped:` line in
  `north ledger worker --recommend`) is **deleted**. Its budget notices to running sessions are **kept**,
  since they stop no start.

`north-dispatch`, `pacing`:
- `sampling.rs`: `update_controls`, the controller, phone notices and grant retention are **deleted**.
  Reading, recording and cadence are **kept**. Its per-account history line becomes the account's spend
  verdict and active sessions, which the scoreboard's idle stretch reads.
- `grants.rs` (`north fleet pace grant`, `grant_for`, `confirm`, `expired_grant_hold`): **deleted**.
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
  `spend_worker_charge`, `spend_reading_max_h` and the four tripwire knobs are **added**.
- South's orchestration `[pace.*]` tables and the firn unit that described pace enforcement: **deleted** and
  **reworded** in their own repositories.

These are **not** usage gates and are **kept**: each host's saturation reading, the CPU
spawn gate, battery and playtest leases, the landing governor, and the cloud lane's session count. They bound
machines and landing contention. They do not bound account spend.

## Why this shape

Every deleted gate answered one question in its own way: will this account run out before its window
resets? The answers came from different projections, so they disagreed, and the most conservative one won.
The provider already enforces running out. North needs only to keep a little capacity for Tom and the proxy
(rule 1) and to notice a runaway quickly (rule 2). A per-account floor would strand a tenth of every account,
so the reserve is pooled (Tom,
2026-11-07).

## Tier defaults (north#702 2026-11-07, north#966 2026-11-09)

Usage is the binding constraint, so a launch starts from the cell its category's staffing rule names and climbs the ladder after a failed attempt. `north fleet dispatch` and `north fleet launch` both take the model and effort from `dispatch::tier::route_for`: the staffing rule seeds the cell (Haiku high first for mechanical work, whose retry is Sonnet high; Sonnet high for art and every other category, except an unknown-cause bug that names a failing test or repro command, which starts there and climbs once to Opus medium, never a second Sonnet try; Opus for netcode-determinism, performance, review, an unknown-cause bug naming no repro, and planning at medium effort within one module or high across modules, north#966), and the cell draw then picks among the category's leader and two challengers one model or effort step away from `harness/routing-cells.toml` (north#939). A challenger leaves once the bootstrap puts it behind the leader at 90% confidence, or replaces the leader once it is ahead, and the next untried cell comes in, so every non-excluded cell is tried in turn while at most three are live. The Sonnet-against-Opus arm draw, north tune's `route CATEGORY` cells and the flow alert on alternating tier arms are deleted. Both paths refuse a `Category:` outside the brief vocabulary and name the valid list, because unknown words fell through to Opus. When a dispatched worker reports `Not done:` with `Stop: reasoning`, or the lander refused one of its lanes twice for the lane's own failing checks without fewer errors and nothing landed after, `dispatch::tier::route` relaunches the box one rung up the ladder of docs/rationale/nearest-routing.md section 5 with `Follows:` naming the session and its report as the first `Prior report:`; other stops keep the rung.

The before counts by category and tier, and the ledger cost per closed box behind this choice, are private usage figures kept in south (agents/decisions/tier-defaults-2026-11-07.md). The ledger holds no per-run share of a five-hour window, so finished boxes per percent of a window is not readable from it yet.
