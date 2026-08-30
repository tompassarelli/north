---
name: agent-run-lifecycle-distilled
description: >-
  Admit, host, communicate with, supervise, restore, and settle concrete North
  agent runs after Agent Machinery has supplied the portable run design. Use
  for provider/account/runtime selection, graph driver operations, live input,
  wake/wait/rearm/Stop behavior, fallback, or terminal settlement.
---

# Operate an agent run

Start from exactly `role`, `taskGrade`, `domainRequirements`, `topology`,
`tier`, `reasoning`, `posture`, and `composition`. North maps that design to an
enforceable runtime, selects provider/account/model, records graph driver state,
operates message and live-input transport, and owns the concrete run through
settlement. Never add a runtime fact as a ninth portable field.

Treat `work-ownership-v1` as the authority for actor ownership: a driver claim,
message, wake, or launch does not accept or transfer work. Fall back only with
typed proof that no provider side effect became observable. Preserve requested
and resolved route evidence across restoration, keep unresolved restoration
debt visible, and settle process, delivery, driver, and parent/child state
before calling the run complete.

Apply `supervision-distilled` whenever a listener or run is explicitly assigned
supervisor, root-supervisor, or foreman responsibility, or whenever its topology
is orchestrator. Use same-session in-memory collaboration for live children and
North/Store only for cross-session intentions, messages, and acknowledgements.

Arm a listener or rearm after Stop only for live delivery the current run owns.
Attempt each required coordination operation once; retain the exact undelivered
operation on failure instead of probing, retrying, or switching channels.

A notification-dependent handoff is valid only with a machine-readable
`NotificationHandoffReceipt/v1` naming the owning recipient, awaited peer or
run, transport mechanism, subscription generation, observation and expiry, and
runtime-proved `autonomousReactivation=true`. Process presence, North listener
or lease state, background output, and same-turn polling are not proof. Without
a valid receipt, do not end while the peer remains awaited: continue bounded
`collaboration.wait_agent` or listener polling, or transfer to a live monitor
whose reactivation path passed OPEN→ACK.

An operator or parent interruption stops admission and further gate work. Let
only an already-running atomic action reach a safe point, then obtain the
partial result and settle the interrupted run; do not treat background process
presence as permission to continue.

Details: `agents path agent-run-lifecycle-reference`.
