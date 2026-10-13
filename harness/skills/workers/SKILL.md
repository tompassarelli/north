---
name: workers
description: Assign independent work, choose provider tiers and ETAs, and run workers from Codex or Claude.
grounded: 2026-10-10
written: 2026-10-10
---
# Workers

**Throughput is lost to serialized long waits (farm runs, soaks, renders, landings), not to thinking.** Each tick, cut the longest wait on the critical path: batch ready lanes into one landing, run independent unknown-cause hypotheses in parallel (a `Done: ruled out` with evidence closes one), render 2–4 variants and judge them once, and send code-only work to cloud workers first. While a repository's landing queue grows (`agents flow`), widen its landing path before adding workers.

## Staff

- `agents org show` prints the chain of command. Tom's top-level session is his proxy; a SessionStart hook gives it the role. Each domain has a resident lead session, started with `agents lead start` from [the lead brief template](references/lead-brief.md), which covers layering, escalation and finishing. Begin every brief you write with `Delegation: role=<worker|sub-lead> budget=<n>`, where n is below your own budget. The budget limits how many levels of delegation may follow, not how many workers run; the spawn gate refuses a spawn at budget 0, and capacity and ready work bound the count.
- Choose the provider and fast mode with `agents plan`, which reads the signed-in providers and applies the live headroom `agents usage` reports; never choose them by hand. `worker-ledger --recommend '<Category and facet lines>'` picks the tier, and `worker-ledger --summary` shows each category's success.
- Whoever writes a box names, for each Done when line, the existing property, replay or golden test that covers it; a new example test needs a one-line reason.
- When staffing, queue each box that waits on another lane: pipe its brief, carrying `Follows: <commit subject, SHA or Item>`, to `threads queue follow <repo#N> --tier <tier> --lead <your org name>`; the predecessor's landing starts it through the spawn gate, and a box that can't start alerts you and the proxy.
- Check `threads list` and open issues first, and never duplicate a held item. Pick ready work with `threads ready` and read its Done when with `threads show repo#N`. Claim shared-resource work through `threads`; a running worker's brief is its claim.
- Brief: Item, Category, goal, files, Done when, ETA, facet lines `Spec: exact|measured|judged`, `Scope: one-file|module|cross-module|cross-repo`, `Surface: ts|lua|nix|shell|workflow|docs|assets`, `Verify: none|local-test|farm|native|visual`, and the state checked at spawn (main SHA and CI, lane SHAs, what landed). Size each brief to one box of 20 minutes or less as guidance, and name the reason in any brief whose ETA exceeds 20 minutes; take its ETA from the `eta` line of `worker-ledger --recommend`; when that prints `eta n/a`, write your estimate followed by `(uncalibrated)`. Assign one task per worker and one worker per scarce client or device instance. Research Tom asks for inline is timeboxed at 5 minutes and ends by offering a deeper pass with its own ETA rather than starting one; a research worker takes the `worker-ledger --recommend` ETA like any other brief.
- Category: mechanical, docs-policy, tooling, feature, bug-known-cause, debugging-unknown-cause, netcode-determinism, performance, balance-tuning, native-check or research.
- Call Claude `Agent` workers with `run_in_background: true`, including retries and escalations with `Follows:`, so the launch result records the original brief with the actual child identity.
- Start each retry, escalation or continuation as a fresh worker whose brief carries `Follows: <earlier agent id and tier>` and the failed evidence. Send a running worker only its own task's follow-up. Never revive a finished worker by message.
- Workers on native items (life, accounts, travel, ops) report with `threads note REF TEXT`, never issue comments.
- Staff from the tiers below. Set model and effort explicitly on every Codex spawn. Never use Codex low (except the luna-low tier) or max, Sonnet other than the sonnet-high tier, Opus low, and use Fable only when Tom names it.

| Provider | Tier | Model / invocation |
| --- | --- | --- |
| Codex | luna-low | `spawn_agent`, model `gpt-6-luna`, reasoning_effort `low`; mechanical work only (exact edits, lookups, triage, named checks), after Haiku. The unlabeled `gpt-6-luna medium` runs of 1-5 Oct are not outcome evidence |
| Codex | medium, high | `spawn_agent`, model `gpt-6.1-sol`, reasoning_effort `medium` or `high` |
| Claude | haiku | `worker-haiku`, Haiku 5.5 high |
| Claude | sonnet-high | `worker-sonnet`, Sonnet 5.5 high; Claude-side middle band (features, tooling, known-cause fixes), before `worker` in the escalation and the middle band in claude-only mode; Sol keeps it when Codex is up |
| Claude | medium | `worker`, Opus 5.5 medium |
| Claude | high, xhigh | `worker-high` or `worker-xhigh`, Opus 5.5 high or xhigh, escalation only |

