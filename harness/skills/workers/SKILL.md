---
name: workers
description: >-
  Lead-side staffing: spawn, restaff, review, park or land workers or leads; tiers, ETAs, briefs, reports; cloud workers, RemoteTrigger; auto-wake; fleet take-over; Codex leads.
grounded: 2026-10-15
written: 2026-11-04
---
# Workers

Throughput is lost to serialized long waits, not thinking: each tick cut the longest wait on the critical path, and while a repository's landing queue grows (`agents flow`) widen its landing path before adding workers. Rules hold on any provider unless marked Claude-only; Tom staffs Claude alone since 2026-10-16, and `agents plan` picks provider, account and fast mode from live headroom, never by hand.

## Staff

- One task per worker, and one worker per scarce client or device instance, because shared instances corrupt each other's runs. Check `north threads list` and open issues first; never duplicate a held item.
- Start a brief with `north brief new --category X`, which prints every header line the brief guard requires for that category.
- Pick the tier with `north worker-ledger --recommend '<Category and facet lines>'` and take the ETA from its `eta` line, or write your estimate with `(uncalibrated)` on `eta n/a`. Size each box to 20 minutes or less, or name why.

| Provider | Tier | Model / invocation |
| --- | --- | --- |
| Claude | haiku | `worker-haiku`, Haiku 5.5 high |
| Claude | sonnet-high | `worker-sonnet`, Sonnet 5.5 high; first tier for known-cause bugs and art, the retry after a Haiku miss, one arm on tooling and features, before `worker` in the escalation |
| Claude | medium | `worker`, Opus 5.5 medium |
| Claude | high, xhigh | `worker-high` or `worker-xhigh`, Opus 5.5 high or xhigh, escalation only |

- Routing: mechanical to `worker-haiku` (a miss retries at `sonnet-high`, never Opus; its brief says to wait on any run over a minute with Monitor, because Haiku ended its turn mid-run); art to `sonnet-high`; the armed categories ([tiers.md](references/tiers.md)) alternate `Arm: sonnet-high` and `Arm: worker`; docs-policy, unknown-cause debugging, netcode-determinism, architecture, planning and every other Opus-range item to `worker`. Never use Sonnet other than sonnet-high or Opus low; Fable only when Tom names it. `north dispatch` and `north fleet launch` route from the brief's `Category:` line, so launch with no `--model`.
- Restaff every stop at the same tier as a fresh worker with `Follows: <agent id and tier>` and the failed evidence; escalate one tier only after a second `Stop: reasoning` on the same box, and read [tiers.md](references/tiers.md) for which arm that stop goes to. Send a running worker only its own task's follow-up, and never revive a finished worker by message, because its context is spent on the old attempt.
- Brief header, before the first blank line, then goal, Sources and the state checked at spawn: `Item:`, `Category:`, `Spec:`, `Scope:`, `Surface:`, `Verify:`, `Done when:` (first line an outside oracle, with a backticked command), `ETA:`, `Files:`, any `Follows:` and `Needs:`.
- Read [tiers.md](references/tiers.md) when a routing case above does not settle the tier or the next tier cannot run here; [brief.md](references/brief.md) whenever you write a brief, pick ready work, queue a box behind another lane, or do research Tom asks for inline; [codex-sessions.md](references/codex-sessions.md) before you launch a Codex session, write a Codex brief or route a box to Codex; [runtime-incidents.md](references/runtime-incidents.md) when a worker or lead fails to start, dies, stops answering or misreports.

## Placement

- Where a box runs: a box that builds, waits or lands is a fleet session (`north fleet launch`); an Agent subagent is only for `Category: research`, and the spawn guard refuses other `worker*` Agent briefs.
- `north fleet launch` places each box before it starts, cloud first for code-only work. Read [cloud.md](references/cloud.md#placement) when a box needs a local client, a GPU or the laptop, when placement holds it or you override it.

## Review

- No landing carries a review trailer. Read [review.md](references/review.md) before you land a lane, staff a reviewer or planner, or weigh findings; reviews run only for planner designs, trust-boundary code and judged lanes.

## Run, park and land

- Read [run-and-land.md](references/run-and-land.md) when you plan a tick, wait on a long run, a report is late, you give a brief a Budget line, file a process idea, you land or own lanes, landings fail across lanes, staff native workers or get a `north flow` alert; [fleet.md](references/fleet.md) when you launch a child session, message one, wait on its report, or hand a proxy or lead to a successor.
- Run anything that may take over about 45 seconds in the background, because a foreground wait delays every message to you.
- Finish an issue's last unchecked box before starting new work, and after an hour with no closure start nothing new until an open box closes, because open boxes are the throughput.
- Tell Tom only what changes his next action: a landed fix (with its build), a result, or a decision; never relay a worker's interim "still waiting".

## Reports

- Every report begins `Done:`, `Not done:`, `Blocked:` or `Split:`, and every Not done or handoff names `Stop: landing|capacity|client|leash|reasoning|decision`, so the ledger can classify it. The last message is the report: keep no status files and never resume a worker to check on it.
- Decide routine calls yourself. A `Stop: decision` or `Blocked: decision` report must name an open gate from `north gate open ... --recommend N --default N --default-after D`; the fleet answers the first ungated one instead of forwarding it, and forwards a later one with a warning (Tom, 2026-10-29).
- Report a fix as landed only after fetching origin and verifying its SHA is an ancestor of `origin/main`, because queued or Autoland-green is not landed; until then report `Done: queued <worktree> <branch> <sha>`.
- A `Split:` puts its parts as boxes on its own issue, never new issues; read [run-and-land.md](references/run-and-land.md#reports) when a worker splits or reports findings.
