---
name: agent-run-lifecycle-reference
description: >-
  North command and operational reference for concrete agent-run admission,
  graph driver state, messaging, wake/wait/rearm/Stop behavior, fallback,
  restoration, and settlement. Use only when agent-run-lifecycle-distilled
  calls for operational detail.
---

# Agent-run lifecycle reference

Agent Machinery emits the exact eight-field portable request. North consumes it
without adding provider, model, account, runtime, dispatch, telemetry, or
ownership fields.

| Operation | North surface |
| --- | --- |
| Launch managed work | `north delegate "<task>" --thread <thread>` or `north spawn <role> "<prompt>"` |
| Inspect a run or thread | `north agents`, `north show <thread>`, `north threads` |
| Assert an operational driver | `north tell <thread> driver <actor>` |
| Retract that exact driver | `north retract <thread> driver <actor>` |
| Read durable pending messages | `north inbox [--as <recipient>]` |
| Record a durable request | `north mention <recipient> "<body>" [--about <thread>]` |
| Deliver urgently to a live recipient | `north interrupt <recipient> "<body>"` |
| Steer a running managed run | `north msg <agent-id> "<body>"` |
| Arm owned real-time delivery | `north listen <agent-id>` |

Offers, acceptance, direct transfers, refusals, and escalations follow
`work-ownership-v1`; the driver commands above are graph operations only.
Mentions permit an offline recipient. Interrupts and managed steering require a
live recipient. A listener is long-running: wait only when the current run owns
that wait, and rearm after Stop only while required child or delivery state
remains unsettled.

## Notification handoff receipt

A notification-dependent handoff requires an unexpired machine-readable receipt
with this complete shape:

```json
{
  "schema": "NotificationHandoffReceipt/v1",
  "owningRecipient": "recipient-id",
  "awaitedPeerOrRun": "peer-or-run-id",
  "transportMechanism": "transport-id",
  "subscriptionGeneration": "generation-id",
  "observedAt": "timestamp",
  "expiresAt": "timestamp",
  "autonomousReactivation": true
}
```

`autonomousReactivation` is true only after that transport generation has
proved its wake path at runtime from OPEN through ACK. A PID or process, armed
North listener or lease, background stdout, and same-turn polling are
insufficient. The current North listener cannot wake an idle Codex root and
never proves this receipt. Keep provider/Codex wake repair under separate
incident ownership.

Without a valid receipt, the owner must not end while the peer remains awaited.
Continue bounded `collaboration.wait_agent` or listener polling, or transfer to
a live monitor whose reactivation path passed OPEN→ACK.

North may change provider/account/runtime only before observable provider
acceptance or under a recorded restoration protocol. Keep requested and
resolved route facts, fallback proof, and outstanding restoration debt intact.
Terminal settlement reconciles process outcome, delivery outcome, exact driver
state, and any parent/child settlement before releasing the lifecycle.
