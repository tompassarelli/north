---
name: agent-run-design-reference
description: >-
  Detailed reference for $agent-run-design-distilled. Use only when that skill
  identifies an unresolved run-design procedure or when the user explicitly
  requests this reference.
---

# Agent run design reference

## Template comparison

Compare a stock template across responsibility, deliverable, topology,
capabilities, decision authority, escalation conditions, done criteria, and
report shape. A mismatch in any of those requires a bespoke composition.
Override only task grade, domain requirements, capability floor, service
class, reasoning, or posture and
record the exact changed fields plus one reason.

Template identity and role are independent. `composition.id` names the stock
template that supplied the behavior contract; `role` names the responsibility
assigned to this run. Use `--template ID` when those IDs differ. Metadata never
adds capabilities, decomposition authority, ownership, or access.

## Bespoke worksheet

Record a stable composition ID; role; responsibility and deliverable; task
grade; domain requirements and supplied context; worker or orchestrator
topology; capability floor, service class, and reasoning; posture; canonical capabilities; permitted
decisions; escalation conditions; observable done criteria; and report shape.
A nearest template may seed values but contributes no authority.

## Consumer handoff

Generate the request with `agent-machinery-compose-routing ROLE`. Validate it
through the catalog-advertised `validateContract` export and admit it only
after the consumer maps every capability fail-closed. Keep template provenance
within `composition`; keep lifecycle and concrete execution facts outside the
nine portable fields.
