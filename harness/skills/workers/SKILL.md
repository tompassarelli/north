---
name: workers
description: >-
  Lead-side staffing: spawn, restaff, review, park or land workers or leads; tiers, ETAs, briefs, reports; cloud workers; auto-wake; fleet take-over.
grounded: 2026-11-12
written: 2026-11-12
---
# Workers

Throughput is lost to serialized long waits, not thinking: each tick cut the longest wait on the critical path, and while a repository's landing queue grows (`north ledger flow`) widen its landing path before adding workers. `agents plan` picks provider, account and fast mode from live headroom, never by hand.

## Staff

- One task per worker, and one worker per scarce client or device instance, because shared instances corrupt each other's runs. Check `north threads list` and open issues; never duplicate a held item.
- Start a brief with `north work brief new --category X`, which prints every header line the brief guard requires for that category.
- Take the ETA from the `eta` line of `north ledger worker --recommend '<Category and facet lines>'`, or mark your estimate `(uncalibrated)` on `eta n/a`. Size each box to 20 minutes or less or name why.
- Launch with no `--model`: `north fleet dispatch` and `north fleet launch` route from the brief's `Category:` line. A Haiku brief says to wait on any run over a minute with Monitor, because Haiku ended its turn mid-run.
- `north fleet dispatch` staffs DeepSeek on boxes outside credentials, sandbox and locking up to the cap in harness/opencode.toml, and sends each DeepSeek `Done:` to a sampled Sonnet review or to safe-push; never launch, relay or forget a DeepSeek worker by hand.
- Admit a run only when it produces part of the artifact or its result changes the next action and you can name that fork first; uncertainty, confidence, completeness and idle capacity admit none, and no scout, reviewer, verifier, judge, watchdog, status collector or second coordinator shadows ordinary delivery unless asked for that deliverable or an external boundary's answer changes the decision.
- A box needs the owner only for a direction choice or a subjective feel judgment; send the owner no test checklist unless they asked for a playtest.
- Restaff every stop at the same tier as a fresh worker with `Follows: <agent id and tier>` and the failed evidence; `dispatch::spin` escalates only on `Stop: reasoning`, so keep every other stop on its rung. Message a running worker only with its own task's follow-up; never revive a finished one by message, because its context is spent.
- Brief header, before the first blank line, then goal, Sources and the state checked at spawn: `Item:`, `Category:`, `Spec:`, `Scope:`, `Surface:`, `Verify:`, `Done when:` (first line an outside oracle, with a backticked command), `ETA:`, `Files:`, any `Follows:` and `Needs:`.
- Read [tiers.md](references/tiers.md) when the routing does not settle the tier or the next tier cannot run here; [brief.md](references/brief.md) whenever you write a brief, pick ready work, queue a box behind another lane, or do research Tom asks for inline; [runtime-incidents.md](references/runtime-incidents.md) when a worker or lead fails to start, dies, stops answering or misreports.

## Placement

- `north fleet launch` places each box before it starts, cloud first for code-only work. Read [cloud.md](references/cloud.md#placement) when a box needs a local client, a GPU or the laptop, when placement holds it or you override it.

## Review

- Read [review.md](references/review.md) before you land a lane, staff a reviewer or planner, or weigh findings; reviews run only for planner designs, trust-boundary code and judged lanes.

## Run, park and land

- Read [run-and-land.md](references/run-and-land.md) when you plan a tick, wait on a long run, a report is late, you land or own lanes, landings fail across lanes, staff native workers or get a `north ledger flow` alert; [fleet.md](references/fleet.md) when you launch a child session, message one, wait on its report, or hand a proxy or lead to a successor.
- Finish an issue's last unchecked box before new work, and after an hour with no closure start nothing new until an open box closes.
- Tell Tom only what changes his next action: a landed fix (with its build), a result, or a decision; never relay a worker's interim "still waiting".

## Reports

- Every report begins `Done:`, `Not done:`, `Blocked:` or `Split:`, and every Not done or handoff names `Stop: landing|capacity|client|leash|reasoning|decision`. The last message is the report: keep no status files and never resume a worker to check on it.
- Upward messages are by exception: a child reports a Done, a landing, a refusal or red it cannot fix, or a blocker needing authority it lacks; a lead speaks down when priorities shift.
- Decide routine calls yourself. A `Stop: decision` or `Blocked: decision` report must name an open gate from `north gate open ... --recommend N --default N --default-after D` (or `--ask-once CATEGORY` in place of the default for an ask-once matter) (Tom, 2026-10-29).
- Report a fix as landed only once its SHA is an ancestor of a freshly fetched `origin/main`, because queued or Autoland-green is not landed; until then report `Done: queued <worktree> <branch> <sha>`.
- A `Split:` puts its parts as boxes on its own issue; read [run-and-land.md](references/run-and-land.md#reports) when a worker splits or reports findings.
