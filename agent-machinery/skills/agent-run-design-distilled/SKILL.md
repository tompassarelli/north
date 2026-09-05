---
name: agent-run-design-distilled
description: >-
  Design a portable agent run and resolve its execution plan before admission, using a stock template or a bespoke composition.
---

# Agent run design

Ownership and run design are separate: choosing a role or provider transfers no
work. Use `work-ownership-v1` for acceptance and transfer.

Admit a run only when it produces a required artifact or changes the immediate
next action. Keep coupled work together; delegate independent required pieces.
A shadow review, inventory, or supervisor needs an explicit deliverable or a
named external boundary whose answer changes delivery.

Read the package doctrine, staffing catalog, and routing guide. Before choosing
a stock template or writing launch restrictions, classify the delegated
deliverable's local dependency shape. Being a child is never evidence for
worker topology: independently deliverable pieces may require a director,
team-lead, or bespoke orchestrator with enforceable coordination; one atomic
or cohesive piece remains a worker. Size, importance, utilization, or a desire
to supervise does not justify orchestration.

Classify each routing axis independently. A stock template must fit
responsibility, deliverable, topology, capabilities, decisions, completion,
and report shape; otherwise use a bespoke composition.

Emit exactly these fields:

`role`, `taskGrade`, `domainRequirements`, `topology`, `capabilityFloor`,
`serviceClass`, `reasoning`, `posture`, `composition`.

Keep template provenance in `composition`; it need not equal `role` and grants
no authority. Include domain context, canonical capabilities, and reasons for
overrides. Never lower the required capability floor or grant capabilities the
consumer cannot enforce. Apply terminal no-delegation instructions only after
deliberately choosing a worker route. Workers remain terminal and escalate
decomposition; they never self-upgrade topology.

When dependency shape evolves, the accountable parent reclassifies at a safe
checkpoint and re-admits the work with a complete route and acknowledged
ownership. In-scope delegation needs no human permission when that parent
already holds coordination authority. A runtime or transport failure must not
silently downgrade an admitted orchestrator or strand the work under a worker
brief: restore the admitted topology, and keep runtime flags aligned with it.

Agent Machinery resolves provider/model/effort from its catalog and the
consumer's live inventory. The consumer owns accounts, leases, access mapping,
dispatch, communication, and settlement.

For template comparison or a bespoke handoff, use
`agents path agent-run-design-reference`.
