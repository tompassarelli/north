# Spend rule: north holds back no launch on usage grounds

Decided 2026-11-11 under north#1076: north refuses, queues, caps and delays no launch on a reading, a
projection or a rate of account usage. The owner ordered every usage limiter removed on that date; a new
design may follow later and starts from nothing here. This supersedes the decisions of 2026-11-07 (north#702,
one pooled reserve), 2026-11-09 (north#1000, the reserve restated at 5 percent) and 2026-11-10 (north#1049, a
per-account operational floor), which this note used to state. The second half of the note keeps the list of
every gate that existed on 2026-11-07 and its fate, because it explains why no usage gate should come back
by accident.

## What still stops a start

One module, `north_pace::spend`, answers whether a session may start on an account. Dispatch,
`north fleet launch`, the spawn hook, hand-over and follows-release call it. It refuses for three reasons, and a
refusal names which: `api hold`, `provider limit` or `operational reserve`.

1. **The owner's API hold.** While `[accounts.claude] api_hold` records a day and a reason, every API
   organization (the pooled `api` account and each `api:ORG`) is refused to every caller of every role, the
   proxy, the owner's sessions, the privacy judge, the lander, canaries, spare fills and one-shot asks
   included, before any request leaves; the refusal is one line naming the hold. `spend::hold` is the one copy
   of the rule: the start refusal every host start passes through (`fleet::registry::reserve`), placement and
   dispatch admission, the API pool (no key handed out, no exhausted mark probed, no session moved onto another
   organization) and the judge's API step all ask it, the park step stops any session found on a held
   organization, and `north accounts` lists every grant as held. Callers then use plan accounts, and DeepSeek or
   Gemini where they already do. North writes no hold and lifts none; only removing that record on the owner's
   word lifts it, so an organization becomes drawable again by an edit the owner ordered, never by a code path. A
   policy that cannot be read or parsed holds every organization too, with one line naming why, so a broken
   file never reads as no hold. A plan session whose own profile settings name an API key helper or a billing
   variable is refused at start, by the same check (`spend::billing`) a one-shot ask makes.

