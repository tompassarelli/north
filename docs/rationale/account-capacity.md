# Account usage: one reserve, placement that manages work in flight, and a spend breaker

Plan of 2026-11-04 (laptop time), revised the same day to Tom's design of 2026-11-05. Tom finds the account usage talk
confusing and surprising: leads hold workers on projections the written rule forbids, and several mechanisms disagree.
His purposes, in order: spend every unit of allowance before it resets, highest-value work first (P1); keep his
declared reserve for his own use (P2); stop work that runs far past its estimate early (P3); never lose work in flight
when a window fills (P4); explain every decision from its inputs in one line (P5). This document inventories what
decides today, specifies the replacement as pure functions over recorded readings, lists what is deleted and splits
the work into boxes. It edits no code.

Words: **usage** is a provider account's allowance (five-hour, weekly and model-scoped weekly windows); **cores** is
machine capacity (cores, memory, leases, the capacity watchdog), which this plan does not touch. Code, docs and reports
on the account side say usage, never capacity.

A reviewer of a different tier tried to break it; each finding and what changed is in
[Review disposition](#review-disposition).

## 1. Inventory: everything that decides, holds, delays, nudges or reports on account usage

Paths are relative to the repository root; `C` is `crates/north-core/src`, `P` is `C/ops/pacing`. State files live
under `~/.local/state/agents` unless named otherwise. Line numbers are at origin/main c7060fa6f.

### 1.1 Placement: `north fleet launch`, `agents plan --place`, revive, probes

- `src/cli/fleet/router.rs:152` `launch` calls `admission::admitted` with `placement::placed` (`:160`); placement's
  account overrides `--account` (`:161-162`).
- `src/cli/fleet/placement.rs:104` `placed` skips non-Claude and `--test` launches, else `check` (`:115`, prints the
  verdict lines) and `settle` (`:126`): with `--account` it runs `admit` (`:161`), else `pick` (`:136`).
- `src/cli/fleet/placement.rs:204` `exhausted_refusal`: an account the exhaustion rule refuses is refused; `--pin`
  cannot override it.
- `src/cli/fleet/placement.rs:224` `account_refusal`: refuses a named account unless it ranks at or above
  placement's pick or ties within `TIE_WEEK_POINTS = 5.0` week points; `--pin` overrides.
- `src/cli/fleet/admission.rs:38` `admitted`: retries only machine saturation (every 60 s for up to 2 min, `:11-12`);
  account refusals are final at once.
- `P/placement/gate.rs:439` `decide`: picks the account, then the machine. A code-only public box with a best
  account becomes `Target::Cloud`; no verdicts holds "no live Claude quota in usage-gate.json; run pace-control"; no
  available account holds "every Claude account exhausted or over its burn cap". Then `sign_in_on_machine` (`:111`),
  `keep_here_on_local_account` (`:126`) and `run_on` (`:140`) switch to the best account signed in where the work runs.
- `P/placement/gate.rs:185` `window_refusal` and `:211` `judged`: no `used_pct` in any window is unavailable ("no live
  quota sample"); the exhaustion refusal (with burn) is `exhausted`; else available with headroom from
  `exhaustion::headroom` (`:191`). `:207` `verdict` is this with burn counted; `C/ops/dispatch/survey.rs:160` reuses it.
- `P/placement/gate.rs:422` `standing_verdicts`: roster standing per `claude:` control: duplicate or unknown profile
  is unavailable, foreign is judged with owners.
- `P/placement/gate.rs:169` `best`: the available account with the highest rank, ties by name. `:176`
  `roomiest_here`: most headroom among unowned accounts, burn ignored; used by `src/cli/probe_account.rs:12` ("no
  Claude account has headroom for the probe") and `C/ops/model_watch/mod.rs:156`.
- `P/placement/room.rs:82` `room`: five-hour room `95 - used`, week room `100 - used`; an unsampled window counts as
  unused. `:38` `Room::expiring` is week room times plan multiplier per hour to reset. `:110` `Rank::of` and `:114`
  `order`: a live pace spend target first, then `expiring`, then scaled five-hour room. `:120` `ties`. `:47`
  `box_reserve` 1.0 point, `:52` `week_short`, `:57` `takes`, `:65` `plan` (plan multipliers from
  `harness/model-catalog.toml:288-297`).
- `P/placement/gate.rs:490` `live` reads `orchestration.toml`, `usage-gate.json` (`:181` `live_gate`) and
  `pace-control.json` (rank basis); `:478` `summary` prints the `target ...` line that
  `C/ops/timers/fleet_session.rs:31` parses before launching with `--account` (`:63`).
- `agents plan --place`: `src/cli/agents.rs:32` to `C/ops/agents/plan.rs:28` to `P/placement.rs:284` `cli`
  (`place place BRIEF`, `place credit`), which runs `gate::decide` and prints verdict lines
  (`P/placement/gate.rs:226`: `{account} available: {usage} ({room}% headroom)`).
- Legacy `place_spawn` (no caller in the repository; only `P/fixtures/placement-replay.json` and
  `P/placement_tests.rs` exercise it): `P/placement.rs:252` `operation`, `:205` `place`, `:228` `release` (ETAs from
  the five-hour reset or `state_since + 10 min`), `P/placement/launch.rs:293` `spawn`, `P/placement/survey.rs:25`
  `control_reason` (an account with no `held_reason` reads "stale"), `:101` drain preference
  (`placement_drain_margin`), `:114` `open_accounts` order, `P/placement/tiers.rs:30` `tier_for`.
- `src/cli/fleet/revive.rs:36` `restart_placed` re-runs `placement::check`; `src/cli/fleet/rebooted.rs:48` turns
  `Refusal::Exhausted` into `Revived::Waiting` (deferred).

### 1.2 The Agent-tool spawn gate

- Wiring: `harness/policy-owners.toml:8` registers `north-hook.sh` on `PreToolUse:Agent`; `C/hook.rs:131` maps
  `Agent` and `spawn_agent` to `PreAgent`, which runs `agent_cwd`, `worker_tier` and `spawn_capacity::evaluate`
  (`:195`); `north hook spawn-capacity` runs `admission` (`:197`).
- `C/guard/spawn_capacity.rs:32` `evaluate` (killswitch unit `spawn-capacity-guard`), `:66` `decide`, `:78` `spawn`:
  1. `:83` op `grant_for` looks for a proxy grant in `pace-control.json`.
  2. `C/guard/spawn_capacity/usage.rs:5` `verdict` reads `usage-gate.json[provider]`, requiring a reading newer than
     `min(usage_stale_min x 60, 120)` = 120 s (`:41`) and, when paced, a fresh control (`:44-55`); otherwise it runs
     `agents usage --gate claude` (45 s, `world.rs:138`), which samples through `north pace gate`. Refresh
     unavailable: "Hold: claude usage refresh unavailable; needs a reading <=120s old" (`:10`); still stale:
     "... remains stale after refresh" (`:13`).
  3. `:86` a gate-file `refusal` denies: the money refusal written at `C/usage/gate.rs:99-106` from
     `C/router/route.rs:146` `money_refusal`.
  4. `:93-104` when paced and not granted: no control holds "Hold: {name} pace controller has no fresh account
     reading; refresh before spawning" (`:96`), else op `refusal` (`C/pace/admission.rs:24`).
  5. `:106` `admitted` runs op `reserve` (`P/reservation.rs:107`), which adds a worker to `usage-gate.json`.
  6. `:67-75` any `Hold:` also runs op `expired_grant_hold`, which may nudge the lead (`P/grants.rs:383`).
  7. Confirmation after the spawn: `:46` `confirm_native` (Codex), `C/guard/worker_handoff/spawn.rs:68` and
     `C/guard/spawn_state.rs:19` (Claude), op `confirm` (`P/grants.rs:194`).
- `:124` `capacity_refusal` is the CPU and landing-flow gate (`priority_headroom`, machine leases). It is not account
  capacity and stays.
- `:58` `gate_summary` (`north hook spawn-gate`, read by `C/ops/agents/goal/plan.rs:20`) reports one `open` value
  for every account, ignoring per-account state.
- Denials are logged to `spawn-gate-denials.jsonl` (`C/guard/spawn_capacity/logs.rs`).

### 1.3 `north dispatch`

- `C/ops/dispatch/survey.rs:150` `accounts`: reads `pace-control.json` and `usage-gate.json` controls (only `claude:`
  names get a sample), `gate::verdict` (`:160`), plan size (`P/placement/room.rs:65`), `room` and the five-hour burn
  (`:146`).
- `survey.rs:120` and `:133` `account_refusal`: a `pace-control.json` older than `PACE_FRESH_S` = 900 s (`:27`) is
  stale; no verdict staffs the account nowhere; else `Room::week_short(box_reserve)` refuses an account with no more
  week points than the 1 a box reserves.
- `C/ops/dispatch/slots.rs:69` `open`: only open accounts get slots; `slots.rs:77` prints the per-account line
  (`mod.rs:210`).
- `C/ops/dispatch/mod.rs:130` `decide`: each box charges `box_reserve` (1 point); its ETA is the ledger p75
  (`:166`).
- `C/ops/dispatch/plan.rs:64` `charge` (1 point over plan size per window); `:70` `takes`: both windows stay above 0
  after the charge and the five-hour room exceeds measured burn x ETA; `:89` `burn_held` prints "{host} holds the
  box: {account} measured burn X%/h over a N min ETA spends Y of the Z points left to its 5h stop"; `:103`
  `best_account` by `Rank`; `:109` `pin`; `:143` `revive` ("{account} has no room for another box this run");
  `:165` code-only boxes take the cloud lane with no account check.
- `C/ops/dispatch/tries.rs:13`: a session whose report names a usage limit spends no try.
- `C/ops/dispatch/grounds.rs:79` `Grounds::budget` writes the Budget line: twice the cell's median closed-box
  API-equivalent cost (`BUDGET_TIMES`, 7-day window), floored at the all-cell median box. Nothing parses or enforces
  it; a worker reads its own spend with `north fleet spend --self` (`src/cli/fleet/spend.rs:12`).

### 1.3b The router (`agents plan` tier and account choice)

- `C/router/route.rs:42` `launch_refusal` is the exhaustion rule; `:77` `route` keeps routable accounts (auto-route
  and finite price) with no launch refusal and ranks them by under pace, then week expiry; `:55` `route_account`
  takes under-pace accounts only; `:132` `fast` needs plan billing, under pace and every window above `fast_floor`;
  `:146` `money_refusal`.
- `C/router/ctx.rs:341` `metrics`: pace line `(100 - target_left) x elapsed share of the week`, state "under pace" or
  "at or over pace", projected left from burn, `drain_behind` on Thursday above the `pace_band` top; `:385` `price`
  (infinite when billing is not plan or use is at `money_ceiling` 92 with money overflow, 0 under pace, else 1);
  `:246` `exhausted` (week headroom at or below `target_left`); `:298` `stale` (over 30 minutes).
- `C/router/choose.rs:189` `choose`: a tier is open if its price is finite and its account has no launch refusal;
  `:246` `closed` returns "Blocked: decision for Tom: no under-pace plan-billed tier admits ...; bill usage credits for
  it?" (an ask about extra usage, which Tom's policy forbids raising) or waits "account exhausted" (`:265`); `:271`
  `pace_warning` admits with "account {n} at or over pace"; `:158` `explorable` needs the account under pace.
- `C/router/plan.rs:194` `plan` fails outright when any account is stale (`:195-196`).
- `C/router/policy.rs:23-24` `five_hour_target` and `five_hour_ceiling` are parsed and never read.
- Rate-limit text: `C/providers/claude/runs/outcome.rs:21` classifies "You've hit your ..." runs as idle;
  `C/providers/claude/quota.rs:5` and `C/fleet/model.rs:305` keep stream `rate_limit_event` windows for display only;
  neither records the provider's `status` (`allowed` or `rejected`). `C/providers/claude/token_quota.rs:29` reads
  only the `-utilization` and `-reset` headers, not `anthropic-ratelimit-unified-status`.

### 1.4 The pace controller (`C/pace`, `P/sampling.rs`, `bin/pace-control`)

- `bin/pace-control` (3 lines) runs `north pace sample`; nixos-config `modules/pace-enforcement/default.nix:29-49`
  runs it every minute on the laptop and gex45 (`OnCalendar=*:*:00`, `TimeoutStartSec=110s`; also named in
  `hosts/whiterabbit/configuration.nix:66`, `tags.json:114` and the `update-notify.test.sh:36` fixture).
- `P/sampling.rs:304` `sample`: reads due adapters, records rows in the threads `usage` table, measures, publishes.
  `:42` `due_adapters` delays a provider until `providers.<p>.next_at`. `:52` `read_adapters` runs
  `python3 -I harness/lib/usage_{claude,agy}.py` locally and over ssh. **Live defect:** commit 04db623b1 (2026-11-04)
  deleted those scripts; Python exits 2, `:161` treats any nonzero exit as throttled, and `C/pace/admission.rs:15`
  `interval` doubles without a ceiling. On gex45 at 5:31 PM UTC `pace-control.json` showed `providers.claude`
  `code 2, interval_s 30720` and all four Claude controls 2 h old, `held: true, over: true`. The threads `usage` table
  itself was fresh (17:21 UTC rows): only the controller froze.
- `P/sampling.rs:401` `update_controls`: per account counts fleet nodes `working` on it and calls `control::advance`
  with margin 10 (`:442`) and initial 2 (`P/command.rs:115`).
- `C/pace/control.rs:192` `advance`: burn from a 10-minute anchor (`:12`, `:76-77`) or a 60-minute trail
  (`C/pace/trail.rs:4-18`, `pace_burn_window_min`); `:185` `is_over` (a week at or over its ceiling: `plan_stop` 98
  with a target, else `quota_ceiling`, else `100 - pace_target_left_claude` = 95; or any non-five-hour window
  projected over 100, with a 10-point hysteresis); `:156` `standing` (`held = !fresh || over || workers unknown`);
  `:161` a 10-minute dwell between HOLD and RESUME (`pace_state_dwell_min`); `:168` `notify`; `:172` `next_allowed`
  (halve allowed workers when over, else raise by one when the projection allows); `:130` `capacity` (allowed
  workers from five-hour room over measured rate).
- `C/pace/window.rs:76` `projected` (used + burn x horizon), `:57` `ceiling`, `:65` `horizon`, `:80`
  `below_deadline_line`. `C/pace/target.rs:55` `target_for` reads `[pace.<account>] by, spend_to_pct` from
  `orchestration.toml` (`south/agents/orchestration.toml:14-21` holds two expired targets).
- `C/pace/exhaustion.rs:19` `refusal`: the rule that refuses today. A week window at or over 100
  (`WEEK_EXHAUSTED`, `:8`); a five-hour window at or over 95 (`FIVE_HOUR_STOP`, `:7`); a five-hour burn over
  `five_hour_burn_cap_pct_h` = 50 %/h (`:29-32`). `:36` `headroom` ranks.
- `C/pace/admission.rs:9` `is_fresh` and `:24` `refusal`: a sample or window older than `MAX_AGE_SECONDS` = 120
  (`C/pace/window.rs:8`) holds ("Hold: {account} pace sample is not fresh (needs <=120s)"); else the exhaustion
  rule, with the reset in local 12-hour time (`:44` `resets`).
- `P/sampling.rs:375` `phone_notice`: sends PACE-HELD or PACE-RESUMED to Tom through `bin/supply-alert`.
- `C/usage/gate.rs:45` writes `usage-gate.json` (`:109` per-window projection `used + burn x reset_in`; `:127`
  `quota_ceiling = min(plan_stop, 100 - target_left)`; `:99-107` per-account money refusal whose text at `:15` tells
  the lead to "wait for the window to reset").
- `P/mod.rs:63` `north pace gate PROVIDER` (exit 3 when refused); `src/cli/agents.rs:28` `agents pace`, `:51`
  `agents usage --gate P`.

### 1.5 Grants and reservations

- `P/grants.rs:7` `find`, `:29` `workers`, `:70` `terms`, `:102` `registered_lead`, `:143` `store_grant`, `:194`
  `confirm` (logs `grant-admission`, and `grant-late-confirm` after `grant_reservation_hold_s` = 30 s), `:383`
  `expired`: once per key, sends `GRANT-EXPIRED-HOLDING` to the lead through `north fleet send` (a session nudge).
- `P/command.rs:62` `north pace grant claude:L MAX --until --for --reason [--task --uses]`.
- `P/reservation.rs:107` `reserve` under `pace-control.lock` (6 s wait, "Hold: ... stayed locked"); `:79`
  `reserve_pooled` ("Hold: ... no account reading", else the admission refusal, else `workers += 1` in
  `usage-gate.json`); `:40` `reserve_grant` (`max_workers`, `uses`, pending reservations younger than
  `spawn_reservation_ttl_s` = 5 s).
- State: `pace-control.json` (`accounts`, `providers`, `history`, `grants`), `usage-gate.json`,
  `pace-control.jsonl` (append-only, 82 MB on gex45), `pace-control.lock`, `pace-sampling.lock`.

### 1.6 Nudges and readouts

- The capacity watchdog never reads account usage: `C/capacity/watchdog/detect.rs:298-314` (overload nudge) and
  `:223` (release) are about machine load, and `bin/capacity-watchdog` `report` flags context size
  (`handoff_ctx`, `:189-193`). It stays. The account nudges are `GRANT-EXPIRED-HOLDING` (`P/grants.rs:383`), the
  PACE-HELD phone notice (`P/sampling.rs:375`) and the grounding readout below.
- `C/grounding/live.rs:8-54`: any prompt matching usage, pace, burn, quota, 5h or weekly gets "Usage, source of truth
  pace-control.json" with each account's windows and `allowed_workers N[ held]` (called from
  `C/grounding/compose.rs:357,378`). With the controller marking every account `held`, every lead that asks about
  usage reads "held", which is how leads come to hold on projections the written rule forbids.
- `north usage` / `agents usage` (`C/usage/table.rs:64-71`): per account `{windows} | {resets} | {burn}{plan}`;
  `:41` each window; `C/usage/burn.rs:18-48` the burn column, including the projection "burn X%/h, empty in
  {duration} ({time})"; `C/usage/mod.rs:152-161` JSON with `stale` past `usage_stale_min`.
- `C/usage/fast.rs:8-24` `north usage --fast` and `C/router/route.rs:143` allow fast mode only while every window
  keeps more than `fast_floor` = 10 points of headroom.
- `C/router/route.rs:47` `launch_refusal` calls the exhaustion rule; `:100` and `C/router/choose.rs:285-287` say "at
  or over pace"; `C/router/ctx.rs:114-126` per window headroom and burn, `:145-156` rollover after a reset,
  `:158-171` `target_left`, `band` (`pace_band` [10, 20], a Thursday projection above the top flags `drain_behind`)
  and `thursday`.
- `C/health.rs:92-102` check `pace` (newest `pace-control.json` history under 180 s) and `:104-106` check `usage`
  (newest usage row under 600 s).
- `north flow` prints no account pace or projection line; `C/ops/flow/mod.rs:85-86` notes a stale
  `claude-web.json` (7 h).
- Docs that state the rule: `docs/features/02-staffing-and-routing/0005-routing.md`, `0006-no-pace-hold.md`,
  `0016-pace-notices.md`, `docs/features/03-fleet-sessions/0006-placement-gate.md`, `0011-dispatch.md`,
  `docs/harness.md:101`, `docs/rationale/hooks-and-runtime.md:56`.

### 1.7 `harness/defaults.toml` entries the usage code reads

- `placement_drain_margin` 0.5 (`:1`; `P/placement/tiers.rs:26`, `survey.rs:105`)
- `reservation_fallback_points` 1.0 (`:9`; `P/placement/room.rs:48`, `tiers.rs:27`)
- `usage_stale_min` 30 (`:108`; `C/router/ctx.rs`, `C/usage/gate.rs:61`, `C/usage/settings.rs:22`)
- `money_ceiling` 92 (`:115`; `C/router/ctx.rs`)
- `plan_stop` 98 (`:122`; `C/pace/limits.rs:29`, `C/usage/gate.rs:128`)
- `fast_floor` 10 (`:129`; `C/router/ctx.rs:45`)
- `pace_target_left_claude` 5 (`:227`; `C/pace/limits.rs:45`)
- `pace_band` [10, 20] (`:234`; `C/router/ctx.rs`)
- `pace_state_dwell_min` 10 (`:277`; `C/pace/limits.rs:53`, `P/placement.rs:237`)
- `pace_burn_window_min` 60 (`:284`; `C/pace/limits.rs:57`)
- `five_hour_burn_cap_pct_h` 50 (`:291`; `C/pace/limits.rs:49`)
- `spawn_reservation_ttl_s` 5 (`:513`; `C/pace/limits.rs:61`, `P/reservation.rs:8`)
- `grant_reservation_hold_s` 30 (`:521`; `C/pace/limits.rs:64`, `P/reservation.rs:34`, `P/grants.rs:233`)
- Read through `C/pace/limits.rs` but not account capacity, so they move, not go: `handoff_ctx` (`:94`),
  `handoff_arm_ctx` (`:101`), `overtime_min` (`:87`), and `default`/`prior_measured` (`C/ops/report/calibration.rs:73`).
- Hard-coded: `FIVE_HOUR_STOP` 95 and `WEEK_EXHAUSTED` 100 (`C/pace/exhaustion.rs:7-8`), `MAX_AGE_SECONDS` 120
  (`C/pace/window.rs:8`), `BURN_PERIOD_MINUTES` 10 (`C/pace/control.rs:12`), margin 10 (`P/sampling.rs:442`),
  initial 2 (`P/command.rs:115`), `TIE_WEEK_POINTS` 5 (`src/cli/fleet/placement.rs`), `PACE_FRESH_S` 900
  (`C/ops/dispatch/survey.rs:27`), `WEEK_H` 168 (`P/placement/room.rs:9`).

### 1.8 Skill and agent text

- `harness/skills/workers/SKILL.md:10`: "`agents plan` picks provider, account and fast mode from live headroom".
- `harness/skills/workers/references/tiers.md:5`: choose provider and fast mode with `agents plan` from live
  `agents usage` headroom.
- `harness/skills/workers/references/cloud.md:12`: an account is out at five-hour 95%, five-hour burn over 50 %/h, or a
  provider-exhausted week; pace lines, projections and held/over/allowed never hold; `:20` cloud placement under
  any account with headroom; `:34` `agents plan --credit`.
- `harness/skills/workers/references/run-and-land.md:15`: a brief's Budget line is a pacing signal, not a bill.
- Lead-facing refusal text that says to hold: `C/usage/gate.rs:15` ("wait for the window to reset, or report
  Blocked"), `C/pace/admission.rs:26-37` and `C/guard/spawn_capacity.rs:96` ("Hold: ... refresh before spawning").
- Standing order in the session prompt (Tom 2026-10-16): never launch on an account whose window is over or
  "burning toward exhaustion in under 2 hours"; weekly pace lines are advice. That order is itself a projection
  rule; section 5 replaces it.
- `harness/claude/agents/*.md` and the `AGENTS.md` files hold no account-capacity text.

### 1.9 Where the mechanisms disagree

1. Staleness has six bounds: 120 s (admission, spawn gate), 180 s (health `pace`), 600 s (health `usage`), 900 s
   (dispatch), 30 min (`usage_stale_min`, router and usage table) and 7 h (flow's web cache).
2. An unsampled window is unavailable in `gate.rs:211` but counts as fully unused in `room.rs:82`.
3. The burn cap refuses in `gate::verdict` but `roomiest_here` ignores it.
4. The week's ceiling is 100 in the exhaustion rule, 95 (`100 - target_left`) or 98 (`plan_stop`) in the
   controller and the gate's `quota_ceiling`, and 92 (`money_ceiling`) in the router.
5. `north fleet launch --account X` is refused unless X is within 5 week points of the pick; the Agent-tool gate only
   checks the billed account.
6. Dispatch refuses an account whose five-hour room is below measured burn times the box's ETA
   (`C/ops/dispatch/plan.rs:70`): a projection that decides, which `0006-no-pace-hold.md` says never happens.
7. The controller marks accounts `held` and `over`, which the rule says never refuse, and the grounding hook prints
   "held" into every usage prompt.
8. The router ranks and opens tiers by "under pace" (`C/router/route.rs:55`, `:77`, `C/router/choose.rs:158`),
   so `agents plan` routes on a pace line while placement says pace never decides; when no under-pace tier admits
   a box it asks Tom whether to bill usage credits (`C/router/choose.rs:250`).
9. `agents plan` fails when any one account is stale (`C/router/plan.rs:195`), while placement only drops that
   account.

### 1.10 What the replacement builds on

- Live readings: `C/claude_stream.rs:109` `RateLimitInfo` (`status`, `resetsAt`, `rateLimitType`,
  `unifiedWindows`) arrives on every live Claude stream, but `UnifiedWindows` (`:121`) keeps only `five_hour` and
  `seven_day`, dropping scoped windows such as `seven_day_opus` that the stream carries (`C/providers/claude/quota.rs`
  test fixture), and nothing writes these frames to the `usage` table: `C/providers/claude/quota.rs:5` and
  `C/fleet/registry/follower.rs:59-60` keep them for the fleet view only.
- Idle readings: `C/providers/claude/token_quota.rs:29` (one-token Messages call, `-utilization` and `-reset`
  headers, no `-status`), `C/usage/refresh/claude.rs:72-89` (`claude -p /usage`, scoped weekly rows such as
  `weekly_scoped/Fable`), `C/usage/web.rs:35` (`claude-web.json`). All land in the threads `usage` table
  (`C/usage/store.rs:9`: `ts, provider, account, kind, name, used_pct, window_min, resets_at, amount, source`).
- Parking: `src/cli/fleet/waits.rs` (`north fleet wait RUN` parks a worker on a `job:`, `farm:`, `branch:`,
  `capture:` or `pid:` run; `north fleet waits` revives it, `C/fleet/waits.rs:23` `Run`); `src/cli/fleet/rebooted.rs`
  and `north fleet stranded --json` revive sessions a reboot stranded; `src/cli/fleet/revive.rs:36`.
- Hand-over: `north fleet launch --take-over` (`src/cli/fleet/launch.rs:104`) ends the session holding a name and
  gives the new one its name and messages; `Follows:` briefs chain runs into one box
  (`C/guard/worker_tier.rs`, `crates/north-base/src/platform/follows.rs`).
- Run outcome: `C/providers/claude/runs/outcome.rs:21` recognises "You've hit your ..." stops.
- Estimates: `north worker-ledger --recommend 'FACETS'` (`src/cli/worker_ledger/usage.txt:5`); `C/ledger/closed.rs:75`
  `Figures` (median priced box cost, p75 minutes; no dollar p90); `C/threads/calibration/recommend.rs:157` (active
  minutes p25, median, p90 by facet cell with back-off).
- Spend: `north fleet spend --self` (`src/cli/fleet/spend.rs:12`, `C/ledger/books.rs:12`, priced by
  `C/router/pricing.rs:64` from `C/router/prices.json`).
- Cloud credit: `P/placement/hosts.rs:12` `credit`. The only credit, $250 on claude:a, was spent by 2026-10-26 and
  expires 2026-11-05 7:59 AM UTC (`docs/rationale/capacity-scheduler.md`, "anthropic-cloud, modelled honestly"); cloud
  sessions otherwise count toward the launching account's plan usage.

### 1.11 Found by the review

- `south/agents/rules.toml:747`: agents take usage numbers from `~/.local/state/agents/pace-control.json`.
- `south/agents/routines/pace-enforcement.md`: says fleet sessions receive pace transitions.
- `south/agents/orchestration.toml:163-167`: the `[pace]` comment and `enforce_from`.
- The worker-ledger timer runs `agents usage --refresh` every 15 minutes (nixos-config
  `modules/worker-ledger/default.nix:44`), which takes `pace-control.lock` and rewrites `usage-gate.json`
  (`C/usage/refresh/mod.rs:43-55`).
- `C/ops/timers/cross_project.rs:99-168` picks a Claude account by trying each until one is not "at its usage limit".
- `bin/follows-release.lib.sh:38` and `bin/follows-release-gates.test.sh:7` write `usage-gate.json` from shell.
- Integration tests that pin today's behaviour: `tests/it/{fleet_launch,session_path,agents_dispatch,
  agent_spawn_hook,worker_handoff_hook,usage_gate,placement_gate}.rs`; `nexus-alerts/README.md:21`.
- The provider stops sending a five-hour window once the week is out: latest rows on 2026-11-04 have a's `session`
  from Oct 31 1:19 AM UTC beside a fresh `weekly_all` 100, b's from Oct 31 10:08 AM beside 100, and c's `session` at 0
  with no `resets_at`.
- `C/usage/refresh/claude.rs:129-130` records `billing extra_usage` with `used_pct` = utilization and `amount` =
  `is_enabled`; nothing reads it.
- `active_min` is set on 102 of 2,674 threads `runs` rows since Oct 24; `minutes` (wall) is set on all.

## 2. Evidence

### 2.1 Windows fill, and a full window stops every session on the account

Source: five-hour windows in `pace-control.jsonl` on gex45, 2026-10-24 5:58 PM to 2026-11-04 3:32 PM UTC (29,149
readings), the threads `usage` table, and session transcripts under each profile.

- 16 five-hour windows reached 95%; 13 went on to 100%, usually 0 to 14 minutes after crossing 95, and sat full for 11
  to 176 minutes until their reset. All but two were on claude:c on Oct 26 and 27, while its week read 5% to 91%.
- Sessions on accounts c and d end turns with "You've hit your limit · resets 3pm" (Oct 27 11:33 AM to 1:15 PM UTC,
  five times; Oct 27 10:02 PM; Oct 28 3:26 PM; Oct 29 1:47 PM). north#259 measured runs ended by a usage limit at
  $237, 9% of spend, cut off mid-turn.
- The 95% stop did not keep those windows from filling (in-flight work overshoots), and Tom reports it stranded
  capacity at 98% (2026-11-05). So the fix for P4 is to park and revive the work a full window stops, and to fail the
  lead over, not to stop launching early. Consequence for landing order: the 95% gate is removed only after parking
  (box 7) and hand-over (box 8) land, so no state of main has neither.

### 2.2 Weeks ran out before their resets

Every week recorded in the `usage` table since 2026-10-15 reached 100% before its reset (a Oct 29, c Oct 29, b Oct 31,
d Nov 3). Nothing was stranded at a weekly reset in that window; pacing toward a target therefore saved nothing, and
the order among accounts matters for which week's allowance expires unused when demand is lower, which P1's
soonest-reset-first rule handles.

### 2.3 Dollars to window points

Rules 1 and 2 compare a box's expected spend, in API-equivalent dollars, with window percentages. Hours on one
account with one window and both a usage rise and priced fleet turns (`ledger.db` runs priced at
`C/router/prices.json`, 2026-10-24 to 2026-11-01): the week rose 324 points against $2,157 (0.15 points per dollar,
152 hours; by account a 0.30, b 0.11, c 0.17, d 0.22), the five-hour window 1,063 points against $1,228 (0.87 per
dollar, 84 hours). Agent-tool workers and Tom's own sessions are not in `ledger.db`, so these are upper bounds, and
the spread by account is wide. The factor is therefore an input measured per account and window and recorded with its
n and date (box 4), never a constant; a p90 box of $4.54 is then about 0.7 weekly and 4 five-hour points.

### 2.4 Staleness bound

Box 4b keeps a reader that runs every minute (a header read takes about a second), so a reading older than 10 minutes
means the reader is broken, not slow. Without it the only idle reader would be the worker-ledger timer's
`agents usage --refresh` every 15 minutes (nixos-config `modules/worker-ledger/default.nix:44`), and token-only
account d, read only by headers, would never be read again. Pairs of readings of the same window at most 10
minutes apart (`usage` table, 2026-10-24
to 2026-11-04): the five-hour window rose 4 or more points in 1,251 of 18,634 pairs and 10 or more in 155 (0.8%); the
week never rose more than 6. Live sessions report on every turn, so on a busy account the newest reading is seconds
old; 10 minutes is the one bound past which a reading is `Stale`.

### 2.5 Breaker thresholds

Tom set them: a velocity warning at 50% of E before 25% of expected active minutes, a warning at 2x p90, a trip at 4x
p90, each held for two consecutive readings. Against the threads `runs` table (worker runs 2026-10-24 to
2026-11-04, `cost_usd`,
an issue's boxes grouped by tier and category, each box's expected cost the p90 of its cell with at least 5 boxes),
the issue-level ratio spend / expected was p50 0.20, p90 0.89, p95 1.31, p99 2.51, max 5.04 over 607 issues: 2x would
warn on 11 issues (1.8%) and 4x trip 2 (north#385 at 5.0x on $1.86 and north#523 at 4.4x on $2.40). The one multi-box
runaway, north#88 ($16.16 over 7 boxes, 3.6x), would warn and not trip. Box 6 gives the ledger its dollar p90, and box
10b re-measures these rates on API-equivalent cost before the breaker trips anything.

## 3. Design

### 3.1 The setting

One table in the operator file `south/agents/orchestration.toml`:

```toml
[usage.reserve]
claude = 0
```

A provider missing from the table keeps 0. A value must be a whole number from 0 to 100, a percent of the provider's
total weekly allowance; anything else fails the load with the key named. Tom crosses a reserve only by answering a
`north gate` question (`north gate open ... --question "Cross the claude reserve until TIME?"`); the crossing is the
gate's recorded answer in Tom's words, so no agent can write one, and it lasts until the time it names. `target_left`,
`band`, `thursday`, `[pace]`, `[pace.*]` and `enforce_from` are deleted.

Plan weights come from `[accounts.claude].plans` and `harness/model-catalog.toml`'s weekly multipliers (all four
accounts are max-20x today, so equal weights).

### 3.2 Inputs, all recorded with their time

```rust
pub enum Window { FiveHour, Week, Scoped(ModelFamily) }
pub enum Source { Stream, Header, Cli, Web }
pub struct Reading { pub row: RowId, pub account: Label, pub window: Window, pub used: Percent, pub resets_at: Option<Unix>, pub read_at: Unix, pub source: Source }
pub struct LimitEvent { pub account: Label, pub window: Window, pub resets_at: Option<Unix>, pub at: Unix }

pub struct Account {
    pub provider: Provider, pub label: Label, pub weight: f64,
    pub signed_in: BTreeSet<Host>, pub extra_usage_enabled: Option<bool>, pub extra_usage_spend: Option<Usd>,
    pub rates: Rates,
}
pub struct Rates { pub five_hour: Rate, pub week: Rate }
pub enum Rate { Account { points_per_usd: f64, n: u32 }, Pooled { points_per_usd: f64, n: u32 } }

pub enum Owner { Fleet, AgentTool { tool_use_id: String }, Tom }
pub struct Session {
    pub name: Name, pub owner: Owner, pub account: Label, pub family: ModelFamily, pub workstream: Option<Workstream>,
    pub box_id: Option<BoxId>, pub lead: bool, pub spent: Option<Usd>, pub active_minutes: f64,
    pub expected: Expected, pub parked_since: Option<Unix>,
}
pub struct Expected { pub p50: Usd, pub p90: Usd, pub minutes: f64, pub source: EstimateSource }
pub enum EstimateSource { Cell(String), CategoryPool(String), TierMedian(String), BriefEta { minutes: f64 }, Budget(Usd), None }

pub struct Request { pub provider: Provider, pub family: ModelFamily, pub owner: Owner, pub workstream: Option<Workstream>, pub expected: Expected, pub pin: Option<Label>, pub host: Option<Host> }
pub struct Credit { pub remaining: Usd, pub expires_at: Unix, pub read_at: Unix }
pub struct Settings { pub reserve: BTreeMap<Provider, Percent>, pub crossings: Vec<Crossing>, pub clears: Vec<Clear> }

pub struct World<'a> {
    pub now: Unix, pub accounts: &'a [Account], pub readings: &'a [Reading], pub limits: &'a [LimitEvent],
    pub sessions: &'a [Session], pub tripped: &'a [Trip], pub credit: Option<Credit>, pub settings: &'a Settings,
}
```

The shell, `usage::world()`, assembles a `World` from the `usage` table (stream, header, CLI and web rows; limit
rows), the fleet registry and the Agent-tool launch records (sessions), `north fleet accounts` per host (signed in),
the threads `runs` table and `worker-ledger` (spend, expected), the calibration table (rates) and the operator file.
The functions below read no clock, file or environment: `now` is an input.

A `Rate` measured on the account with at least 20 hours of paired readings is `Account`; otherwise the provider's
pooled measurement (`Pooled`) applies, labelled. An Agent-tool worker's `spent` is `None` until `worker-ledger`
ingests its transcript (the 15-minute timer), so the breaker sees it with that lag and in-flight accounting uses its
`expected` meanwhile.

### 3.3 The functions

```rust
pub const FRESH_FOR: Seconds = Seconds(600);
pub const LEAD_FAILOVER_AFTER: Seconds = Seconds(600);

pub fn view(world: &World, account: &Label, window: Window) -> View;
pub fn state(world: &World, account: &Label, family: ModelFamily) -> State;
pub fn free(world: &World, provider: Provider) -> Free;
pub fn rank(world: &World, request: &Request) -> Ranked;
pub fn place(world: &World, request: &Request) -> Decision;
pub fn assess(world: &World, previous: &Assessments) -> Assessments;
pub fn explain(decision: &Decision, time: &dyn Fn(Unix) -> String) -> String;
```

`rank` orders every account for a request with the reason each is placeable or not; `place` is `rank` restricted to
the request's host (when machine placement has chosen one) and takes the first placeable account. Every launcher
calls `place`; the tick calls `assess`. Both are pure, so a decision is reproduced from its recorded `World`; the
shell appends each `World` digest and decision to `~/.local/state/agents/usage/decisions.jsonl`.

#### `view`: one window of one account, first matching row wins

Readings of a window are totally ordered by `read_at`, then `used` (higher first, so a disagreement never opens
room), then `resets_at` (later first), then source (`Stream`, `Header`, `Cli`, `Web`), then `row`. The newest is the
first in that order.

| # | Condition | View |
| --- | --- | --- |
| 1 | a limit event for this window has `resets_at > now` | `Limited { resets_at }`, whatever the readings say |
| 2 | a limit event with no `resets_at` is newer than every reading of the window | `Limited { resets_at: None }` |
| 3 | no reading of the window ever | `NeverRead` |
| 4 | the newest reading has `resets_at <= now` | `Stale { read_at, why: reset since read }` |
| 5 | the newest reading has `|now - read_at| > FRESH_FOR` | `Stale { read_at, why: old }` |
| 6 | otherwise | `Fresh { used, resets_at, read_at, source }` |

#### `state`: one account for a request of one model family, first matching row wins

| # | Condition | State |
| --- | --- | --- |
| 1 | the account is signed in on no host | `SignedOut` (the standing sign-in rule applies) |
| 2 | week view is `Limited`, or `Fresh` with `used >= 100` | `WeekOut { resets_at }` |
| 3 | the family's scoped view is `Limited`, or `Fresh` with `used >= 100` | `ScopeOut { family, resets_at }` |
| 4 | five-hour view is `Limited`, or `Fresh` with `used >= 100` | `Paused { resumes_at }` |
| 5 | week view is `NeverRead` or `Stale` | `Unknown { window: Week, read_at }` |
| 6 | five-hour view is `NeverRead` or `Stale` while the week view is `Fresh` | `Open { five_hour: 0, not_started: true, ... }` |
| 7 | otherwise | `Open { five_hour, week, five_hour_resets, week_resets }` |

Exhaustion is checked before staleness, because the provider stops sending a five-hour window once the week is out
(1.11): an account with a fresh week at 100 is `WeekOut`, not stale. Row 6 covers the five-hour window that has not
started (no row, or a row at 0 with no `resets_at`); a five-hour window that is in fact full shows up as a limit
event on the first turn and row 4 then applies. A `NeverRead` or `Stale` scoped view never closes an account (scoped
rows come from the slower CLI read); an Opus box skips an account whose Opus week is out, a Sonnet box does not.

#### `free`: rule 1, the provider reserve

With accounts `A` of provider `p` and `W` the sum of their weights:

- `free = sum over a in A of weight_a x (100 - week_used_a) / W`; an account whose week view is not `Fresh`
  contributes 0 and is named.
- `in_flight = sum over unparked sessions s on p, other than Tom's, of weight_a(s) x week_rate_a(s) x
  max(0, p50_s - spent_s) / W` (with `spent_s` as 0 while unknown).
- For a request on account `a`: `after_a = free - in_flight - weight_a x week_rate_a x p50_request / W`.

| # | Condition | Gate |
| --- | --- | --- |
| 1 | the request's owner is `Tom` | `Pass` (the reserve is his) |
| 2 | reserve is 0 | `Pass` (only exhaustion stops anything) |
| 3 | a crossing for `p` is active at `now` | `Pass { crossed }` |
| 4 | `after_a >= reserve` | `Pass` |
| 5 | otherwise | `Reserve { free, in_flight, box, reserve }` |

A reset replenishes `free` through the next fresh reading.

#### `rank` and `place`: rules 1 and 2

`fits_a` is true when `(100 - five_hour_used_a) - five_hour_rate_a x (in-flight remaining on a + p50_request) >= 0`.
Placeable accounts are ordered in tiers, and within a tier by soonest week reset (none last), then label:

| Tier | Accounts | Warning |
| --- | --- | --- |
| 1 | `Open`, gate passes, `fits` | none |
| 2 | `Open`, gate passes, does not fit, extra usage not enabled | `MayPause { finish_by: now + minutes_request, resumes_at: five_hour_resets }`; this tier orders by soonest five-hour reset instead |
| 3 | `Unknown`, reserve 0, extra usage not enabled | `Unread { read_at }` |

`place`, first matching row wins:

| # | Condition | Decision |
| --- | --- | --- |
| 1 | the request's workstream is `Tripped` and not cleared | `Refuse(Tripped { workstream, spent, p90 })` |
| 2 | `pin` names no account of the provider | `Refuse(UnknownAccount)` |
| 3 | `pin = Some(a)` and `a` is placeable on the request's host | `Place(a)` with `a`'s tier warning, plus `NotFirst { first }` when `rank` puts another account first |
| 4 | `pin = Some(a)` otherwise | `Refuse(Pinned { a, state, first })` |
| 5 | some account is placeable on the request's host | `Place` the first by tier and order |
| 5b | an account is placeable but none is signed in on the request's host | `Refuse(NotOnHost { host, first })` |
| 6 | every account is `Paused`, `WeekOut` or `ScopeOut` for the family | `Refuse(AllPaused { family, earliest_resume })` |
| 7 | some account is `Open` but none passes the gate | `Refuse(Reserve { ... })` |
| 7b | an `Open` account is blocked only because it would pause with extra usage enabled | `Refuse(ExtraUsage { accounts })` |
| 8 | otherwise | `Refuse(NoneReadable { states })` |

Usage chooses the account first. Machine placement (cores) then picks among the hosts where that account is signed
in; if none can take the box, it calls `place` again with each host it would accept, in its own order, and the first
placement wins. A tier-3 placement on an unread account is caught by the first limit event, then parked or handed over
(P4), so a broken reader no longer stops all work as it did on 2026-11-04 (north#544). An account with extra usage
enabled is never placed where it might pause (tiers 2 and 3), since there a pause becomes paid spend.

The cloud lane is not a usage decision: today's rule (a code-only box in a public repository goes to anthropic-cloud,
`P/placement/gate.rs:462`) stays, and the account under it comes from `place`. A live prepaid credit only puts its
eligible boxes ahead of others in dispatch order while it lasts; the only credit expires 2026-11-05 7:59 AM UTC, so
no box builds more than reading it.

A refused launch resumes by itself because its retrier is named: a dispatched box is retried by the next dispatch tick;
a lead's refused launch registers a wait `usage:PROVIDER@earliest_resume` and is retried by `north fleet waits`.

With an estimate source of `None`, p50 and p90 are 0, the decision says "no estimate", and the breaker does not run.
A `BriefEta` estimate has `p50 = minutes x the tier's measured dollars per minute` and `p90 = p50 x the pooled
p90/p50 ratio of closed boxes`, both labelled.

#### `assess`: rules 3 and 4, each tick

Work in flight, for sessions whose owner is not `Tom` (Tom's sessions are never parked, handed over or broken),
first matching row wins per session:

| # | Condition | Action |
| --- | --- | --- |
| 1 | a lead whose account is `Paused` and `now - parked_since >= LEAD_FAILOVER_AFTER` | hand over as in row 3, before any worker |
| 2 | a session whose account is `Paused` | park with a registered wait `usage:ACCOUNT@resumes_at` (sets `parked_since`); `north fleet waits` revives it after `resumes_at` |
| 3 | a session whose account is `WeekOut`, or `ScopeOut` for its family, and `place` with `pin = None` places elsewhere | `north fleet launch --take-over` there with a `Follows:` brief, the lane being the durable state |
| 4 | every account is `Paused`, `WeekOut` or `ScopeOut` for every family in flight | nothing launches; one line to Tom with the earliest resume, once per episode |

Spend, per box (a `Follows:` chain is one box) and per workstream (an issue, or a lead's tree for work with no
issue). A workstream's p50, p90 and minutes are the sums over its boxes; a lead's own coordination spend is not a box
and is not counted. Spend and active minutes accrue only while a session runs turns, so parked or waiting time
accrues nothing. First matching row wins:

| # | Condition, true at this and the previous tick | Assessment |
| --- | --- | --- |
| 1 | estimate source is `None`, the workstream has no boxes, or p90 is 0 | `Unestimated` (named, never trips) |
| 2 | a recorded clear names a p90 and `spent < 4 x` that p90 | `Cleared { by }`, with the cleared p90 in force for rows 3 to 5 |
| 3 | `spent >= 4 x p90` | `Tripped`: no new launches on the workstream; the session is told once to finish its step, commit and report `Split:` or `Not done:` `Stop: budget` with what it learned |
| 4 | `spent >= 2 x p90` | `Warn`: a line to the lead and a Tom-visible line |
| 5 | `spent >= 0.5 x p50` and `active_minutes < 0.25 x minutes` | `Velocity`: a line to the lead |
| 6 | otherwise | `Within` |

A condition true at only one tick keeps the previous tick's assessment (no trip on a spike). A clear comes from a
newer brief for the workstream whose Budget line states a p90 (`Budget: E p50 $X, p90 $Y`), or from Tom's answer to a
`north gate` question naming one; nothing else clears. Every warning, trip, clear and box outcome is written to the
ledger, and `worker-ledger --recommend` reads them, so estimates recalibrate from actuals.

#### Expected spend E

The worker-ledger cell for the box's facets (p50 and p90 priced box cost, and minutes), else the category pool, else
the tier median, else the brief's ETA times the tier's measured dollars per minute, else `None`; the source is always
carried and printed. Minutes are active minutes where the run records them and wall minutes otherwise, and the
breaker compares a session's minutes on the same basis as its E, so the velocity rule never mixes the two. A
dispatched brief's Budget line states this E (replacing today's "twice the cell median, stop and split there"
text in `C/ops/dispatch/grounds.rs:15` and `:79-90`); a legitimately long box raises E with a newer brief.

#### Two launches at once

The shell takes one lock (`~/.local/state/agents/usage/decide.lock`), builds the `World`, calls `place`, and writes a
launch record before releasing it: a fleet launch writes its session into the registry; an Agent-tool spawn writes
`usage/launches.jsonl` keyed by `tool_use_id` in the PreToolUse hook, removed by the PostToolUse hook when the tool
failed and by the SubagentStop hook when the worker ends. The second decision sees the first in flight. The record
lives exactly as long as the session; it reserves nothing on a timer.

#### Other providers

The same functions take any provider whose adapter records readings in the `usage` table; Codex keeps its seam with
nothing new built, and Gemini's five-hour and weekly rows map to the same windows.

#### Properties the tests check

- Determinism: every function gives the same answer for any permutation of its input slices.
- Soundness: `Place(a)` implies `a` is in tier 1, 2 or 3 for the request's family and host, and tier 3 only with
  reserve 0.
- Reserve: with reserve 0, `place` refuses only when no account is placeable or the workstream is tripped; with
  reserve `r > 0`, no crossing and an owner other than Tom, `Place(a)` implies `after_a >= r`.
- Monotonic: raising the `used` of every reading of a window, aging every reading of a window together, adding a
  limit event, raising the reserve or adding in-flight spend never moves an account to a better tier.
- Override: a limit event with `resets_at > now` closes its window whatever the readings say.
- Exhaustion first: a fresh week at 100 yields `WeekOut` whatever the five-hour view.
- Scope: a `ScopeOut` for one family never refuses a request of another family.
- Warn, never hold: if some account is in tier 1 or 2 and the workstream is not tripped, `place` places.
- Extra usage: an account with extra usage enabled is never placed in tier 2 or 3.
- Breaker: a single-tick spike never changes an assessment; parked ticks never raise `spent`; a workstream with no
  boxes is `Unestimated`; Tom's sessions are never assessed.

### 3.4 What every launcher does

- `north fleet launch`: `place` with `pin` from `--account`; machine placement follows as in 3.3. `--pin` keeps its
  meaning for machine placement only.
- The Agent-tool spawn gate: `place` with `pin` = the lead's billed account (an in-process worker spends the lead's
  account) and `owner` from the session (`Tom` when the session is not a fleet session); a refusal names the account
  `north fleet launch` would use.
- `north dispatch`: each box goes through `north fleet launch`; dispatch decides nothing about usage.
- `agents plan --place`, revive, rebooted, the fleet-session timer, the account probe, model-watch and the
  cross-project timer (`C/ops/timers/cross_project.rs:99-168`) call `place`.
- The router picks the tier from the ledger with no pace input: a tier is open when `place` would place it.
- Fast mode: allowed when `place` places in tier 1.

### 3.5 What Tom is shown

`north usage` and the grounding readout print, per provider:

```
claude: reserve 0%, free 12% of the week
  a  week 100% (resets Thu 9:00 AM)  5h out with the week
  c  week 100% (resets Thu 7:00 PM)  5h 0% (not started)
  d  week 50% (resets Tue 3:00 PM)  5h 41% (resets 7:10 PM)
  b  reading 2 h old (Wed 12:50 AM); not counted as free
  warning north#88: spent $9.40, 2.1x its p90 $4.54
```

Each account shows week used and its reset, five-hour used and its reset, or its named state (`not read`, `reading N
min old`, `signed out`); then active warnings and trips, one line each. Nothing else: no burn, projection, "empty in",
pace line, target, band or held flag. Times are Tom's local 12-hour time, with the weekday when not today.

### 3.6 Alerts to Tom

One line each, once per episode: every account of a provider out for every family in flight (with the earliest
resume); extra usage enabled or showing nonzero spend on any account (at once); a breaker trip. A `SignedOut`
account follows the standing sign-in rule.

## 4. Deletion list

Whole files (about 15,900 lines with fixtures):

- `C/pace/`: `mod.rs`, `admission.rs`, `control.rs` (pace targets, allowed and held workers), `exhaustion.rs` (the
  95% five-hour gate and the 50 %/h burn cap), `target.rs` (after moving `policy_path`, `home_policy_path` and
  `seconds`), `window.rs`, `trail.rs`, `tests.rs`, `replay_tests.rs`, `fixtures/claude-c-replay.jsonl`; `limits.rs`
  after moving `default`, `prior_measured`, `handoff_ctx`, `handoff_arm_ctx` and `overtime_min` to `C/defaults.rs`.
- `P/`: `command.rs`, `gate_order.rs`, `grants.rs`, `grant_tests.rs`, `reservation.rs`, `storage.rs`, `process.rs`,
  `readout.rs`, `readout_tests.rs`, `sampling.rs`, `sampling_tests.rs`, `placement_tests.rs`, `placement/launch.rs`,
  `placement/survey.rs`, `placement/tiers.rs`, `placement/room.rs`, `fixtures/grant-replay.json`,
  `fixtures/placement-replay.json`, `fixtures/sampling-replay.json`.
- `bin/pace-control`; nixos-config `modules/pace-enforcement/` and its mentions in
  `hosts/whiterabbit/configuration.nix:66`, `tags.json:114` and `update-notify.test.sh:36`.
- `C/usage/gate.rs`, `C/usage/burn.rs` (the "burn X%/h, empty in" projection); `C/guard/spawn_capacity/usage.rs`.
- `bin/follows-release.lib.sh:38` and `bin/follows-release-gates.test.sh:7`'s `usage-gate.json` writes.
- State files, once nothing reads them, moved to `~/.local/state/agents/retired/` (not deleted): `pace-control.json`,
  `pace-control.jsonl`, `pace-control.lock`, `pace-sampling.lock`, `usage-gate.json`.

Parts of files:

- `P/placement.rs`: `place_spawn`, `place`, `release`, `operation`; the machine helpers stay.
- `P/placement/gate.rs`: `window_refusal`, `judged`, `verdict`, `standing_verdicts`, `verdicts`, `best*`,
  `roomiest_here`, `Basis`, and the account re-pick in `sign_in_on_machine`, `keep_here_on_local_account` and `run_on`
  (`:110-152`), which 3.3's host loop replaces; `Decision` and `Target` (shared with `hosts.rs:150`) keep only
  machine and lane fields. What remains of `P/` becomes `C/ops/placement/`.
- `src/cli/fleet/placement.rs`: `exhausted_refusal`, `account_refusal`, `TIE_WEEK_POINTS`, the verdict lines.
- `C/guard/spawn_capacity.rs`: the grant, reserve, confirm, expired-grant and pace-refusal path; the cores and brief
  checks stay; `C/guard/worker_handoff/spawn.rs:68-90` and `C/guard/spawn_state.rs:17` move from grant confirmation
  to the launch record of 3.3.
- `C/ops/dispatch/`: `survey.rs`'s account survey and `PACE_FRESH_S`; `plan.rs`'s `charge`, `takes`, `burn_held`,
  `rank`, `pin`, `best_account`; `slots.rs`'s re-exports of `Charge`, `PlanSize`, `Room`; `grounds.rs:15` and
  `:79-90`'s Budget text (rewritten to state E).
- `C/router/`: `ctx.rs` `metrics`, `price`, `exhausted`, `stale`, burn, `target_left`, `band`, `thursday`,
  `drain_behind`, `money_ceiling`; `route.rs:42` `launch_refusal`, `:55` `route_account`, `:77` the pace rank,
  `:132` fast floor, `:146` `money_refusal`; `choose.rs:246-271` `closed`'s pace and usage-credit text and
  `pace_warning`, `:158` the pace condition in `explorable`; `plan.rs:194-196` the stale failure; `policy.rs:23-24`;
  `C/usage/gate.rs:99-107` money refusal (3.3's extra-usage rule and 3.6's alert replace it).
- Nudges driven by pace: `GRANT-EXPIRED-HOLDING` (`P/grants.rs:383`) and the PACE-HELD / PACE-RESUMED phone notice
  (`P/sampling.rs:375`). The capacity watchdog sends no pace-driven nudge (1.6) and stays.
- `C/grounding/live.rs`: the controller readout, replaced by 3.5's lines.
- `C/health.rs:92-102` check `pace`; the `usage` check stays at `FRESH_FOR`.
- `C/usage/refresh/mod.rs:43-55`: the `pace-control.lock` and gate rewrite after a refresh.
- CLI: `north pace` (all subcommands), `agents pace`, `agents usage --gate`, `north usage --write-pace-gate`,
  `north usage --fast`'s headroom rule.
- `north flow` has no account pace or projection line (1.6); nothing to delete there.
- `harness/defaults.toml`: `placement_drain_margin`, `reservation_fallback_points`, `usage_stale_min`,
  `money_ceiling`, `plan_stop`, `fast_floor`, `pace_target_left_claude`, `pace_band`, `pace_state_dwell_min`,
  `pace_burn_window_min`, `five_hour_burn_cap_pct_h`, `spawn_reservation_ttl_s`, `grant_reservation_hold_s`.
- `south/agents/orchestration.toml`: `[pace]`, `[pace.codex]`, `[pace."claude:a"]`, `enforce_from`, `target_left`,
  `band`, `thursday`; `south/agents/rules.toml:747`'s pace-control source; `south/agents/routines/pace-enforcement.md`.
- Budget-as-hold text: `harness/skills/workers/references/run-and-land.md:15` ("a pacing signal") and
  `C/ops/dispatch/grounds.rs:15` `AT_THE_LINE`.
- Skill and agent sentences telling a lead to hold on a projection, replaced by section 6's text:
  `harness/skills/workers/SKILL.md:10`, `references/tiers.md:5`, `references/cloud.md:12` and `:20`,
  `C/usage/gate.rs:15`, `C/pace/admission.rs:26-37`, `C/guard/spawn_capacity.rs:96`, the feature docs
  `docs/features/02-staffing-and-routing/0005`, `0006`, `0016`, `docs/features/03-fleet-sessions/0006`, `0011`,
  `docs/harness.md:101`, `docs/rationale/hooks-and-runtime.md:56`, `nexus-alerts/README.md:21`.

## 5. Boxes, in landing order

The live sampler defect (north#544) is in flight as its own box, usage-reader-fix, outside this list.

Each box is at most 45 minutes, lands alone through `safe-push --to main`, leaves main building and working, and
updates the integration tests (`tests/it/*`, 1.11) it breaks in the same change. `U` is the new `C/usage/decide/`.

1. **View and state.** `U/{mod,world,view,state}.rs`: 3.2's types, `view` and `state`, one example test per row,
   properties for determinism, override, exhaustion first, stale, scope and monotonic.
   Done when: `cargo test -p north-core usage::decide::` passes on origin/main; a mutant that swaps `state` rows 2 and
   5 fails a test.
2. **Free, rank and place.** `U/{free,rank,place,explain}.rs` with one test per row and tier, properties for
   soundness, reserve, monotonic, warn-never-hold and extra usage.
   Done when: the same filter passes; a mutant that drops `free` row 1 fails a test.
3. **Assess.** `U/assess.rs`: both tables, two-tick confirmation, parked ticks, Tom exclusion, empty workstreams.
   Done when: tests cover each row and the breaker properties; no caller yet.
4a. **Stream and limit rows.** The stream reader writes each `rate_limit_event` (every unified window, scoped
   included, plus `status`, `resetsAt`, `rateLimitType`) to the `usage` table as `source = stream`; a `rejected`
   status or a 429 with `resetsAt` writes a limit row.
   Done when: on gex45 a running Claude worker produces `source = stream` rows within a minute
   (`sqlite3 -readonly ~/.local/state/threads/threads.db "select max(ts) from usage where source='stream'"`).
4b. **One-minute reader.** `north usage sample` (header reads for token accounts, `-status` and `extra_usage`
   `is_enabled` recorded, CLI reads for the rest); nixos-config gains its one-minute timer on every host that holds a
   Claude login, landing after this north change.
   Done when: `systemctl --user list-timers` on the laptop and gex45 shows the timer, and `select max(ts) from usage
   where account='d'` stays under 2 minutes old for 10 minutes.
4c. **Rates.** Points per dollar per account and window, measured from rises and priced turns into the calibration
   table with n and date, with the pooled fallback. `north threads calibrate --write` records kind `rate` rows,
   cell `ACCOUNT five_hour|week` and `pooled five_hour|week`, n the hours holding both a rise (consecutive readings of
   one window instance, `session` and `weekly_all`) and priced `run_costs` turns, value points per dollar, `at` the
   date; `rates_of` reads an account's own rate at 20 or more hours and the pooled one below that (3.2's `Rate`).
   Done when: `calibration` holds a five-hour and a week rate per account, each with its n.
5. **Shell and log.** `usage::world()`, the decision lock, launch records (registry for fleet launches, the hook
   file for Agent-tool spawns), `usage/decisions.jsonl`, and `north usage decide --dry-run BRIEF`.
   Done when: a test runs two real decisions against a fixture registry under the lock and the second lists the
   first in flight; `--dry-run` prints `explain`'s line.
   The shell is `usage/shell/`: `Shell::world` reads the usage table (header, CLI, stream and web rows by their source text),
   the registry through the `Registry` trait (`FleetRecords` reads the live fleet session records) and `launches.jsonl`;
   `Shell::decide` holds `decide.lock` across world, `place`, the caller's launch record and the `decisions.jsonl` line;
   `dry_run` takes the lock and records nothing. Until their boxes land, sign-in is this host, rates are the pooled
   1.0 points per dollar, limit events are empty (4a) and `spent` is unread (6); `[usage.reserve]` is read from the
   operator file when present.
6. **Expected spend.** The ledger gains p50 and p90 priced box cost and minutes per cell with the fallback chain;
   `worker-ledger --recommend` prints E with its source; dispatched briefs' Budget lines state E.
   Done when: `north worker-ledger --recommend 'Category: feature ...'` prints `E p50 $X p90 $Y, N min (cell ...,
   n=K)`; a brief with no cell prints the fallback it used.
7. **Park and revive.** `north fleet wait` gains `usage:ACCOUNT@TIME`; the tick parks a session whose account is
   `Paused` and `north fleet waits` revives it after the reset.
   Done when: a fixture session parked on a fixture limit event is revived once the fixture clock passes
   `resumes_at`, and its spend did not accrue while parked.
8. **Hand-over and lead failover.** The tick hands a `WeekOut` or `ScopeOut` session over through `--take-over` with a
   `Follows:` brief, a lead first once paused 10 minutes; the all-out line to Tom.
   Done when: a fixture fleet with a lead and a worker on a week-out account yields two take-over launches, the lead's
   first, and one Tom line naming the earliest resume when every account is out.
9a. **Fleet launch.** `north fleet launch` and `agents plan --place` call `place`; `gate.rs`'s account steps and
   re-pick go, `Decision` and `Target` keep machine and lane fields; `exhausted_refusal`, `account_refusal` and
   `TIE_WEEK_POINTS` go. Needs 7 and 8.
   Done when: `north fleet launch --test --dry-run` prints `explain`'s line; `rg 'exhausted_refusal|account_refusal|
   TIE_WEEK_POINTS|sign_in_on_machine' src/cli/fleet crates/north-core/src/ops/pacing/placement/gate.rs` is empty.
9b. **Other launchers.** Revive, rebooted, the fleet-session timer, dispatch, the probe, model-watch and the
   cross-project timer call `place`; dispatch's survey, charge, takes and `burn_held` go.
   Done when: `rg 'roomiest_here|box_reserve|PACE_FRESH_S|burn_held' crates src` is empty and dispatch tests pass with
   no account fixture.
10a. **Spawn gate.** The Agent-tool gate calls `place` with the lead's account and owner; grants, reservations,
   confirm and the pace refusal leave `spawn_capacity`; the hooks write and remove launch records.
   Done when: the spawn-gate tests assert one refusal per non-placeable state and an admit in each tier;
   `rg '"op": "(reserve|grant_for|confirm|expired_grant_hold)"' crates` is empty.
10b. **Breaker tick.** The tick runs `assess` for spend, tells a tripped session once, records warnings, trips, clears
   and outcomes in the ledger; clears come from Budget lines and Tom's gate answers.
   Done when: a replay of north#385's runs trips on the second tick past 4x p90 and not the first; a newer brief whose
   Budget line states a higher p90 clears it.
11. **Router.** `agents plan` routes with no pace input: `metrics`, `price`, `route_account`, the under-pace rank,
   `explorable`'s pace condition, `pace_warning`, `closed`'s usage-credit ask, `money_refusal` and the stale failure
   go.
   Done when: router tests pass with a fixture whose accounts sit at 60% week, "at or over pace" by the old line, and
   their tiers still open; `rg -i 'under pace|usage credits|money_ceiling' crates/north-core/src/router` is empty.
12. **Readouts and alerts.** `north usage` and the grounding readout print 3.5; the alerts of 3.6; `burn.rs`, the burn
   column, `--gate`, `--write-pace-gate` and the fast floor go.
   Done when: `north usage` on gex45 prints the provider line and one line per account as in 3.5; the usage tests pin
   the format and assert no "empty in" or "burn".
13a. **Timer first.** nixos-config drops `pace-enforcement` (module, `configuration.nix:66`, `tags.json:114`, the
   `update-notify.test.sh:36` fixture), landing before any north deletion.
   Done when: `systemctl --user list-timers` on the laptop and gex45 shows no `pace-enforcement`.
13b. **Delete the controller.** `C/pace/`, `P/sampling.rs`, `readout.rs`, `process.rs`, `command.rs`, `storage.rs`,
   `gate_order.rs`, `bin/pace-control`, `north pace`, `agents pace`, the health `pace` check and the phone notice;
   `limits.rs`'s survivors move to `C/defaults.rs`.
   Done when: `rg -l 'pace::|pace-control' crates src harness bin` is empty; `north health` passes on both hosts.
13c. **Delete grants, reservations and legacy placement.** `grants.rs`, `reservation.rs`, `placement/{launch,survey,
   tiers,room}.rs`, the fixtures and the defaults entries; `P/` becomes `C/ops/placement/`.
   Done when: `rg -l 'ops::pacing|reservation_fallback|grant_reservation' crates src harness` is empty.
14. **Operator file, docs and skills.** `orchestration.toml` gains `[usage.reserve]` and loses the pace keys;
   `rules.toml:747` and `routines/pace-enforcement.md` change; the feature docs and section 6's text land; retired
   state files move to `retired/`.
   Done when: `north usage` shows the reserve read from south main; `rg -i 'pace line|projection|burn cap|grant'
   docs/features harness/skills` returns no usage rule.

Order: 1, 2 and 3 first (pure, no callers); 4a, 4b, 4c, 5 and 6 need 1 to 3 and not each other; 7 and 8 need
4a and 5; 9a needs 7 and 8 (so the 95% gate goes only once parking and hand-over exist); 9b needs 9a; 10a needs 5;
10b needs 6 and 10a; 11 and 12 need 5; 13a needs 4b; 13b needs 9b, 10a, 11, 12 and 13a; 13c needs 13b; 14 needs 13c.

## 6. Skill and policy text

- `harness/skills/workers/SKILL.md:10` becomes: "`north fleet launch` places every worker by usage: the provider's
  reserve, then an account whose five-hour window fits the box, soonest weekly reset first. Never choose an account by
  hand and never hold a launch on a projection, pace line or burn; a placement warning is information, not a hold."
- `references/tiers.md:5`: drop the headroom sentence; account and fast mode come from placement.
- `references/cloud.md:12`: "placement decides the account (`north usage decide --dry-run` explains it)"; `:20` "the
  account placement picks"; the lane rule itself is unchanged.
- `references/run-and-land.md:15` and `references/brief.md` (briefing and ETA): the Budget line states the box's E
  (p50 and p90); the breaker warns at 2x p90 and trips at 4x; a tripped worker finishes its step, commits and reports
  `Split:` or `Not done:` `Stop: budget` with what it learned; a lead clears a trip only with a newer brief stating a
  higher p90, Tom with his gate answer; a velocity warning means re-read the brief, not stop.
- New line in the workers skill: "A placement refusal is final for that launch; act on its one line (another account,
  the named reset, or `Blocked:`), and never re-derive usage from numbers. A parked worker is revived by
  `north fleet waits`; do not restart it by hand."
- The session standing order of 2026-10-16 ("never launch on an account whose window is exhausted or burning toward
  exhaustion in under 2 hours ...") is Tom's text; box 14 asks the lead that maintains the fleet brief to replace it
  with "every launch goes through `north fleet launch`, which applies the usage rules; nothing else holds a launch",
  since its burning clause is a projection the new rules forbid.

## Review disposition

An Opus-high planner reviewed the revised doc against the code at 7c2a76350, the threads `usage` and `runs` tables,
`ledger.db`, nixos-config and south (2026-11-04). 19 findings; all accepted except where noted.

1. Box 13 deleted the only per-minute reader, leaving token-only d unread: box 4b adds `north usage sample` on a
   one-minute timer before any deletion; 2.4 now rests on it.
2. Staleness was checked before exhaustion while the provider drops the five-hour window once the week is out:
   `state` now checks week, scope and five-hour exhaustion first; a missing five-hour window beside a fresh week is
   "not started"; `resets_at` of none sorts last.
3. Refusing unread accounts broke "reserve 0 never stops before exhaustion": with reserve 0 an unread account is
   placeable in tier 3 with a warning, ranked below every fresh account; the reserve gate still counts it as 0 free.
4. Fix the live defect now: it is in flight as its own box (usage-reader-fix, north#544), ahead of this plan.
5. Timer landing order was reversed and gex45 was missed: box 13a removes the timer in nixos-config before 13b
   deletes `bin/pace-control`; 1.4 names both hosts and the other mentions.
6. `place` could not run ahead of machine placement unchanged: `World` carries per-host sign-in, `rank` and `place`
   take the host, and 3.3 states that usage picks the account first and machine placement loops hosts through `place`;
   box 9a rewrites `Decision` and `Target`.
7. Row 2 ended the cloud lane on credit expiry: the lane rule stays as today and the account under it comes from
   `place`; the credit only orders eligible boxes. This keeps Tom's "spent first" without making the lane depend on
   a credit that expires 2026-11-05.
8. Agent-tool workers had no in-flight record or spend: the hooks write and remove a launch record keyed by
   `tool_use_id`; their spend reaches the breaker through `worker-ledger` with up to a 15-minute lag, stated in 3.2.
9. A lead with no workers tripped on a p90 of 0: an empty workstream or zero p90 is `Unestimated`; lead coordination
   is not counted; `BriefEta` defines p50 and p90.
10. Tom's sessions were parked, handed over and checked by the breaker, and his spawns hit his own reserve: `Owner`
    excludes them from `assess` and passes the reserve gate for him.
11. Box 9's grep matched code later boxes own: 9a and 9b grep only the files they change; the full grep is in 13b.
12. Box 5's dry-run could not show the first launch in flight: the test runs two real decisions against a fixture
    registry.
13. Monotonicity was false when aging one reading and determinism had a tie hole: the property ages or raises every
    reading of a window together, and the order adds `resets_at` and the row id.
14. A warn-never-hold launch with extra usage on spends money: box 4b reads `is_enabled`; such an account is never
    placed where it may pause; the alert fires on enabled, not only on spend.
15. Missed deciders, readers and text: listed in 1.11 and added to section 4 and box 14.
16. Budget line meaning: the dispatched Budget line states E (p50 and p90); a clear names a p90; Tom's clear is a
    gate answer naming one.
17. Active minutes are mostly missing and `cost_usd` was mis-attributed: E's minutes are active where recorded and
    wall otherwise, compared on the same basis; 2.3 and 2.5 cite the threads `runs` table.
18. Smaller gaps: rates have a pooled fallback; the all-out line is per family; `parked_since` is an input and the
    lead failover row precedes parking; a pin that is not first warns `NotFirst`; crossing and clearing go through
    Tom's gate answers; refused launches name their retrier; 3.5 shows unread, stale and signed-out accounts.
    `MayPause` keeps "may pause before it finishes (by T)" rather than a pause time: a pause time needs a burn-rate
    forecast, the projection the design forbids, while the finish time is the box's own estimate.
19. Boxes over 45 minutes: 4, 9, 10 and 13 are split into 4a to 4c, 9a and 9b, 10a and 10b, 13a to 13c, each
    landing alone with main building.
