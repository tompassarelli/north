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

Read the package doctrine, staffing catalog, and routing guide. Classify each
routing axis independently. A stock template must fit responsibility,
deliverable, topology, capabilities, decisions, completion, and report shape;
otherwise use a bespoke composition.

Emit exactly these fields:

`role`, `taskGrade`, `domainRequirements`, `topology`, `capabilityFloor`,
`serviceClass`, `reasoning`, `posture`, `composition`.

Keep template provenance in `composition`; it need not equal `role` and grants
no authority. Include domain context, canonical capabilities, and reasons for
overrides. Never lower the required capability floor or grant capabilities the
consumer cannot enforce. Workers remain terminal and escalate decomposition.

Agent Machinery resolves provider/model/effort from its catalog and the
consumer's live inventory. The consumer owns accounts, leases, access mapping,
dispatch, communication, and settlement.

For template comparison or a bespoke handoff, use
`agents path agent-run-design-reference`.
