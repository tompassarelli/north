---
name: agent-run-design-reference
description: Run-design rationale, template comparison, bespoke composition, and consumer handoff.
---

# Run design: full notes

## Adopted model

The portable request describes the work, not the machine executing it.
Role, task grade, domain requirements, topology, capability floor, service
class, reasoning, posture, and composition are independent routing fields.
A template supplies a behavior contract; its ID does not grant authority.

This separation allows the same work to be resolved against live inventory
without rewriting its competence requirements or accepted scope.

The listener delegates independently executable delivery by default when it
can proceed alongside useful primary work, retaining judgment and reconciliation.
Cohesion determines the owner of each piece, not an obligation for the primary
to implement it. Trivial direct work and genuinely non-delegable coupled steps
remain direct; no fixed number of children or supervision tiers is required.

## Stock template or bespoke composition

Compare responsibility, deliverable, topology, capabilities, decision
authority, escalation, done criteria, and report shape. A mismatch requires a
bespoke composition. Only task grade, domains, capability floor, service class,
reasoning, and posture are overridable within the stock contract; record the
changed fields and one reason.

Classify topology from the delegated deliverable's local dependency shape
before selecting a template or composing its launch brief. A delegated
workstream containing independently deliverable pieces can itself need a
director, team-lead, or bespoke orchestrator; its position below another run
does not make it a worker. Give an orchestrator an enforceable coordination
capability and omit terminal-worker restrictions. Keep one atomic, cohesive, or
tightly coupled piece with a worker and explicitly make that route terminal.
Do not create an orchestrator merely because work is large, important, could
occupy more agents, or would benefit from someone watching it.

Template and role IDs may differ. `composition.id` records the source
template; `role` names this run's responsibility. Use `--template ID` where
the command requires that distinction.

For bespoke work, specify a stable composition ID, the nine routing fields,
supplied context, canonical capabilities, permitted decisions, escalation
conditions, observable completion, and report shape. A nearby template can
seed values but contributes no extra authority.

Topology controls terminal versus coordinating authority; filesystem and shell
authority remain separately declared and consumer-enforced. Current stock
orchestrator templates are read-only, and selecting one preserves that fixed
boundary. Use a bespoke orchestrator when the coordinating deliverable itself
needs bounded integration edits or when its runtime must retain already
authorized implementation authority for descendants. Declare the closed
capability set and its scope in the contract. Supervision alone grants no
authority, authoring capability does not license absorbing unrelated terminal
work, and a runtime with effective full access must not be represented as
read-only.

## Consumer boundary

Generate with `agent-machinery-compose-routing ROLE`, then validate using the
catalog-advertised `validateContract` export, not JSON Schema alone.
The consumer must map every capability fail-closed before admission.

Provider, model, account, runtime, lease, connectivity, and settlement remain
execution facts. A lease race changes available inventory, not the portable
request or an independently invented fallback order.

If a worker discovers that its deliverable now has independent pieces, it
reports that shape instead of delegating or silently acquiring coordination
authority. At a safe checkpoint, the accountable parent settles the current
run as needed, reclassifies the remaining deliverable, and re-admits it with a
complete routing request and acknowledged ownership. That parent acts without
new human permission when delegation remains inside its accepted scope and it
already has coordination authority; otherwise it escalates the missing
authority.

Runtime flags and transport must implement the admitted topology. A failed
coordination transport is a runtime defect or blocker, not evidence that the
deliverable became atomic. Restore or repair the orchestrator path rather than
silently relaunching the responsibility as a terminal worker. Conversely, do
not leave delegation enabled on a deliberately terminal worker route.

## Common confusion

A more senior role does not automatically require a premium service class.
Higher effort does not compensate for a capability floor below the task.
Neither a template nor a successful resolution acknowledges work ownership.
Those are separate contracts and must remain independently inspectable.

## Model-specific staffing

Primary/overseer work uses Astra high by default and xhigh for greater complexity.
Substantive terminal workers default to Astra medium. Low needs affirmative
evidence that semantics and method are settled and a clear oracle exists; a
clear desired outcome alone is insufficient. High and xhigh follow the actual
unresolved reasoning demands. Luna xhigh/max remains an affirmatively justified
mostly mechanical exception. Sol is available through explicit choice or a
declared bounded comparison, never ordinary automatic fast/economy selection.
Authoring defaults do not rewrite an explicitly requested model or effort.
Astra max requires a named
load-bearing decision with significant costly-to-reverse consequences; ordinary
architecture does not automatically qualify. Supervision remains orthogonal to
role, topology, and capabilities: the consumer marks a primary using selection
context, while orchestrator topology independently implies overseer protection.

The operator places Astra low around Sol high–xhigh competence, Astra medium above
Sol max, and Astra high/xhigh in the upper tier. Those beliefs are priors,
not evidence, and equal effort labels are not equal competence across models.
The selection catalog therefore declares model-by-effort competence floors and
automatic/experimental eligibility. Suitable worker experiments compare Astra
low/medium with Sol low through xhigh, capped by the existing seeded assignment
and quality policy. Same-model experiments retain the adjacent-effort bound;
cross-model comparisons use declared model-effort competence instead of that
misleading distance. Luna xhigh/max also participates on mechanical work. Overseers, Astra max,
named load-bearing decisions, and
explicit model or effort pins never enter downshift experiments.

One substantive Luna failure escalates only that task to Astra. The listener
passes the failing check, partial artifact, and already located context in the
next accepted brief and sets the existing model constraint to Astra for that
resumed run. Do not repeatedly attempt Luna repairs, discard useful context, or
infer a global model prohibition from one failure. Record the outcome through
the existing evidence surface; task escalation and aggregate calibration differ.

See agent-machinery:docs/routing.md for the existing resolver context, assignment
record, evidence, and consumer-owned persistence boundary. Never add a second
consumer routing table or present the policy change as an observed benchmark.
