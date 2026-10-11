---
name: workers
description: Assign independent work, choose provider tiers and ETAs, and run workers from Codex or Claude.
grounded: 2026-10-10
written: 2026-10-10
---
# Workers

**Throughput is lost to serialized long waits (farm runs, soaks, renders, landings), not to thinking.** Each tick, cut the longest wait on the critical path: batch ready lanes into one landing, run independent unknown-cause hypotheses in parallel (a `Done: ruled out` with evidence closes one), render 2–4 variants and judge them once, and send code-only work to cloud workers first.

## Staff

- `agents org show` prints the chain of command. Tom's top-level session is his proxy; a SessionStart hook gives it the role. Each domain has a resident lead session, started with `agents lead start` from [the lead brief template](references/lead-brief.md), which covers layering, escalation and finishing. Begin every brief you write with `Delegation: role=<worker|sub-lead> budget=<n>`, where n is below your own budget; the spawn gate refuses at budget 0.
- Choose the provider and fast mode with `agents plan`, which reads the signed-in providers and applies the live headroom `agents usage` reports; never choose them by hand. `worker-ledger --recommend '<Category and facet lines>'` picks the tier, and `worker-ledger --summary` shows each category's success.
- Whoever writes a box names, for each Done when line, the existing property, replay or golden test that covers it; a new example test needs a one-line reason.
- Check `threads list` and open issues first, and never duplicate a held item. Pick ready work with `threads ready` and read its Done when with `threads show repo#N`. Claim shared-resource work through `threads`; a running worker's brief is its claim.
- Brief: Item, Category, goal, files, Done when, ETA (one box, 20 minutes or less; research defaults to 5 minutes and ends by offering a deeper pass with its own ETA), facet lines `Spec: exact|measured|judged`, `Scope: one-file|module|cross-module|cross-repo`, `Surface: ts|lua|nix|shell|workflow|docs|assets`, `Verify: none|local-test|farm|native|visual`, and the state checked at spawn (main SHA and CI, lane SHAs, what landed).
- Category: mechanical, docs-policy, tooling, feature, bug-known-cause, debugging-unknown-cause, netcode-determinism, performance, balance-tuning, native-check or research.
- Start each retry, escalation or continuation as a fresh worker whose brief carries `Follows: <earlier agent id and tier>` and the failed evidence. Never revive a finished worker by message.
- Workers on native items (life, accounts, travel, ops) report with `threads note REF TEXT`, never issue comments.
- Staff from the tiers below. Set model and effort explicitly on every Codex spawn. Never use Codex low or max, Sonnet or Opus low, and use Fable only when Tom names it.

| Provider | Tier | Model / invocation |
| --- | --- | --- |
| Codex | medium | `spawn_agent`, model `gpt-6.1-sol`, reasoning_effort `medium` |
| Codex | high | `spawn_agent`, model `gpt-6.1-sol`, reasoning_effort `high` |
| Codex | xhigh | `spawn_agent`, model `gpt-6.1-sol`, reasoning_effort `xhigh` |
| Claude | haiku | `worker-haiku`, Haiku 5.5 high |
| Claude | medium | `worker`, Opus 5.5 medium |
| Claude | high | `worker-high`, Opus 5.5 high, escalation only |
| Claude | xhigh | `worker-xhigh`, Opus 5.5 xhigh, escalation only |

- Start every Opus-range item at `worker`. Escalate one tier only after `Stop: reasoning`, and restaff every other stop at the same tier. When the next tier cannot run here, bring the supervisor one recommendation.
- Send mechanical work and lane mechanics (rebase, regenerate, conflicts, box ticks) to `worker-haiku`. Judged art, bisects, native checks and any job that waits more than 2 minutes on a run go to `worker` or stay with the lead. Send other tooling and bug-known-cause items to `worker-haiku` with `Arm: haiku-trial` until the finest facet cell with 5 runs has 5 Haiku closures or 2 Haiku failures ([notes](references/notes.md)).

## Review

