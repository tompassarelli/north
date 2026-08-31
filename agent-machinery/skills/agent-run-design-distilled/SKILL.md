---
name: agent-run-design-distilled
description: >-
  Design the portable run side of delegation from a role, execution
  requirements, and a stock or bespoke composition, then resolve its ranked
  provider/model/effort plan from live inventory. Use before a concrete run is
  admitted, including when no stock template fits.
---

# Agent run design

Delegation has two separate seams. `work-ownership-v1` determines which
intentional actor owns a piece of work; run design determines the portable
contract for a run that may accept it. Choosing a role, template, or provider
does not transfer ownership.

Before composing a run, require one of two facts: it directly produces part of
the requested artifact, or its result changes the immediate next action. Name
the action fork internally. If every possible result leads to the same action,
do not compose or admit the run. Uncertainty, confidence, completeness,
possible usefulness, idle capacity, and independent confirmation are not work.

During delivery, keep one shortest-path DAG. Parallelize only independent
artifact-producing nodes already on it. Do not compose read-only shadow roles
to watch, audit, resnapshot, review, verify, inventory, collect status, census
processes, or supervise another supervisor. Those roles require an explicit
informational or assurance deliverable, or a named external boundary whose
answer changes the immediate delivery decision. Observe direct children
directly and keep tightly coupled work with one owner.

Read `agent-machinery:doctrine.md`, `agent-machinery:staffing/catalog.json`,
and `agent-machinery:docs/routing.md`. Resolve `project-exposure-v1`, classify
the role and every other route axis independently, then use a stock template
only when its complete behavior and authority contract fits. Otherwise create
a bespoke composition. Never lower a capability floor or admit authority the
consumer cannot enforce.

Emit exactly `role`, `taskGrade`, `domainRequirements`, `topology`,
`capabilityFloor`, `serviceClass`, `reasoning`, `posture`, and `composition`.
A template ID is provenance metadata
inside `composition` and need not equal `role`; it grants neither ownership nor
runtime access. A worker remains terminal even when its brief reveals useful
decomposition and must escalate that signal to its immediate parent.

Return the nine fields, canonical capabilities, supplied domain context, and
reasons for overrides or bespoke boundaries. Run lifecycle, wake, wait, rearm,
Stop, transport, account/lease selection, and runtime access mapping remain
consumer responsibilities. Agent Machinery resolves the provider/model/effort
plan from its catalog, empirical evidence, and the consumer's live inventory.

For the comparison worksheet and CLI handoff, run
`agents path agent-run-design-reference` and read its `SKILL.md` completely.
