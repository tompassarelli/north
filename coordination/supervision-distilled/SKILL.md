---
name: supervision-distilled
description: >-
  Supervise direct child runs after an explicit supervisor, root-supervisor,
  or foreman assignment, or whenever the current run has orchestrator topology.
  Use for same-session delegation rounds, active-child waiting, mailbox
  reconciliation, interruption handling, and terminal settlement.
---

# Supervise direct children

Treat supervision as an active responsibility, not a title. Agent Machinery
owns actors, acknowledged work ownership, and the eight-field portable route;
North owns concrete admission, collaboration transport, wait/wake, telemetry,
and settlement. Do not add staffing, a ninth routing field, or a new task
species.

## Admit and account

- Admit only independent shortest-path work whose time saved exceeds its
  integration cost. Give every child a complete route and bounded authority.
- Offer or transfer the work, obtain the required acknowledgement, and retain
  accountable-parent responsibility for each direct child.
- Keep the live direct-child set and each accepted outcome in the current
  transcript. Same-session coordination uses in-memory collaboration;
  cross-session intentions, messages, and acknowledgements use North/Store.
  Do not create Store bookkeeping for ordinary same-session work.

## Stay in the foreman loop

Use the wait surface that owns the thing being waited on:

- `collaboration.wait_agent` waits for direct-child or mailbox lifecycle.
- `functions.wait` resumes only a running `functions.exec` cell and is valid
  only with the exact `cell_id` yielded by that exec call. It is never a
  supervision, mailbox, elapsed-time, or child-liveness wait.

Never invent, remember by resemblance, or probe an exec cell ID. If
`functions.wait` reports that a cell does not exist, do not retry it with the
same or another guessed ID. Quarantine that entrance for supervision, preserve
the deterministic failure as incident evidence, and return directly to the
native collaboration surface.

While any acknowledged direct child is live or any completion remains
unconsumed or unreconciled:

1. Enter a bounded native wait with `collaboration.wait_agent`; use intervals
   long enough to give the child working space and short enough to remain
   responsive, normally minutes rather than seconds. The wait returns early on
   mailbox or user activity, so its multi-minute bound is not response latency.
2. React immediately when the mailbox changes. Consume completed results,
   answer decisions or escalations, resolve seams, and settle the corresponding
   ownership and lifecycle state.
3. After a quiet interval, inspect direct-child state with the native
   collaboration surface. Ask for a checkpoint only when state is ambiguous or
   progress needs a decision; do not manufacture chatter.
4. Make another native wait the next lifecycle action after every reconciliation
   round until no acknowledged direct child is live and no completion remains
   unconsumed or unreconciled. A completed result is unsettled until consumed
   and reconciled.

Never end merely because children run in the background. Do not replace this
loop with a watcher child, daemon, service, raw transcript tailer, or Store
listener. Passage of time is not a hook event and cannot be made deterministic
by prose.

## Contain unreadable collaboration payloads

Invoke collaboration lifecycle tools only through the native collaboration
namespace. Never retry a collaboration lifecycle operation through
`functions.*`.

Treat a child's report of an opaque `gAAAA...` envelope as a failed delivery
even when the collaboration call reported success. It conveys no task,
authority, acknowledgement, result, or ownership transition. The recipient
must not infer, decrypt, quote, or act on it.

After the first opaque payload on a child channel:

1. Quarantine that channel from further task and authority messages. Do not
   probe it by resending the payload, shortening it, or asking the child to
   recover its meaning.
2. Preserve an incident record and any owned lane unchanged, then interrupt
   the affected child without another payload delivery.
3. If the task remains on the shortest delivery path, admit at most one fresh
   full-history, self-describing replacement whose exact bounded task is
   recoverable without the defective payload.
4. Keep the original delivery and ownership state unsettled. Replacement or
   fallback success may contain delivery, but it is not a repair and does not
   prove the primary path healthy.

Report only the normalized failure and affected seam; never copy or forward
the opaque bytes. Additional reproduction, upstream runtime repair, activation,
and a primary-path canary require their own explicit authority.

## Interrupt and settle

An operator or parent interruption stops new admissions and further gate work.
Allow only an already-running atomic action to reach a safe point, request the
partial result, and reconcile or explicitly transfer the remainder. Do not let
an interrupted child continue a check as if its authority were unchanged.

Stop only when every direct child is reconciled and settled, the operator
explicitly pauses or ends the work, an acknowledged transfer moves the
outstanding responsibility, or a valid `NotificationHandoffReceipt/v1` proves
autonomous reactivation. A PID, background output, delegated marker, or armed
listener is not that receipt. When the operator ends work with live children,
report their exact unsettled state rather than silently declaring settlement.
