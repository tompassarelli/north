# Concerns: standing judgments with history, one registry and four views

Design of 2026-11-07 for north#709, which asks that north be supervised as standing concerns, things managed indefinitely, rather than as tickets that close and are forgotten. An outside review proposed concerns with states, telemetry kept apart from thresholds, a hold-the-line rule on close and four supervisor views. This note adopts that on the primitive north already has, `goals.toml` and `north goals check` (docs/rationale/targets.md, "Goals as checks"), and decides each part of it. It adds nothing beside that primitive: no new database, no second registry, no third learning system, and no second keep-or-revert judge.

## Terms

A **concern** is one named judgment north keeps forever: a measured value, the line it is judged against and what north does when it leaves green. "Goal" read as a thing hit and done, so the primitive is renamed (north#709, 2026-11-07): `north goals` becomes `north concerns`, each project's `goals.toml` becomes `concerns.toml`, `[[goal]]` becomes `[[concern]]` and the crate `north-goals` becomes `north-concerns`, with every caller updated in the same change and no alias. Two kinds replace the classes:

- A **floor** is today's contract: a line the value must not cross. `direction` says which way is better (`down` for a latency, `up` for a share), and `bound` is the line.
- A **climb** is today's debt-allowed goal: a value being moved in a `direction` toward a `target`, with an optional `worst` (today's `ceiling`) past which it reads red, at least one owner and a `revisit` trigger. The policy climber (crates/north-climb, docs/rationale/policy-climber.md) is one climber of one such value, the cost per kept box; the shared word is deliberate.

`max` and `min` go: `direction` plus `bound` (floor) or `target` (climb) say the same thing once, for both kinds.

Every concern also carries:

- How it is measured, exactly one of:
  - `oracle`: a command whose first output line is one number, run by the concerns sampler. Further lines are the detail (which lanes, which hosts), carried into messages and views.
  - `producer`: the program that already measures it in its own run and records its samples (`north land doctor`, `north flow`), so no second probe runs. A doctor dry run probes every lander place; running one per concern every 10 minutes would repeat that a dozen times.
