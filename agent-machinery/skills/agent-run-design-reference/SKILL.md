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

## Stock template or bespoke composition

Compare responsibility, deliverable, topology, capabilities, decision
authority, escalation, done criteria, and report shape. A mismatch requires a
bespoke composition. Only task grade, domains, capability floor, service class,
reasoning, and posture are overridable within the stock contract; record the
changed fields and one reason.

Template and role IDs may differ. `composition.id` records the source
template; `role` names this run's responsibility. Use `--template ID` where
the command requires that distinction.

For bespoke work, specify a stable composition ID, the nine routing fields,
supplied context, canonical capabilities, permitted decisions, escalation
conditions, observable completion, and report shape. A nearby template can
seed values but contributes no extra authority.

## Consumer boundary

Generate with `agent-machinery-compose-routing ROLE`, then validate using the
catalog-advertised `validateContract` export, not JSON Schema alone.
The consumer must map every capability fail-closed before admission.

Provider, model, account, runtime, lease, connectivity, and settlement remain
execution facts. A lease race changes available inventory, not the portable
request or an independently invented fallback order.

## Common confusion

A more senior role does not automatically require a premium service class.
Higher effort does not compensate for a capability floor below the task.
Neither a template nor a successful resolution acknowledges work ownership.
Those are separate contracts and must remain independently inspectable.
