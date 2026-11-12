# Lifecycle deadlines: every box and session in a named state with a deadline and an owner

On 2026-11-12, in one night of unattended work, four things waited unseen because no state they sat in had a deadline or an owner: finished workers' reports that never reached anyone, six finished sessions of the outside provider unreviewed for hours (north#1231), lander units restarting through a promotion reported as eleven alarms (north#1227), and a change reverted seconds after landing while its worker had reported it landed (north#1230). A fifth of the same shape followed that morning: a launch queued because its account was at its weekly limit stayed queued with that reason for eight minutes after the account had reset, while other launches on it started. Each was found by a reader noticing an absence. This note is the plan of north#1235: the states a box and a session pass through, what records each today, its deadline, who is told and what happens by itself when the deadline passes, and the slices that build it.

Owner: north#1235

## What exists, read before proposing

- **The lifecycle kernel** (`crates/north-base/src/lifecycle.rs`, [state-machines.md](state-machines.md)). A machine declares its states, its legal moves, a `dwell` per state and an `orphan` state for a dead holder. Every record carries `due_ms` and `owner`; `Store::faults` finds every overdue state and every dead holder for every machine without knowing its type; `transition_with_effects` commits a move and its side effects in one transaction, keyed so a repeat is dropped. Thirteen machines are on it, among them `fleet-session`, `land-ticket`, `row-delivery`, `after-landing-run`, `red-main`, `claim`, `gate` and `runtime-promotion`. What it lacks: a passed deadline only becomes a finding. The `state-machines` flow of `north health` pages the coordinator after two failing ticks; nothing moves the instance.
- **The fleet's session states.** `fleet-session` records the process phase (live, parked, idle-exited, host-ended, stopped, adopted, retired) in the host ledger; `north fleet list` derives starting, working, idle, parked and exited from it and the stream (`crates/north-fleet-store/src/status.rs`). The work phase (working on the box, reported, report delivered, handed in) has no record.
- **The quiet-against-ETA notice** (`crates/north-fleet/src/fleet/silent.rs`): a child quiet twice its brief's ETA, or parked or exited with no final report or with a `Done:` over an unlanded lane, is reported to its parent once. Its clock is an in-memory `Instant` in the watching process, so a restart of that process resets it.
- **The launch spans** (north#1179, `crates/north-fleet/src/fleet/hops.rs`): `hops.jsonl` in each session's directory holds one span per hop from the launch command to the model's first answer.
- **The landing spans** (north#1214, `crates/north-land-checks/src/land/trace/landing.rs`): a landing's trace rooted at `landing:TICKET`, with hand-in, gates, verdict and landed times.
- **The page-flood backstop and the folding of alike messages** ([page-flood.md](page-flood.md)): at the fleet store's send path (`crates/north-fleet-supervisor/src/fleet/registry/messages.rs`, `send_condition`), a service's condition key gives one message per episode (`fold.rs`), alike rows that differ only in a unit, repository or path become one follow-up with the count and every name (`alike.rs`, north#1227), and past three messages in ten minutes one sender is folded into one line (`flood.rs`). Which service alarms wake the coordinator is a string match per service (`page.rs`). A worker's report reaches the coordinator only if it opens `Blocked:` or a `Not done:` with news (`crates/north-fleet/src/fleet/upward.rs`, `wakes_proxy`); a `Done:` report goes to `held.jsonl`, which the coordinator must fetch with `north fleet held` (`receipts.rs`, `deliver_report`).
- **The promise reminders** (`crates/north-fleet/src/fleet/promises.rs`): a report that names a future time and a check is reminded to its parent when the time comes.
- **The dispatch review job** (north#1020, north#1231, `crates/north-dispatch/src/dispatch/deepseek/review.rs`): for a lane-only session resting in a host's listing whose last report opens `Done:`, dispatch queues one review job, which lands the lane or starts a reviewer; a 30-minute requeue window and `deepseek-reviews.jsonl` make it once.
- **The launch queue** (`crates/north-fleet-supervisor/src/fleet/launch_queue.rs`, drained by `src/cli/fleet/tick.rs`): a refused launch is a file under `.launch-queue/` holding its request and the refusal text; the fleet tick retries each 3 min after its last try, inside the tick's time budget, and defers the whole queue to the next tick when the budget runs out. The refusal text names a reason but not when it lapses, and a queued launch is no session, so stopping it by name finds nothing.
- **The revert notice** (`crates/north-landing/src/land/after/owner.rs`, `tell`): after a red after-landing run the sheriff reverts and tells the lane's session if it is live, else restaffs its box or tells the coordinator. The ticket stays `landed`, an end state, and the parent that was told "landed" is never told otherwise.
- **The digest** (`crates/north-fleet/src/fleet/digest_report.rs`): `north fleet digest` prints what landed, what is in flight, open gates, stalls over 2 h and the held messages, when asked. Nothing sends it.

## The design

Two additions, both at choke points that already exist, and nothing per component.

1. **A deadline acts.** The lifecycle kernel gains `expire(state) -> Option<(to, effect)>` beside `dwell`, and one sweep, `Store::expire(at)`, run from the fleet tick, moves every instance past its `due_ms` to the state its machine names and queues its effect in the same transaction. A machine with no `expire` keeps today's behaviour: a finding in `north health`. One sweep serves every present and future machine, as the send path's ceiling serves every sender.
2. **The box's work has one record.** A new machine `box-work`, one instance per box and try (`REPO#N/BOX@SESSION`), in the ledger host's `threads.db` through the same store, records the work phase that `fleet-session` does not: its owner is the session working the box, and each state's dwell is its deadline. The process phase stays in `fleet-session`; the two are joined by the session name, so a session that parks mid-box is parked in one machine and working in the other.

The writers are the places that already decide each move (claim, launch registration, the final turn, the receipt, enqueue, the lander's verdict, the sheriff's revert, the runtime's promotion); each adds one `transition::<BoxWork>` call, through the threads spool when the writer is on another host.

## States

Each entry: what records it today; its deadline; who is told; what happens by itself when the deadline passes.

### On the path, ready to promoted

- **ready**: the box is open on its issue and unclaimed. Today: the issue body's checkbox, read by dispatch's survey. Deadline: none; the backlog is ranked, not timed. Told: nobody. Expiry: none.
- **claimed**: a session holds the box. Today: the `claim` machine in `threads.db`. Deadline: 10 min to a registered launch or a queued one. Told: the claimant's parent. Expiry: the claim is released and the box is ready again, with the cause logged.
- **queued-to-launch**: the launch was refused for a reason that lapses (an account at its limit, a host at its cap, a spend bound) and waits. Today: a file in the launch queue with the refusal text and its last try. Deadline: the moment its stated reason lapses (the account's reset time from the usage reading, the cap's next free slot), plus 1 min; a reason with no lapse time takes 15 min. Told: the parent, only when a retry is refused for a different reason or after three refusals. Expiry: the sweep retries the launch at once, outside the tick's queue budget, and records the new reason and its lapse time when it is refused again. The instance carries the session name it will have, so `north fleet stop NAME` withdraws it.
- **launching**: the session is registered and has not answered. Today: `fleet-session` live, the launch spans in `hops.jsonl`. Deadline: 2 min to the first answer (the hops take seconds). Told: the parent. Expiry: the try moves to failed-launch and the parent is told once; the box is ready again.
- **working**: the session works the box. Today: `fleet-session` live and the stream; the quiet notice's in-memory clock. Deadline: twice the brief's ETA without output (45 min when none), the quiet notice's rule, now a recorded `due_ms` that each turn pushes forward. Told: the parent. Expiry: the parent is told once per quiet stretch, as today, and the clock survives a restart.
- **reported**: the final turn is written (`Done:`, `Not done:`, `Split:` or `Blocked:`). Today: the stream; `receipts` claims a key when it delivers. Deadline: 2 min to delivered. Told: the parent. Expiry: report-undelivered.
- **delivered**: the report is in the parent's session. Today: the receipt and the `row-delivery` record of its row. Deadline: for a `Done:` with a lane holding commits, 10 min to queued; for a lane-only session, finished-unreviewed at once. Told: the parent. Expiry: the quiet notice's "Done over an unlanded lane" line, from the record.
- **queued**: a land ticket holds the lane. Today: `land-ticket` queued or checking, the landing spans. Deadline: 15 min to landed (the checking state's own dwell is 2 h, which stays the lander's bound). Told: the worker, and the coordinator only past the deadline. Expiry: one message to the coordinator naming the ticket and its step from the landing spans, folded with its like.
- **landed**: the change is on main. Today: `land-ticket` landed, `after-landing-run`. Deadline: the after-landing run's verdict within 30 min (the sheriff's probe bound). Told: the worker and its parent. Expiry: the run is a `red-main` or `after-landing-run` fault already; the box waits for it.
- **promoted**: a live runtime holds the commit. Today: `runtime-promotion` live and its swap log; the `landing` flow of `north health` measures the oldest main commit no runtime holds. Deadline: none; it is the end. Told: nobody beyond the landing notice.

### Off the path

- **parked** (session, `fleet-session` parked or idle-exited): today the record and any registered wait. Deadline: a parked session's box stays in working, so its deadline is working's; a park on a gate takes the gate's default time. Told: the parent, by working's expiry. Expiry: as working's; a park is never its own alarm.
- **finished-unreviewed**: a lane-only session reported `Done:` with a commit, and no ticket or reviewer holds it. Today: nothing but the review job's queue file and a host listing. Deadline: 10 min to a review job queued, then 1 h to a ticket or a reviewer's verdict. Told: nobody at the first deadline; the coordinator once at the second, folded with its like. Expiry: the sweep queues the review job itself, keyed by session and commit, wherever the session's host is listed.
- **report-undelivered**: reported past its 2 min. Today: the report sits in the stream, in a failed `row-delivery` row, or in `held.jsonl`. Deadline: one sweep. Told: the parent. Expiry: the report is delivered again as one message; past three failed deliveries the coordinator is told once with the session and the report's first line.
- **reverted**: landed, then reverted by the sheriff. Today: the revert commit and the sheriff's message to a live session; the ticket stays landed. Deadline: 1 min to the worker and its parent told; 30 min to a restaffed try or back to ready. Told: the worker and its parent, as "reverted SHA: cause", replacing the "landed" they were told. Expiry: the box is ready again with the revert's cause in its next brief, as the sheriff's restaff does today.
- **restarting** (a lander unit, not a box): the unit exits when the live runtime changes and systemd restarts it. Today: systemd's restart count, read by `crates/north-lander/src/land/unit_start.rs`. Deadline: the promotion's own record settling, plus 2 min. Told: nobody inside the deadline; the coordinator once after it, one message for every unit still restarting. Expiry: the alarm. North#1227's second box owns this cause; the plan gives it the state and the deadline and builds nothing beside that box.

## The four incidents of 2026-11-12, and a fifth

- **Reports never delivered.** State reported, deadline 2 min. A `Done:` to the coordinator went to the held log by rule, and a report whose delivery failed waited on a row retried for an hour; the sweep would have delivered each within one tick of its 2 min.
- **Six finished outside-provider sessions unreviewed.** State finished-unreviewed, deadline 10 min. The review job found its work through one naming of the session's host and missed the other (north#1231). Keyed by box and session, the record does not depend on which listing names the host; the sweep would have queued each review 10 min after its `Done:`, and told the coordinator once an hour later had none landed.
- **Lander units restarting through a promotion.** State restarting, deadline the promotion's settle plus 2 min. The units were active again seconds after the runtime was in place, so nothing would have been sent; a unit still restarting after the deadline would have been one message for all.
- **A change reverted seconds after it landed.** State reverted, deadline 1 min. The worker's parent was told "landed" and nothing after; the move to reverted tells the worker and the parent in the same transaction as the move. Why the change reached main is north#1230's.
- **A queued launch waiting past its reason** (the fifth, the next morning). State queued-to-launch, deadline the account's reset plus 1 min. The tick retried on a 3 min clock blind to the reset and could defer the queue whole; the expiry would have retried within a minute of the reset, and the stop by name would have found the instance.

## Sorting the coordinator's messages

Every message to the coordinator passes `send_condition`; that is where it is sorted, by one function `sort(row) -> ActNow | Digest | Duplicate`, which takes over the decisions now spread across `page.rs`, `fold.rs`'s hold, `alike.rs`, `flood.rs` and `upward.rs`'s `wakes_proxy`.

- **Pinned before anything reads the text.** Decided by the row's kind and sender, never by a classifier: a message from Tom, a red main, a gate opened or answered, a failed order (a launch, send or hand-in the coordinator asked for that failed), and a worker's `Done:`, `Not done:` or `Blocked:` report are act now, always and whole. A worker's `Done:` report to the coordinator stops going to the held log.
- **Fast and deterministic first.** The rules that exist already sort most rows in microseconds: the condition key's episode (a repeat is a duplicate), the alike template (one follow-up with counts), the per-sender ceiling, a cleared or recovered notice (digest), a service alarm outside the page-now classes (digest). The ones left over are counted by sender and template in the alarm statistics.
- **A model tier only on a measured residue.** If after a week the rows no rule sorts are more than a fifth of the coordinator's turns, a small model sorts that residue only, with a time limit of 1 s and act now when it errs or times out. Not before: it would send message text to a provider and add a second's wait to rows that rules sort today.
- **Sorting is all it does.** No classifier decides anything about landing, privacy, credentials or gates; a sorted row changes only when it reaches the coordinator.
- **The digest is pushed and drops nothing.** Every 15 min, when it has rows, the coordinator gets one message: each digest row once, each duplicate group as its text once with its count and its first and last time. `held.jsonl` stays the log the digest is read from, and nothing has to be fetched.

## What it removes

- The launch queue's blind 3 min retry clock (`QUEUE_RETRY_MS`) and its `tried_ms` mark, replaced by queued-to-launch's deadline at the reason's lapse.
- The coordinator's fetch of held messages: `north fleet held` as a step the coordinator must remember, replaced by the pushed digest.
- `page.rs`'s per-service string match and `upward.rs`'s `wakes_proxy`, folded into `sort` with the pinned rules.
- The quiet notice's in-memory clock in `silent.rs`, replaced by working's recorded `due_ms`; the notice's text stays.
- The review job's own requeue window and queue file (`REQUEUE_S`, `deepseek-reviews.jsonl`), replaced by finished-unreviewed's expiry effect and its idempotency key.
- The digest's 2 h stall line (`STALL_AFTER_MS`), replaced by the box records' deadlines.
- The sheriff's direct sends in `owner.rs`, which become the reverted move's effects.

It keeps the promise reminders (a session's own stated time, not a state), the `state-machines` health flow as the backstop for a machine with no expiry, the launch and landing spans as the source of each state's times, and the flood ceiling under `sort` as the last bound.

## Slices, in order of what they remove

Each lands and is live on the runtime in under two hours.

1. **One box's path from records that exist.** `north box path REPO#N [--box K]` prints each state from ready to promoted with its start and the time spent in it, read from the claim, `fleet-session`, launch span, receipt, land ticket, landing span, after-landing run and promotion records. Done when: for a box landed today it prints every state from claimed to promoted, and the times spent sum to the wall time from claim to promotion within 1 s. Size: 90 min.
2. **A deadline acts.** `Lifecycle::expire` and `Store::expire`, run by the fleet tick; `north check machines` checks the expiry moves with the rest. Done when: an existing lifecycle store test gains a record past its dwell that the sweep moves once and whose effect is queued once over two sweeps. Size: 60 min.
3. **The `box-work` record, worker side.** The machine, its chart and registry entry, and its writers at claim, launch, each turn and the final turn; slice 1's command reads it where present. Done when: `north check machines --root .` passes with the chart, and `north box path` on a box worked after the slice reads its states from the record. Size: 90 min.
4. **Reports reach their reader.** Reported's 2 min deadline re-delivers, and a worker's `Done:` to the coordinator is delivered, not held. Done when: an existing delivery test shows a `Done:` report to the coordinator delivered as a message and absent from the held log. Size: 45 min.
5. **A queued launch retries when its reason lapses.** The launch queue records each refusal's lapse time, queued-to-launch's expiry retries it, and `north fleet stop NAME` withdraws a queued launch. Done when: an existing launch-queue test queues a launch refused for an account limit with a reset one minute ahead, and one sweep after the reset starts it while the 3 min clock would not yet have. Size: 60 min.
6. **The `box-work` record, landing side.** Writers at enqueue, landed, reverted and promoted; reverted tells the worker and its parent as its effect. Done when: an existing lander revert test shows the parent receiving one "reverted SHA" message after its "landed" one. Size: 90 min.
7. **Finished work is reviewed.** Finished-unreviewed's expiry queues the review job; the review job's requeue window and queue file go. Done when: a dry run of the sweep on the live runtime names every lane-only session whose `Done:` is older than 10 min with no ticket, whichever host listing names it. Size: 60 min, after north#1231's first box.
8. **Coordinator turns per landed change.** `north fleet turns --per-landing --days 7` prints, per day, the coordinator's turns, the changes landed and their ratio, from the coordinator's runs and the land tickets. It lands before the sorting so the sorting is measured against a baseline. Done when: the command prints seven days with each day's turns matching the coordinator's runs count that day. Size: 60 min.
9. **One sort at the send path.** `sort` with the pinned rules, absorbing `page.rs` and `wakes_proxy`. Done when: an existing send-path test sends one row of each pinned kind among forty alike and repeated service rows and the coordinator receives each pinned row whole and at once. Size: 90 min.
10. **The pushed digest.** Every 15 min, with counts and first and last times. Done when: an existing digest test shows twelve held rows of three causes arriving as one message with three lines whose counts sum to twelve, and the held log still holding all twelve. Size: 45 min.

### What can be built at the same time

Read from the files each slice changes:

- **Now, together: 1, 2, 8 and 9.** Slice 1 adds a read-only verb over existing records, 2 changes the lifecycle kernel (`crates/north-base/src/lifecycle.rs`, its store) and the fleet tick, 8 adds a read-only verb over runs and tickets, and 9 changes the send path (`crates/north-fleet-store/src/{page,fold,alike,flood}.rs`, `crates/north-fleet-supervisor/src/fleet/registry/messages.rs`, `crates/north-fleet/src/fleet/upward.rs`). Slices 1 and 8 each add a numbered verb page under `src/cli/verbs/`, so the second to land takes the next number.
- **After 2: 3.** It implements the kernel's new `expire` and registers its machine in `src/cli/machines.rs`, which 2 also changes.
- **After 3 and 9: 4.** Reported is a `box-work` state, and holding or delivering a `Done:` to the coordinator is decided in `upward.rs` and `receipts.rs`, which 9 rewrites.
- **After 3: 5, 6 and 7, together.** Queued-to-launch, the landing-side states and finished-unreviewed are `box-work` states. Slice 5 changes the launch queue and the tick's drain, 6 the lander's enqueue and verdict and the sheriff's `owner.rs`, 7 the review job (`crates/north-dispatch/src/dispatch/deepseek/review.rs`); 7 also waits for north#1231's first box, which reads the same job.
- **After 9: 10.** The digest is the sort's digest class.

## What it would not build

- A new store, a dashboard or a screen: the records live in the ledgers through the store that exists, and `north box path` and the digest are text.
- A timer per component: every deadline is a dwell, every action an expiry effect, run by one sweep.
- A model in the path of a pinned message, or a model tier before the residue is measured.
- Any change to what a gate, a check or the sandbox allows, or to what the lander decides.
- A second machine for the process phase: `fleet-session` keeps it.
- A box record for work that has no box (an interactive session, a reviewer): those stay `fleet-session` alone.
- A backfill of box records before the slice that writes them; slice 1 reads the existing records for older boxes.

## Considered and not chosen

Fixing each incident where it occurred (the review job's host naming, the held `Done:`, the revert's parent, the restart alarm) is being done under north#1227, north#1230 and north#1231 and leaves the next state of the same shape without a deadline. Adding a deadline inside each component would repeat what the page-flood note found for once-marks: several mechanisms for one guarantee drift apart, and the weakest decides.