- Staff `planner` (Opus xhigh; `effort: max` only when Tom asks by name) for a plan, not code, when an issue gets its second Not done, a category's last 10 runs fall below 40% closed, or the change is architectural.
- Before staffing a planner's architectural design, have one reviewer from another model family try to break it, plus Gemini Pro as an extra reviewer (`gemini-review`, public diffs only). Revise once.
- Codex implements most work to conserve Claude usage; Claude workers take only critical-path or quality-critical pieces, and Claude leads oversee, review and land.
- Keep Codex on a tight leash: brief each Codex worker verbosely with the goal and why, explicit anti-goals (no unrequested formal machinery, scope, tests or abstractions), exact files, the behavior each box may change, Done when and ETA; forbid weakening existing assertions; review every diff for behavior drift as well as path drift before it lands and cut drift.
- Append a `LESSON <date> <lead>: <instruction or hook> -> <measured effect> (<evidence ids>)` line to `~/.local/state/agents/handoffs/codex-lessons.md` whenever a Codex brief wording, skill or hook measurably changes first-try closure, drift or speed, failures included.
- Claude Opus owns delivery quality for Codex-implemented work: before landing, a Claude review checks it against Tom's intent and cuts formal machinery the spec does not ask for.
- Give each adversarial reviewer one lens that no other reviewer has. Each reviewer reports `Accepted flaws: N`.
- Weigh each finding by its evidence first and the reviewer's tier second. A failing scenario, test or measurement counts whichever model raised it. Investigate an unevidenced finding from a larger model; have a stronger model check a smaller model's unevidenced finding once before anyone acts on it.
- Price rewrite and maintenance effort at AI cost: agents do it 10–100x more cheaply than a human. Rank review findings and alternatives by leverage, not by human effort.
- Before landing a `Spec: judged` cross-module or cross-repo lane, have a reviewer from another model family or tier check the diff for wrong behavior and weakened tests. Fix what it finds before `safe-push`.

## Run and land

- Keep `worker-sweep --wait` running while workers run, and act on each flagged row and `WAIT` line by its named move. STALLED: ask for a report, then archive the worker. OVERTIME or HANDOFF: nudge once, then staff a `Follows:` replacement. PARKED: move the wait to the parent. Never judge a worker by recent activity alone.
- Expect a report at 45 minutes or twice the ETA, whichever comes first. Recycle a worker at 45 minutes or 350k context: it queues what passes and writes a complete handoff at a checkpoint, and a fresh worker continues from that handoff.
- When a worker has been idle 10 minutes without reporting, ask it for a report, then archive it. Never park a worker to wait on a farm run, a client or another worker.
- Send every code-only Smashcraft or Wisp item to `cloud-workers` first. Staff local workers only for real-game clients, private game assets, the LAN pool or unpushed local state.
- In an Autoland repository, a worker's landing ends when `safe-push --to main` has pushed its `claude/land-*` branch, and it reports `Done: queued <branch>`. Elsewhere, run `safe-push` in the foreground with a 10-minute timeout.
- Before starting an issue, check `lane-gc --unlanded` for an existing branch, and continue that branch if one exists.
- Stop your own monitors and background shells when their work ends. Close each Codex worker when its final report arrives.
- Promote a process idea to policy only after a measured result. After an hour with no closure, start nothing new until an open box closes.

## Reports and ticks

- Require every report to begin with `Done:`, `Not done:` or `Blocked:`. Every Not done or handoff names `Stop: landing|capacity|client|leash|reasoning|decision`.
- Start every `tick` with `threads unowned`. Give each priority:now UNOWNED row a worker (a cloud worker first when the item is code-only), or record why it cannot be staffed with `threads block <repo#N> <reason>`.
- Schedule `tick` only from `south:agents/routines/`: run CronList, then CronCreate with `agents routines pointer tick` only if no session cron carries it.
- Write the status file in plain sentences, one line per item, and end each line with `priority:now unowned=N blocked=M owned=K`.
- Read `agents --help`, `threads --help` and `worker-ledger --help` for command detail, and [notes](references/notes.md) for ledger rules, compute routing, Autoland, recycling and ticks.