- Start every Opus-range item at `worker`. Restaff every stop at the same tier with a `Follows:` brief carrying the failed evidence, and escalate one tier only after a second `Stop: reasoning` on the same box. Until each arm has 15 runs, send those second reasoning stops in turn to `Arm: escalate` (one tier up), `Arm: cheap-fix` (the same tier with a better brief) and `Arm: split` (the box broken into parts), so the ledger can compare them. When the next tier cannot run here, bring the supervisor one recommendation.
- Send only mechanical edits (renames, link fixes, box ticks) and lane mechanics (rebase, regenerate, conflicts) to `worker-haiku`. Policy, skill and documentation prose (`Category: docs-policy`) goes to `worker` and is written carefully, never by Haiku. Judged art, bisects, native checks and any job that waits more than 2 minutes on a run go to `worker` or stay with the lead. Alternate tooling, balance-tuning and bug-known-cause items that name their files and have a measured Done when between `worker` and `worker-haiku` with `Arm: haiku-trial`, until the finest facet cell with 5 runs has 5 Haiku closures or 2 Haiku failures ([notes](references/notes.md)).

## Review

- Covered harness changes land only with a `Reviewed:` trailer from a fresh reviewer who did not author the change: Astra high/xhigh, Sol high/xhigh or Opus. This is the interim review rule until north#52e mechanically enforces review of the combined patch-id. Covered paths are reported by `safe-push --durable-harness-paths <repository>`.
- Staff `planner` at Opus high by default; Tom's hypothesis (12 Oct) is that high fits about 90% of plans, xhigh the 90th to 98th percentile of complexity and `max` the top 2%. Pass `effort: xhigh` or `max` only with evidence from our own data (a high plan for the same issue already came back Not done, or `worker-ledger` shows high plans in that category closing worse than xhigh ones) or when Tom asks by name, and name that evidence in the brief; place each plan in that mix by ranking its complexity against our past planned work in `worker-ledger` (scope, surface, summed box ETA and prior Not dones), not by feel, and keep planner runs near the 90/8/2 mix until the ledger disproves it. Staff it for a plan, not code, when an issue gets its second Not done, a category's last 10 runs fall below 40% closed, or the change is architectural.
- Before staffing a planner's architectural design, have one reviewer from another model family try to break it, plus Gemini Pro as an extra reviewer (`gemini-review`, public diffs only). Revise once.
- Codex implements most work to conserve Claude usage; Claude workers take only critical-path or quality-critical pieces, and Claude leads oversee, review and land.
- Keep Codex on a tight leash: brief each Codex worker verbosely with the goal and why, explicit anti-goals (no unrequested formal machinery, scope, tests or abstractions), exact files, the behavior each box may change, Done when and ETA; forbid weakening, normalizing or skipping an existing assertion, because a failing one means the code it guards is wrong until its owner rules otherwise; review every diff for behavior drift as well as path drift before it lands and cut drift.
- Append a `LESSON <date> <lead>: <instruction or hook> -> <measured effect> (<evidence ids>)` line to `~/.local/state/agents/handoffs/codex-lessons.md` whenever a Codex brief wording, skill or hook measurably changes first-try closure, drift or speed, failures included. If a pending line, or a line marked `expired` in `codex-lessons-folded.md`, already records the same lesson, add `; also <date> <lead> (<evidence ids>)` to it instead of writing a new one, moving an expired line back into `codex-lessons.md`. A defect in a tool or script is an issue for its owning lead, not a lesson; so is a privacy, safety or data-loss slip, filed the same day as a box for a mechanical check.
- Before landing, an independent review checks the work against Tom's intent and cuts formal machinery the spec does not ask for.
- Give each adversarial reviewer one lens that no other reviewer has. Each reviewer reports `Accepted flaws: N`.
- Weigh each finding by its evidence first and the reviewer's tier second. A failing scenario, test or measurement counts whichever model raised it. Investigate an unevidenced finding from a larger model; have a stronger model check a smaller model's unevidenced finding once before anyone acts on it.
- Price rewrite and maintenance effort at AI cost: agents do it 10–100x more cheaply than a human. Rank review findings and alternatives by leverage, not by human effort.
- Before landing a `Spec: judged` cross-module or cross-repo lane, have a reviewer from another model family or tier check the diff for wrong behavior and weakened tests. Fix what it finds before `safe-push`.

