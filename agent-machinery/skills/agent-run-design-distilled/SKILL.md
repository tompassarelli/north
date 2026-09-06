---
name: agent-run-design-distilled
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

Agent Machinery resolves provider/model/effort from its catalog and the
consumer's live inventory. The consumer owns accounts, leases, access mapping,
dispatch, communication, and settlement.

Use Astra high for primary/overseer work, xhigh for higher complexity; exclude
these runs from downshift experiments. For workers use Luna xhigh/max for mostly
mechanical work, Sol low/medium as light-thinking candidates, and Astra low for
substantial well-specified implementation; Astra medium–xhigh covers harder work.
Reserve Astra max for a named load-bearing decision with significant
costly-to-reverse consequences, not architecture in general. Astra low roughly
covers Sol high–xhigh competence; Astra medium exceeds Sol max. These are operator
priors, not measured evidence. Use the existing
bounded experiment sidecar for suitable Astra low/medium versus Sol comparisons,
preserving floors and pins; log assignment and outcomes through calibration.
Pass primary/overseer identity in selection context independently of role and
topology. Effort labels are model-local; do not equate them with competence.
After one substantive Luna failure, transfer that task to Astra with the failed
check, partial artifact, and known context; pin Astra for that resumed task.
Do not turn one task's failure into a global Luna prohibition or restart discovery.

For template comparison or a bespoke handoff, use
`agents path agent-run-design-reference`.
