---
name: planning
description: >-
  Plan consequential engineering work. Use when a decision changes
  architecture, persisted data, a durable contract or dependency, or another
  protected surface; is expensive to reverse; claims novelty; or is explicitly
  a milestone.
---

# Planning

Establish the local engineering context before applying engineering taste.

Use first principles inside the novelty boundary, boring engineering outside
it, and evidence proportional to irreversibility.

## Orient the decision

Establish:

- **Objective** — the capability to deliver or uncertainty to remove.
- **Protected surfaces** — real consumers, persisted data, public contracts,
  actual compatibility obligations, invariants, and authority boundaries.
- **Novelty boundary** — where invention is strategically necessary.
- **Rewrite zone** — what may be replaced or deleted freely.
- **Reversibility** — recovery cost and blast radius if wrong.
- **Evidence burden** — proof proportionate to irreversibility and consequence.

Do not let labels such as `MVP`, `research`, or `production` decide posture by
themselves. Derive posture from these facts and scope it to the decision,
workstream, or milestone rather than automatically to the whole project.

Skip planning ceremony for obvious, local, low-risk edits inside a settled
direction.

## Route the work

Use `prior-art` when the work:

- claims novelty;
- creates a protocol, dependency, persisted representation, or durable
  interface;
- touches a protected surface;
- is expensive to reverse; or
- proposes custom infrastructure for a commodity capability.

Load only the domain knowledge and heuristics needed for the decision. Treat
their authority distinctly:

- `knowledge` supplies facts, idioms, and ecosystem practice;
- `heuristic` supplies a fallible preference with applicability and
  counter-signals;
- `policy` supplies a non-negotiable authority or safety constraint; and
- `procedure` supplies an ordered workflow with entry and exit conditions.

A heuristic never overrides policy, protected surfaces, or the declared
project thesis. Use `verification` to select proportionate evidence and `todo`
only when continuity must survive the current response.

## Choose plan depth

Use a task plan for bounded execution inside a settled direction:

```text
Goal:
Constraints:
Steps:
Verification:
```

Use a milestone plan only when the work creates a durable capability, removes
a material uncertainty, or crosses an operational boundary:

```text
Milestone

Capability delta:
Non-goals:

Decision posture
  Objective:
  Protected surfaces:
  Novelty boundary:
  Rewrite zone:
  Reversibility:
  Evidence burden:

Precedent
  Conventional answer:
  Closest credible alternatives:
  What we can reuse:
  Deliberate departures:
  Remaining uncertainty:

Strategy
  First falsifiable step:
  Critical path:
  Pivot / stop conditions:
  Exit proof:
```

Omit `Precedent` only when the direction is already settled and no research
trigger applies.

## Set the autonomy envelope

Let the executor act freely inside the rewrite zone while preserving protected
surfaces and required evidence. Escalate only when new information materially
changes a protected surface, failure survivability or containment, the novelty
boundary, or the evidence needed for acceptance. Do not reopen choices the plan
already authorized.

## Guardrails

- Let decision consequence, not task size, determine planning depth.
- Do not preserve a surface merely because it exists.
- Do not invent outside the novelty boundary without showing why conventional
  answers fail.
- Do not turn prior-art work into a bibliography.
- Do not atomize a milestone into an unordered task pile.
- Prefer the smallest owned surface that preserves the thesis.
- Make the first proving step capable of falsifying the direction, not merely
  beginning implementation.

When invoked for planning, produce the plan in the current response. Do not
stop at an intent statement or ask permission unless an unresolved choice
would materially change the result.

Produce the smallest plan that makes the engineering posture, critical path,
autonomy boundary, and completion evidence unambiguous.