## Run and land

- Keep `worker-sweep --wait` running while workers run, and act on each flagged row and `WAIT` line by its named move. STALLED: ask for a report, then archive the worker. OVERTIME or HANDOFF: nudge once, then staff a `Follows:` replacement. PARKED: move the wait to the parent. Never judge a worker by recent activity alone.
- Expect a report at 45 minutes or twice the ETA, whichever comes first. Recycle Codex leads and sub-leads at 150k context. Across providers, workers recycle at 350k context or 45 minutes. At a checkpoint, each role queues what passes and writes a complete handoff for a fresh session.
- When a worker has been idle 10 minutes without reporting, ask it for a report, then archive it. Never park a worker to wait on a farm run, a client or another worker.
- Send every code-only Smashcraft or Wisp item to `cloud-workers` first, and move a code-only local worker to the cloud when you find one. Staff local workers only for real-game clients, private game assets, the LAN pool or unpushed local state.
- Place each written box with `agents plan --place BRIEF` and start its worker on the printed target: a host (gex45, nexus) below its resident cap, `anthropic-cloud` per `cloud-workers`, `laptop` as a `threads queue add laptop` job, `codex-cloud` once C1 passes, or `held: <reason>` until that clears. Mark a box that is not code-only with `Needs: native|sign-ins|private|local-state`; native and sign-in boxes go to the laptop only.
- In an Autoland repository, a worker's landing ends when `safe-push --to main` has pushed its `claude/land-*` branch, and it reports `Done: queued <branch>`. Elsewhere, a lead runs `safe-push` in the foreground with a 10-minute timeout; a worker runs it in the background, because worker-wait-guard refuses a foreground landing, and reports once it exits.
- Before starting an issue, check `lane-gc --unlanded` for an existing branch, and continue that branch if one exists. An owner who abandons a lane deletes it in the same turn.
- A lane ends landed, continued under an open issue, or archived; never left. The hourly `lane-gc` archives landed-elsewhere, superseded and stale-dirty lanes to `archive/<container>/<lane>/<tip>` tags before retiring them; every other lane is owned by its project's lead and shows in that lead's tick as an `OWNED` row with age and issue, oldest first. Decide each one: continue, land or archive. A lead holding more than 25, or any older than 72 h, gets a supply-alert.
- Stop your own monitors and background shells when their work ends. Close each Codex worker when its final report arrives.
- File an unproven process idea as an issue, and promote it to policy only after a measured result. Finish an issue's last unchecked box before starting new work. After an hour with no closure, start nothing new until an open box closes.

## Reports and ticks

- Require every report to begin with `Done:`, `Not done:`, `Blocked:` or `Split:`. Every Not done or handoff names `Stop: landing|capacity|client|leash|reasoning|decision`.
- A worker that finds independent parts, or projects past twice its ETA, stops early and reports `Split:`: the parts as boxes, each with a Done when, and what it learned. It then leads them: resume it, or restart it with a `Follows:` brief, as `role=sub-lead budget=1`, and it files the parts as boxes or sub-issues, staffs them and lands them. Approve a split only when it reaches into another domain or would push the repository past one hour of its landing rate; otherwise it proceeds. Brief big or uncalibrated items at budget 1 so they can split.
- Start every `tick` with `threads unowned`. Give each priority:now UNOWNED row a worker (a cloud worker first when the item is code-only), or record why it cannot be staffed with `threads block <repo#N> <reason>`.
- Schedule `tick` only from `south:agents/routines/`: run CronList, then CronCreate with `agents routines pointer tick` only if no session cron carries it.
- Write the status file in plain sentences, one line per item, and end each line with `priority:now unowned=N blocked=M owned=K`.
- Read `agents --help`, `threads --help` and `worker-ledger --help` for command detail, and [notes](references/notes.md) for ledger rules, compute routing, Autoland, recycling and ticks.
