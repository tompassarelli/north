---
name: planning-reference
description: Full notes on plan depth, decision records, dependencies, and stopping criteria.
---

# Planning: full notes

## Purpose and boundary

A plan resolves costly choices and orders necessary implementation. It is not
proof of diligence, a second product, or a prerequisite for an obvious edit.
Begin with the requested usable artifact; include a step only if it produces
that artifact or changes the immediate next action.

## Minimal useful plan

Capture the outcome, scope, constraints, behavior that must survive, decisions
settled here versus left local, ordered work, and completion evidence.
A short paragraph can be enough for a bounded change.

For a consequential milestone, add only the affected boundaries: capability
gained, rejected alternatives and reasons, migration/rollout needs, failure
containment, and the first result that would disprove the approach. A milestone
does not automatically require every lifecycle mechanism.

## Dependencies and seams

A step should make clear what it consumes, produces, and depends on.
Name authority, consumers, and durable/public surfaces where those facts change
the design. Split independently implementable pieces at real integration seams;
keep tightly coupled decisions together.

A plan is executable when the next step can start without repeating discovery
and its completion is observable. A sequence of vague verbs is not executable.

## Revision and stopping

When evidence invalidates a premise, revise the remaining route rather than
defending the document. Keep already useful work. After the artifact and
decision-changing check exist, stop; do not complete stale plan items merely
because they were written down.

Optional worksheet:

```text
Outcome and scope:
Constraints / behavior to preserve:
Decisions and decisive evidence:
Required steps and dependencies:
Completion boundary:
Unresolved owner decision, if any:
```