- `window`: the span the measurement averages over (`0` for a reading of now, `24h` for a ready bar, `7d` for the detection rate). The climb measurement below waits a window after a landing before it reads the effect.
- `reads`: `tree` when the oracle reads only the checkout, so a landing can turn it red and the after-landing gate may attribute and revert (crates/north-landing/src/land/after/goals.rs); `live` when it reads the running system. Today the gate attributes every contract, including `landing-p90` and `main-green-latest-verdict`, which read live records: it runs them on the landed tree and at the base, and a live value that flips between those reads can blame an innocent commit. Only `tree` floors are attributed.
- `host`: the role whose machine measures it, the roles `harness/loops.toml` uses (`ledger`, `lander`, `here`), because the lander's facts, the ledger and the fleet's records live on different machines and no host reads another (docs/rationale/learning-loops.md). The role resolves to one host, so each concern has one series.
- `warn` (optional): a value between green and the line (`bound` for a floor, `worst` for a climb) at which the concern reads drifting whatever its slope.
- `posture`: how loudly it speaks, the dial Tom turns without touching the measurement:
  - `watch`: views only;
  - `tell`: the proxy hears red (today's behaviour, the default every existing entry gets);
  - `early`: the proxy hears drifting and red;
  - `page`: as `early`, and red also goes out as a `supply-alert` push. Of the alarms folded here only tier routing pages today; lane gc, land pause and the supply-chain scan send their own pushes and are not concerns.
- `owners` (at least one on a climb, optional on a floor): the issues that answer for the concern. A list, because a priority issue closed against an existing concern adds itself without dropping the hold an earlier issue has.
- `pays` (climb only): unchanged, the paths whose lanes pay it.
- `remedy` (optional): the program that acts when this line is crossed (`north land doctor`). The line is then also a decision constant of that program's loop, which changes what the ratchet may do with it.

## History

**Store.** Samples go in one table of the threads ledger, the store every cross-host reading already uses (`land_tempo` is the precedent, crates/north-threads/src/threads/landing/mod.rs):

```sql
CREATE TABLE IF NOT EXISTS concern_samples(
  project TEXT NOT NULL,
  concern TEXT NOT NULL,
  sampler TEXT NOT NULL,
  host TEXT NOT NULL,
  at TEXT NOT NULL,
  value REAL,
  why TEXT NOT NULL DEFAULT '',
  detail TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL CHECK(state IN ('green','drifting','red','repairing','unmeasured')),
  PRIMARY KEY(project, concern, host, at)
) STRICT;
```

`value` is NULL and `why` holds the failure when a reading fails; `detail` holds the detail lines; `state` is the judged state, kept for views and audits. `sampler` names the writer (`concerns-tick`, `land-doctor`, `flow`, `concerns-ledger`). Each sampler writes one batch per run through `north threads concerns record` (JSON lines on stdin), so the threads client carries it to the ledger host and spools it when that host is unreachable, as every other ledger write does.

Readers (`--trend`, the views, the dividend) open `threads_database()`: the ledger itself on the ledger host, else the page snapshot, refreshed every 15 minutes. A 12 h window does not notice that lag, and `--trend` prints the current value from its own live run. The judge does not read the snapshot: it needs the previous sample's state, which a 15-minute-old copy can miss, so each sampler keeps the judged state and episode of every concern it measures in its own state file, as the goal tick keeps `north/goals/tick.json` today. On gex45 the snapshot is 0 bytes since 2026-10-27 and `north scoreboard --ready` fails there with "no such table: landings"; lane 2's Done when is read on a host whose snapshot reads, and the broken pull is its own issue (north#712).

Alternatives that lost:
- North's own `ledger.db`: one per host, so a lander concern and a fleet concern would live in two files no single view reads.
- A JSON-lines file under the state root, as `ready-priority.jsonl` does: no index and no cross-host home, and every reader re-parses it.

**Cadence.** One sample per concern every 10 minutes, the goal tick's existing `CHECK_EVERY`. The fleet tick samples the `here` oracles; `north land doctor`, which runs every 2 minutes on the lander host, records its concerns at most every 10 minutes from the run it already does; `north flow`'s timer records its concerns from each run; a 10-minute user timer on the ledger host runs `north concerns sample --role ledger`, added in nixos-config by the lane that adds the first ledger concern, since the nightly `north tune` unit there is too slow.

**Retention.** Raw samples are kept 35 days and deleted by the record verb in the same transaction that writes a batch. 35 days covers the 7-day slope, the ratchet's 7 days and the dividend's 30-day lane window with room to spare. About 50 concerns at 144 samples a day is about 250 000 rows at the cap, a few tens of megabytes in a ledger of 136 MB. No downsampled tier: nothing reads past 35 days.

**A stale sampler fails `north health`.** docs/feedback-loop.md's Recorded readings get one row per sampler (`SELECT max(at) FROM concern_samples WHERE sampler = 'NAME'`, cadence 10m for the tick, the doctor and the ledger timer, 15m for flow), so one sampler stopping fails the silent-signal check even while the others write.

## The four states

Each sample's value is judged in order:

1. **Unmeasured** (shown, not one of the four): the measurement failed, or the newest sample is over 1 hour old. A concern unmeasured for an hour tells the proxy at posture `tell` and above, as "not read" does today.
2. **Red**: a floor crosses its `bound`, or a climb crosses its `worst`. A climb with no `worst` is never red. A `tree` floor reads red on the first red sample (the landing gate already confirms it twice); a `live` concern needs two red samples in a row, the two-run rule `north flow`'s alerts use today, so one slow reading does not message anyone.
3. **Repairing**: red, and the 12 h slope moves significantly back toward the line.
4. **Drifting**: not red, and either the value sits inside the `warn` band, or the slope is significant toward the line and, at the 24 h slope, the value would cross it within 7 days. For a climb with no `worst`, drifting is a significant 24 h move away from the target.
5. **Green**: otherwise.

Separately from its state, the judge says whether a climb **meets** its target. The ready verdict needs that answer: a ready bar short of its target is a green climb that does not meet.

Leaving drifting, repairing or red needs two samples in a row that say so, so a concern does not flap at a boundary.

**Slope that stays quiet on noise.** Each window's slope is the Theil-Sen estimate, the median of the slopes between every pair of samples, which one outlier cannot move. 12 h and 24 h use the raw samples (72 and 144); 7 d uses hourly medians (168 points), keeping the pair count small. A slope counts as significant only when all of these hold:

- the window holds at least 6 samples spread over at least half of it;
- the move it predicts across the window, slope times window, exceeds twice the noise. The noise is 1.4826 times the median absolute deviation of the samples from the Theil-Sen line (a robust standard deviation), but never less than a floor: 1 when every sample is a whole number (counts, 0 or 1 verdicts), else 1 % of the line's magnitude. Without the floor a step-shaped series has a deviation of 0 and every step reads significant;
- the 12 h slope points the same way as the 24 h slope, so a move has persisted.

The 24 h slope decides drifting, the 12 h slope confirms it and decides repairing, and the 7 d slope catches a creep too slow for a day to show: a significant 7 d slope toward the line with a crossing within 7 days also reads drifting. A count that steps (0 Python files, then 1) reads red on its line, the right answer for a count, and never drifting on the step itself.

Alternatives that lost:
- Ordinary least squares: one bad reading (an oracle that timed out into a fallback, a landing stuck behind a host reboot) swings the slope and fires drifting.
- A fixed percentage change threshold: the right threshold differs between a load average and a landing p90; the series' own noise sets it.

`north concerns check --trend` (`north goals check --trend` until the rename lands) prints, per concern: state, current value against its line and warn band, and for 12 h, 24 h and 7 d the median and the slope per day.

## One registry, one judge

Four places judge health today, each with its own thresholds and its own judge. The registry kind stays per project (targets.md: projects own their goals, so the file sits beside the README it is drawn from); "one registry" means every threshold is a `[[concern]]` in a `concerns.toml` and one function, `north_concerns::verdict::judge`, decides state and whether a target is met for all of them. Measurement stays where the domain lives (the scoreboard measures readiness, the doctor reads the lander, `north flow` reads the queues). That is the review's telemetry-apart-from-thresholds split: the domain code measures, concerns.toml holds the numbers, north-concerns judges.

**Scope.** `north concerns` reads the concerns file of every active project (not set aside in orchestration.toml, the rule the ready table already uses) into one view and one dispatch order, so priorities are weighed across the studio. The fixed `PROJECTS` list goes, with its uses in crates/north-goals/src/tick.rs and crates/north-landing/src/land/after/goals.rs. A project with no file contributes nothing and is not a problem line (makgora has none today).

**A registry that does not parse.** Today the after-landing run reads goals.toml only after the tree has landed, so a broken file reaches main. With the doctor reading its lines from the file, a broken file would also stop the lander's repairs while the fix waits on that lander. Two rules close it: the land check parses concerns.toml before a lane lands and refuses one that does not parse, and every reader that acts on lines (the doctor) keeps the last copy that parsed and uses it, with one unknown-state page, until a good one lands.

**What stays outside.** `north flow detect`'s self-detectors and their `detector_*` thresholds in harness/defaults.toml judge events, not standing values: each records an incident when it fires. They stay detectors; the detection-rate concern below counts them. Decision constants that act (admission's `saturation::QUEUED_LIMIT` and `build_disk_free_gib`, the doctor's `PATIENCE_SECS`, `NO_VERDICT_PROMOTIONS` and `GOOD_KEPT`) stay with their loops, because a loop tunes them; a concern judging the same quantity has its own line, which may sit beside the constant.

### Ready bars

The nine `ready` goals become climbs in north's concerns.toml with `direction`, `target` = today's bar, no `worst`, `window = "24h"`, `host = "here"`, owners north#641, `revisit` "north#641's ready verdict holds three days in a row", and the oracle `north scoreboard --ready --metric NAME`, a new flag that prints the worst reading over the ready repositories (north and makgora) for the last 24 h with one detail line per repository, and fails when no repository has a record:

- `ready-landing-p90`, `ready-verification-p50`, `ready-verification-p90`, `ready-rework`, `ready-first-try`, `ready-ask-to-landing`, `ready-corrections`, `ready-priority-now`, `ready-idle-account`.

src/cli/scoreboard/ready.rs keeps its measurement (`Records`, `Bar` with labels and `quiet_passes`, days in a row from per-day windows of records, one table per repository) and asks `judge` whether each repository's window meets the concern's target. Deleted: the `Goals` struct and its `read`, `parse` and `of`; `Standing::of`'s own comparison; the bound arithmetic in `threshold`, which formats the concern's line instead; the `ready` class, its parse arm in registry.rs, the `class != Ready` filter in `check` and the ready arm in `judge`. `landing-p90` (last 20 landings) and `ready-landing-p90` (last 24 h) stay two concerns because their windows differ; a later pair on the same quantity and window merges.

### Doctor bounds

The doctor's bounds become floors in north's concerns.toml with `producer = "north land doctor"`, `remedy = "north land doctor"`, `host = "lander"`, `reads = "live"` and `posture = "watch"`, because the doctor already remedies each known state and pages once per unknown one, and a second message for the same episode would be noise; they still show in Concerns, and in Decisions when red. Each records the worst subject's value with one detail line per subject over:

- `lander-oldest-queued-lane` (20 min), `lander-since-landing` (30 min), `lander-round-silence` (15 min), `lander-link-lag` (2 h), `lander-link-held` (15 min), `lander-slot-hold` (15 min past the land-check ceiling, today's `SLOT_GRACE_SECS`), `lander-disk-free` (10 % free), `lander-stale-worktrees` (0), `lander-infrastructure-failures` (0), `lander-unserved-queues` (0), `lander-toolchain-from-source` (0), `lander-promotion-no-verdict` (0).

The doctor reads these lines from the registry at start and judges each subject with `judge`; its remedy choice per state (`bounds::judge_queue` and its siblings) stays, since choosing a remedy is diagnosis, not a threshold. Deleted from crates/north-lander-doctor/src/bounds.rs: `OLDEST_QUEUED_SECS`, `SINCE_LANDING_SECS`, `ROUND_SILENCE_SECS`, `LINK_LAG_SECS`, `HELD_SECS`, `SLOT_GRACE_SECS`, `DISK_FREE_PERCENT` and `Bound::limit`; `SLOT_DEADLINE_SECS` becomes the ceiling plus the concern's line, including in the text `source_builds` writes. The doctor loop in harness/loops.toml names these concerns instead of the deleted constants, so the loops inventory still accounts for every line the doctor acts on.

### Flow alerts

Each `north flow --stall-alarm` alert becomes a floor with `producer = "north flow"`, `host = "here"` and `reads = "live"`. The repository alerts read the row `north flow` already writes per repository each run (`agents/flow/REPO.json`, crates/north-ops/src/ops/flow/measure.rs); the flow run records each concern's count, worst across repositories, with one detail line per lane, host or category:

- the stall notice (main idle and lanes waiting past the adaptive limit, or a landing job past twice its median) → `landing-stalled`, 0, `tell`;
- the capacity guards `actions`, `wisp` and `reuse` ("Underused capacity") → `capacity-underused`, 0, `tell`;
- the `pins` guard ("Broken pin") → `pins-broken`, 0, `tell`;
- stranded lanes (stopped sessions' unlanded lanes after 15 minutes) → `lanes-stranded`, 0, `tell`;
- refused land branches with no live session → `lanes-refused-unowned`, 0, `tell`;
- a usage sampler that has not read a provider for ten minutes → `usage-sampler-stale`, 0, `tell`;
- tier routing that keeps alternating with full arms → `routing-alternating`, 0, `page`, keeping today's `supply-alert` push;
- host saturation → `capacity-queued` (machine-capacity admission queue at most 3 for 10 minutes) and `build-disk-free` (GiB free on each policy host, its line starting at today's `build_disk_free_gib`), `tell`. It stays apart from `lander-disk-free`: one is GiB on build hosts, the other a share on lander places.

The measuring functions stay (`stall::verdict`, `stranded::rows`, `refused::rows`, `routing::stalls`, `sampler::failing`, `capacity::saturation`). Deleted: the episode and delivery machinery in crates/north-ops/src/ops/flow/alerts.rs (`Record`, `State`, `alert_plan`, `episode_actions`, `deliver`, `stall_alert`), `sampler_plan` and `sampler_alert`, `routing_plan`, `routing_alert` and `alert`, `saturation_plan` and `saturation_alert`, their state files `stalls.json`, `saturation.json`, `sampler.json`, `routing.json` and `stalls.lock`, and `rolling_issue::Breaches` if nothing else uses it.

One announcer replaces them: each sampler tells on a state change by posture, once per episode, keyed on the concern and a digest of its detail lines (a newly stranded lane is a new episode, as it is today), and again every 6 hours while red, keeping the episode in its state file. The repository's Fleet parent stops getting its copy; the proxy routes, which is its role (docs/rationale/proxy-role.md). `--stall-alarm` keeps running `north ci-diagnose`, `north red-main-notify` and `north experiments judge`, which are not threshold alarms. The recurrence tick in src/cli/fleet/waits.rs reuses only the announcer and moves to the posture announcer with the rest.

## Concerns, loops and experiments

`harness/loops.toml` lists every automatic decision north makes as a learning loop, and `docs/rationale/experiments.toml` judges every live policy change on its review date. Concerns repeat neither. **Loops tune decisions, experiments keep or revert changes, concerns judge outcomes.** A loop's `signal` and an experiment's metric are outcomes, and where a concern measures that outcome they read the concern's history instead of a copy. Loop health becomes concerns: `loops-stale-ledger`, `loops-stale-lander` and `loops-stale-here`, each `north loops check --role ROLE` counting that role's stale closed loops, floor 0, sampled on that role's host. The climb order below is itself an automatic decision, so it gets a `[[loop]]` entry (decision: which climb box dispatch feeds next; signal: dividend; updated by the concerns tick; cadence 24 h) and its site joins the loops inventory.

## Climbs: one change, one measurement, keep or revert

Climbs follow the hill-climb protocol adopted in the 2026-10-15 pstack review (south agents/decisions/harness/rebuild-pstack-review-opus.md, finding 8): one change, one measurement, keep or revert, a target and an attempt floor, and the predicate is never relaxed.

**One change, one measurement, through the experiments judge.** `north experiments judge` already keeps or reverts a change on before and after windows against a metric, a direction and a bar, and the policy climber's `climb-` entries already use it. A climb lane names its concern in a `Concern: NAME` line of its brief, and landing it registers one experiments entry whose metric is that concern (a new metric kind, `concern:NAME`, read from the samples): the before window is the concern's `window` ending at the landing, the after window the same span starting one `window` after it (at least 12 hours for a reading of now), so a 24-hour or 7-day average has rolled past the landing before the effect is read. The bar is twice the concern's noise. The judge's existing verdicts then decide: a move past the bar toward the target keeps it, as large the other way reverts it, anything between is kept and counts as no move. No second judge is built.

**Dividend.** Each climb shows its last 5 judged lanes in the 30-day window: lanes that named it in `Concern:`, touched its `pays` paths, or cite an owner issue that owns no other concern (the nine ready bars share north#641, so citing it credits none of them). Its dividend is the summed move, as a share of the climb's span (`worst` to `target`, or the target's magnitude when there is no `worst`), per unit of usage, where usage is the worker ledger's cost of the runs whose item the lane landed (threads `runs` joined to `landings`). Lanes on one concern whose after windows overlap share the move equally; the measurement cannot separate them, and saying so beats inventing a split.

**Attempt floor.** A climb whose last 5 lanes moved it under 1 % of its span in total gets no new climb boxes from dispatch; once 7 days pass it gets one probe box, whose verdict re-measures the dividend. The stop is written into docs/rationale/measured-no-gain.md through a lane, so the no-gain record stays one place.

**Dispatch order.** Dispatch reads one order from `north concerns`: boxes whose issue owns a red floor first (a red floor preempts every climb), then climb boxes by dividend, highest first, skipping stopped climbs; boxes tied to no concern keep their current order behind red floors. Dispatch computes none of it.

## Ratchet, and floors only a dated decision loosens

A floor holds a win only if its line moves with it. Once a day the concerns tick checks each floor that names no `remedy`: when the 99th percentile of its raw samples over the last 7 days beats the bound by at least the margin (the warn band's width when one is set, else 10 % of the bound's magnitude), `north concerns ratchet` writes a lane that moves the bound to that 99th percentile plus half the margin, its commit naming the 7-day median, 99th percentile and noise, and queues the lane for the proxy to land. Red is judged on raw samples, so the new line is set from raw samples too; a median would put it inside the noise. The ratchet never lands a tightening itself, since a tightened floor can turn red on noise the seven days missed. A floor at its best possible value (a count at 0) never qualifies.

A floor with a `remedy` is never ratcheted: its line is also when the doctor restarts a lander or evicts caches, so tightening it would turn quiet margin into restart and eviction storms. Moving such a line is a change to the doctor's loop, made by a lane with an experiments entry like any other policy change.

A floor is never loosened by an agent (the pstack review's finding 8: never relax the predicate). A concerns.toml change that moves a floor's `bound` the worse way, deletes a floor or turns it into a climb is refused at landing unless its commit carries a `Floor-loosened:` trailer citing the dated decision in south agents/decisions that loosens it. The refusal ships in shadow first like every new gate (north#706).

## The close rule, shadow first

Closing an issue labelled `priority:now` or `pinned` must name the concern that holds its result: some concern in that repository's concerns.toml on main that lists the issue among its `owners`, existing or added in the closing lane. The guard sits beside the clean-up close guard (crates/north-guard/src/guard/issue_cleanup.rs), which already parses every `gh issue close`. Closes that bypass it (the web UI, a closing keyword in a landed commit) are caught after the fact by the concerns tick, which compares the GitHub mirror's newly closed issues against the registry. The tier routing that sent most launches to Opus for a day on 2026-11-07, after its work had been called done, is the case this exists for: its close would have needed a concern on launch tiers, and that concern would have gone red within the day.

The inverse holds the line: a concern red for two samples with a closed owner reopens that owner with a comment naming the concern, its value, its line and how it is measured.

Both are new gates that refuse or write, so both land in shadow (north#706): each logs what it would refuse or reopen to north#706's shadow record, and they enforce only after a reviewer reads 24 hours or 20 such decisions.

## Pinned

A `pinned` label exists in every active project's repository. Adding it, from the Pinned view or with `gh issue edit --add-label pinned`, is the whole of what Tom does to follow an issue; removing it stops following. Pinned issues fall under the close rule above.

## Views

`north` opens with four views on keys 1 to 4. Each view's data is one read from north-concerns or from the gates, so crates/north-tui renders and computes nothing:

1. **Decisions**: open gates and red concerns, most severe first: red floors (`tree` before `live`), red climbs, then open gates, the one closing soonest first. It replaces the gate digest pane, and `Ctrl+T` goes.
2. **Concerns**: every concern of every active project with its state, value against its line and warn band, posture, owners, and sparklines of 12 h, 24 h and 7 d.
3. **Pinned**: the `pinned` issues with their live workers (fleet sessions whose item is the issue), unlanded lanes and the concerns each moves (concerns it owns). `/pin REPO#N`, handled by the TUI rather than sent to a session, pins; `u` on a row unpins.
4. **Fleet**: today's screen, with the day's launches by model tier in its header, read from north-fleet's session records.

**Keys.** Today the TUI starts with the composer focused (crates/north-tui/src/tui/fleet/screen.rs), where digits are text, and in the digest digits answer the selected gate directly. Both change: `north` starts with the list focused (any letter still starts a message there), keys 1 to 4 switch views from the list, and in Decisions a gate opens with Enter, inside which 1 to 9 answer its options. One extra key per answer buys four views on the four keys north#709 names.

**One session phase.** The T3 Code review (docs/rationale/watch/2026-11-07-t3code.md, comment on north#709) found every T3 surface reading one phase per thread. North already has the start of that: `Fleet::status` (crates/north-fleet/src/fleet/registry/mod.rs) gives each session one `Status` that `north fleet list`, `north fleet show` and the TUI's ordering all read. It grows into the phase every view reads, and nothing else derives its own:
- `waiting` when the session has an open gate, read from the gate store, so Decisions, Pinned and Fleet agree on who is blocked;
- after a turn ends, the report's first word (`Done`, `Not done`, `Split`, `Blocked`) instead of plain idle, because north scores a run by that word and a session idle after `Blocked` needs an answer while one idle after `Done` needs nobody.

T3's short hold before announcing "done" is not needed: north announces the report word, which a following turn on a background task's notification replaces, not the stream going quiet. Its server-side read state is not adopted: Decisions lists only open gates and red concerns, which leave the list when answered or green, so there is nothing to mark read. Its placement rule is: in Fleet, sessions working stay together in the order they started, and a session moves to the top when its phase becomes `waiting` or its report word is not `Done`.

**Read models and ports.** north-concerns owns the read models (`view::concerns`, `view::decisions`, `view::pinned`) and the ports they read through: `History` (the samples), `Gates` (the gate digest north-fleet already reads), `Issues` (the GitHub mirror), `Workers` (fleet sessions by item) and `Lanes`. The root crate wires the adapters, and the TUI's desk gains `concerns()`, `decisions()`, `pinned()` and `pin()`. north-concerns stays on north-base alone; north-tui gains a dependency on it for the read model types.

`verify-north` drives the four views over its PTY journey: it plants a red concern (a scratch concerns.toml whose oracle prints past its bound) and an open gate, presses 1 to 4, and checks that Decisions lists the gate and Concerns shows the planted concern red.

## The first standing concern: detection rate

`incidents-detector-first` is a climb with `direction = "up"` and `window = "7d"`: the share of the last 7 days' incidents first noticed by a detector rather than by Tom or by no one (7 days to 2026-11-06: detector 3 of 128, Tom 9, no one 116). Its oracle is `north incident report --days 7 --metric detector-share`. Today's report counts every non-human noticer as a detector; the metric splits the `noticed_by` values into detector, Tom (`tom`, `proxy`) and no one (rows recorded after the fact by a sweep or an import), and the lane that adds it reproduces 3 of 128 on that week before it lands. Its target is 50 %, a draft, as targets.md asks for a new target; its owner is north#709 until a detection issue takes it; `worst` is left out until a week of history says where a floor would sit.

## Order of work

History and `--trend` ship first and alone, so Tom sees trends within a day of them landing; every later lane reads the samples they write. Then the rename, the schema, the folds, the close rule, the views, the ratchet and the climbs. The lanes, each 20 minutes or less with its files and Done when, are the boxes of north#709.

Overturned by: a concern state that disagrees with what the proxy or Tom sees for a week (drifting that never turns red, red that nobody acts on), a sampler that cannot keep 10 minutes, or a fold whose concern messages more than the alert it replaced.

## Reviewer round

One fresh Opus reviewer read the first draft against the code on 2026-11-07 and reported twelve findings, all taken:

- The ratchet would have tightened the doctor's lines, which are also its remedy triggers, and set lines from hourly medians while judging raw samples: floors naming a `remedy` are never ratcheted, and the ratchet uses the raw 99th percentile.
- The draft said the after-landing run refuses a concerns.toml that does not parse; it runs after the tree lands. The land check now parses it first, and the doctor keeps its last good copy.
- Running a doctor dry run per concern would repeat every probe a dozen times: the doctor and `north flow` record their own samples (`producer`).
- The judge would have read hysteresis from a 15-minute-old snapshot, two hosts could alternate as one series, and one `max(at)` misses one stopped sampler: state lives in each sampler's state file, a role resolves to one host, and each sampler has its own recorded reading.
- An 18-to-30-hour after window still overlaps a 24-hour or 7-day average: each concern declares its `window` and the after window starts one window after landing.
- The ready fold lacked a met-target answer, dropped makgora's bars, gave nine bars one owner that would credit every lane to all nine, and omitted `revisit`: all four fixed above.
- The draft merged a GiB disk floor into a percentage one: they stay two concerns.
- A whole-number series has zero deviation: the noise has a floor.
- A single `owner` would drop earlier holds: `owners` is a list.
- north#706's change accounting is an expected effect, not measured usage: the dividend joins the worker ledger's runs to landings.
- Climbs would have duplicated `north experiments judge`: each climb lane is an experiments entry on a `concern:` metric.
- Smaller points: `lander-slot-hold` keeps the doctor's key, the page posture's claim was narrowed, loop health is one concern per role, the `PROJECTS` uses and loops.toml's doctor constants are named, and the TUI's composer focus and direct-answer digits are dealt with.

After the round, the session-phase paragraph under Views was added from the T3 Code comment on north#709; it extends `Fleet::status` and adds no new store.

Evidence: north#709; north#706; north#641; docs/rationale/targets.md; docs/rationale/learning-loops.md; docs/rationale/lander-doctor.md; docs/rationale/scoreboard.md; docs/rationale/experiments.toml; docs/rationale/watch/2026-11-07-t3code.md; south agents/decisions/harness/rebuild-pstack-review-opus.md finding 8; Theil-Sen estimator (P. K. Sen, "Estimates of the regression coefficient based on Kendall's tau", Journal of the American Statistical Association, 1968).