2. **The provider's own limit.** An account at 100 percent of its five-hour or week window cannot run
   anything, so placement skips it and a launch that named it waits in the launch queue for the reset. A
   session that reaches the limit mid-run parks and is revived at the reset, or continues on another account
   in its own conversation (north#998). This is the provider's rule, read from its own figures; north adds no
   margin below it.
3. **The declared operational reserve.** An account named in `[accounts.claude] reserve` (a plan account by
   label, an API organization as `api:ORG`) takes no worker and no subagent; only the privacy judge, the
   lander and the canaries use it. The entry in use names prepaid API credit, so this is a spending boundary
   the owner decides, kept on purpose when the limiters went: workers do not go onto that credit. Placement
   and dispatch never offer such an account, its week stays out of the pool, and
   `north fleet accounts --plan` lists it as operational-only.

The money refusal and its ceiling also stay (`router::money_refusal`): an account whose overflow would bill
money past a plan window is refused. That is cost policy, not pacing.

Nothing else refuses: no pooled reserve, no per-account floor, no burn projection, no pace line, no week
spread over the hours to reset, no hold, grant, reservation, per-account worker count, drain switch,
stale-sample hold or missing-reading refusal, and no requirement that a named account match placement's
first pick. Machine admission by measured memory and CPU pressure is a different rule and is unchanged.

**Readings.** Spend reads the last sampled used percent of each account's five-hour and week windows from
`usage-gate.json`. A window whose reset has passed reads 0. A five-hour window with no reading since its
reset is unread, not 0: the provider stops sending it (a setup token's 429 on a full week carries only the
7d headers), so spend stops nothing on it and `north fleet accounts --plan` prints `5h unread` (north#872).
An account with no reading at all is admitted; the provider decides. `north spend [--json]` prints the view:
the week points left across the accounts a host is signed in to and whose reading is younger than
`spend_reading_max_h`, and the worker sessions working. It is a readout and stops nothing.

**Order.** Order decides only which source takes a worker. Placement ranks plan accounts and API grants
earliest deadline first (`room::Rank`, north#723): first a source the fleet's measured burn could not use up
before its deadline, then the soonest deadline (a plan's weekly reset while its week has room, a grant's
expiry), then week room per plan unit per hour to reset, less `spend_worker_charge` for each box this run
has placed on it, then five-hour room. `north fleet accounts --plan` prints the order with each source's
reason. Order never refuses.

**Agent-tool subagents** run inside their parent's session and lane, so they add no session. The spawn hook
asks spend about the account the parent bills, and so meets the same two reasons.

**Handover.** A running session whose account reaches the provider's limit is handed to the first account in
room order that spend admits.

**Follows-release.** A released follower is a new worker, so release asks `spend::admit`. A refusal leaves
the follower waiting for the next release pass rather than failing it.

**Budget notices** (`usage::budget`) tell a session and its lead when a box has spent twice its stated p90,
or half its p50 in a quarter of its minutes. They ask for a one-line check and stop nothing.

## Deleted on 2026-11-11 (north#1076)

Each of these refused, queued or capped a launch the provider would have accepted. All are gone with their
settings, state files, refusal texts, commands and tests.

- **The spend tripwire and its circuit breaker**, which stopped every launch after a rise in spend.
- **The burn governor**, which projected an account's use to its window resets from the fleet's measured burn
  and refused or queued a worker whose projection passed the floor's budget (`spend/burn.rs`,
  `agents/spend/burns.json`, `spend_burn_five_hour_min`, `spend_burn_week_h`, `spend_rate_max_h`), with the
  cap of two urgent launches per account past it (`spend_urgent_limit`). A launch started with
  `--priority urgent` now only goes first in the launch queue.
- **The pooled reserve** (`spend_reserve`, 5 percent of the combined week of all counted accounts), with the
  charge a run took from the pool for each worker it placed.
- **The operational floor** (`spend_floor_week`, `spend_floor_five_hour`), which kept the last 5 percent of a
  week and 10 percent of a five-hour window of each account for the privacy judge, lander and canaries. Those
  callers now try the declared operational reserve first and meet the provider's limit like everything else.
- **The drain switch** (`north fleet drain on|off`, `spend/drain.json`, the mode line of `north fleet usage`),
  which lifted the pooled reserve and has nothing left to lift.
- **The refusal of an account with no usage reading**, in spend and in placement, whose verdict on an account with
  no sampled window was `no live quota sample`. Such an account is now offered like any other.
- **The hold of one account until another is spent** (`[accounts.PROVIDER] held_until_spent`, `Basis::held`), which
  kept an account out of placement and dispatch while the named account had week left.
- **The workstream budget breaker's trip**, which told a workstream's sessions to stop at four times their
  p90, with its alert to the owner and its clears.

## Every gate on 2026-11-07, and its fate

`north-pace`, `pace` module:
- `control.rs` (`advance`: `allowed_workers`, `held`, `over`, the burn rates per worker-minute, the
  PACE-HELD and PACE-RESUMED phone notices): **deleted**.
- `window.rs`, `trail.rs` and `plan_stop` (projected percent, horizon, ceiling, burn trail): **deleted**.
- `projection.rs` (`project`, `week_refusal`) and `north tune`'s projection scoring, along with the launch
  record's `projection` field: **deleted**.
- `exhaustion.rs` (the five-hour stop short of the provider's, the five-hour burn cap, the exhausted week):
  **deleted**. The provider limit replaces it.
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
  since they stop no start; they live in `usage::budget`. Its trip, which told a workstream's sessions to stop at
  four times their p90, with its alert and its clears, was **deleted** on 2026-11-11 (north#1076).

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
  the charges of this run ordering its accounts.
- `slots.rs` (`Account.workers` and `Pools.workers`, from `allowed_workers`): **deleted**.
- `survey.rs` (the 15-minute pace-control staleness refusal, `account_refusal` with the week box reserve,
  `pace_workers`, `five_hour_burn`, `readings`): **replaced** by the spend view.

`north-guard`, spawn hook (`spawn_capacity`):
- The usage-freshness "Hold:" with its forced refresh, `pace_hold`, grants and the `reserve` op: **replaced**
  by `spend::admit` on the billed account.
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
  `reservation_fallback_points` and the replay quota floors are **deleted**. `spend_reserve` (deleted 2026-11-11),
  `spend_worker_charge` and `spend_reading_max_h` are **added**.
- South's orchestration `[pace.*]` tables and the firn unit that described pace enforcement: **deleted** and
  **reworded** in their own repositories.

These are **not** usage gates and are **kept**: each host's saturation reading, the CPU
spawn gate, battery and playtest leases, the landing governor, and the cloud lane's session count. They bound
machines and landing contention. They do not bound account spend.

## Why this shape

Every deleted gate answered one question in its own way: will this account run out before its window
resets? The answers came from different projections, so they disagreed, and the most conservative one won.
The provider already enforces running out. The reserve, floor and governor that replaced those gates between
2026-11-07 and 2026-11-11 were fewer, but each was still an approximation of the provider's answer, and each
queued work the provider would have run. So north keeps none: it reads usage to rank accounts and to report,
and leaves stopping to the provider.

## Day and night posture (design, north#1276, 2026-11-12)

This is a routing design, not a limiter: it changes which provider a ready box goes to and never holds a launch,
so the decision above stands. It follows the owner's direction of 2026-11-11 and is built only after this page is
accepted.

- **At the desk.** The trusted top Claude models work the queue at full speed, so landings and decisions arrive
  while the owner can steer. A box is routed by its category's staffing rule, as today.
- **Away.** The well-specified low and medium backlog on public repositories goes to the DeepSeek harnesses
  (`DEEPSEEK_PROVIDERS` in `crates/north-fleet/src/fleet/claude_only.rs`). Claude accounts keep the coordinator
  and every private-repository box. A decision that needs the owner waits as a gate that carries a default.
- **The signal.** At-desk is the laptop seat's `IdleHint` (`seat_idle` in
  `crates/north-capacity/src/capacity/laptop_auto.rs`: `no` is at the desk, `yes` is away) or the owner's word,
  which wins. A reading that cannot be taken counts as at the desk.
- **What each provider may touch.** DeepSeek: public repositories only, none of the subjects matched by
  `[dispatch] claude_only` in `harness/opencode.toml` (credentials, locks, sandbox and the like), no secret and no
  private transcript. Claude: everything, with the top models first on the coordinator and on private work.
- **What it does not do.** It reads no usage figure, queues nothing on one and stops no account. `north fleet
  launch` still refuses a DeepSeek worker on a `claude_only` box however it was started.

## Tier defaults (north#702 2026-11-07, north#966 2026-11-09)

Usage is the binding constraint, so a launch starts from the cell its category's staffing rule names and climbs the ladder after a failed attempt. `north fleet dispatch` and `north fleet launch` both take the model and effort from `dispatch::tier::route_for`: the staffing rule seeds the cell (Haiku high first for mechanical work, whose retry is Sonnet high; Sonnet high for art and every other category, except an unknown-cause bug that names a failing test or repro command, which starts there and climbs once to Opus medium, never a second Sonnet try; Opus for netcode-determinism, performance, review, an unknown-cause bug naming no repro, and planning at medium effort within one module or high across modules, north#966), and the cell draw then picks among the category's leader and two challengers one model or effort step away from `harness/routing-cells.toml` (north#939). A challenger leaves once the bootstrap puts it behind the leader at 90% confidence, or replaces the leader once it is ahead, and the next untried cell comes in, so every non-excluded cell is tried in turn while at most three are live. The Sonnet-against-Opus arm draw, north tune's `route CATEGORY` cells and the flow alert on alternating tier arms are deleted. Both paths refuse a `Category:` outside the brief vocabulary and name the valid list, because unknown words fell through to Opus. When a dispatched worker reports `Not done:` with `Stop: reasoning`, or the lander refused one of its lanes twice for the lane's own failing checks without fewer errors and nothing landed after, `dispatch::tier::route` relaunches the box one rung up the ladder of docs/rationale/nearest-routing.md section 5 with `Follows:` naming the session and its report as the first `Prior report:`; other stops keep the rung.

The before counts by category and tier, and the ledger cost per closed box behind this choice, are private usage figures kept in south (agents/decisions/tier-defaults-2026-11-07.md). The ledger holds no per-run share of a five-hour window, so finished boxes per percent of a window is not readable from it yet.

## Feature record "API credit readings", as written until 2026-11-13

The feature record `0024-api-credit-readings-north-1043.md` was cut to a short entry; this is its earlier text, kept for the reasons and dates it holds.

- API credit readings (north#1043): `north fleet accounts --reading api:ORG USD` records the balance the operator reads in the provider console under `~/.local/state/north/api-readings/ORG`, and `north accounts`, `north fleet usage` and placement use it in place of north's list-price estimate, naming the figure, its console source and reading time with the estimate beside it; a provider reading from the provider's own spend reporting outranks a console one until the operator records a newer provider figure. `north fleet accounts --poll`, run by the fleet tick every ten minutes, reads each api organization whose Admin key this host holds, prices its hourly usage by model from the router's price table and checks the total against the provider's daily spend reporting, then records what is left of the credits recorded for that organization after the cost report's posted spend as its provider reading; the poller reads the key from its file and never prints it. An `api-exhausted/ORG` mark keeps the provider's refusal body verbatim and is written only for a refusal that names low credit; `north fleet accounts --probe` makes one one-token Messages request per held mark, clears the ones that pass, leaves a tombstone of the cleared text so a peer's host sync cannot restore that same mark, and rewrites the body of each mark it keeps from that probe's refusal, so a mark made before bodies were kept learns why the provider refuses. Every view reads one producer of an organization's credit (north#1063): the reading, else the list-price estimate of every unexpired grant charged with the spend of every pool name sharing the organization, closed only by a held exhausted mark; `north dash`'s billing section and the fleet screen's digest print each organization as `api:NAME $X left (SOURCE reading TIME, estimate $Y)`, and `north fleet usage` prints `exhausted` only for a held mark. A subscription account whose client refuses its first turn (subscription access disabled, signed out, a revoked login, or a weekly or five-hour limit) is marked under `~/.local/state/north/account-refused/LABEL` with the refusal text; `north accounts` lists it unavailable naming that text, placement and the contract probe skip it, a launch with `--account LABEL` is refused with it, and a 99% window reading counts as no headroom once the provider has refused. `north fleet accounts --probe` runs one one-turn Haiku prompt through that client and clears the mark only when it answers.
