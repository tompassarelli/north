---
name: agent-run-design
description: >-
  Design a portable agent run and resolve its execution plan before admission, using a stock template or a bespoke composition.
---

# Agent run design

Ownership and run design are separate: choosing a role or provider transfers no
work. Use `work-ownership-v1` for acceptance and transfer.

Admit a run only when it produces a required artifact or changes the immediate
next action. The primary owns judgment and reconciliation and by default
delegates independently executable delivery alongside useful primary work.
Keep each coupled piece with one owner, not automatically with the primary.
Direct trivial work and genuinely non-delegable coupled steps stay direct;
no mandatory tier or child count follows.
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
consumer cannot enforce. Topology controls terminal versus coordinating
authority, not filesystem or shell authority. Keep stock template capabilities
fixed; use a bespoke orchestrator when its coordinating responsibility
genuinely needs scoped integration authority or the runtime must preserve
authorized implementation authority for descendants. Supervision alone grants
none, authoring authority does not license unrelated terminal work, and actual
full runtime authority must not be described as read-only.

Apply terminal no-delegation instructions only after deliberately choosing a
worker route. Workers remain terminal and escalate decomposition; they never
self-upgrade topology.

When dependency shape evolves, the accountable parent reclassifies at a safe
checkpoint and re-admits the work with a complete route and acknowledged
ownership. In-scope delegation needs no human permission when that parent
already holds coordination authority. A runtime or transport failure must not
silently downgrade an admitted orchestrator or strand the work under a worker
brief: restore the admitted topology, and keep runtime flags aligned with it.

The consumer picks model and effort and owns accounts, leases, access mapping,
dispatch, communication, and settlement. Claude workers: Haiku 5.5 high for
mechanical work, Opus 5.5 medium by default, Opus high for hard work. Codex
workers: SOL 6.1 medium by default and as the floor, SOL high for hard work,
Astra xhigh only after a failed high attempt. Never use low or max. Explicit
model or effort choices are never rewritten.

## Worked contrasts

- Bad: routing a single self-contained bug fix through a bespoke orchestrator
  because it "touches three files." Good: one worker owns it end to end;
  file count and idle capacity are not topology triggers.
- Bad: starting at the top tier because a task might be hard. Good: start at
  the medium default and move up one tier after a failed attempt, passing the
  failing check and known context.

For template comparison or a bespoke handoff, use
`references/notes.md`.
