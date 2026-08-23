---
name: prior-art
description: >-
  Research internal and external precedent before consequential invention. Use
  when a decision claims novelty, creates a durable protocol, dependency, data
  representation, or interface, is expensive to reverse, or introduces custom
  infrastructure for a commodity capability.
---

# Prior-art synchronization

Know what is being rejected before inventing a replacement. Research must
change a decision, constrain a design, or retire a named uncertainty; it is not
a literature-review ritual.

Skip explicit research for mechanical work inside an already settled design.

## Frame the research

Establish before researching:

```text
Decision:
Requirement or uncertainty:
Protected surfaces:
Novelty boundary:
Research stopping condition:
```

Do not begin with a broad topic such as “research compilers.” Begin with the
decision the research must inform.

## Synchronize precedent

### Check internal precedent

Inspect the project before the internet:

- current architecture and invariants;
- previous decisions and rejected approaches;
- repository patterns and existing dependencies; and
- benchmarks, regressions, incidents, and migration history.

Do not propose an external pattern while ignoring knowledge the system already
accumulated.

### Check external precedent

Using available and authorized research tools, find the conventional answer and
the closest credible alternatives. Prefer primary sources: official
documentation, specifications, source repositories, papers, and first-party
design notes. Use contemporary sources when ecosystem state matters, and search
adjacent fields when a new abstraction may use different vocabulary.

Use `greenfield` for current new-dependency choices. Before leveraging external
code or copied expression, use `external-code` to establish its license and
attribution boundary.

### Check operational precedent

Look for evidence about performance and scaling, maintenance and migration
burden, failure modes and security properties, adoption constraints, and exit
costs. Do not infer production behavior from an API description alone.

## Build the option set

Consider without treating the list as a moral ladder:

```text
use an existing capability
configure or combine existing tools
extend
fork
build a focused replacement
invent a new architecture
```

Evaluate credible options against fit, preservation of the project thesis,
integration complexity, long-term ownership burden, replacement and exit cost,
and control, performance, security, and portability constraints. Choose the
smallest owned surface that satisfies the requirement and preserves the thesis.

For every material custom component, state:

```text
Needed capability:
Why existing approaches are insufficient:
Smallest custom surface required:
How the claim will be tested:
```

“Not invented here,” aesthetic preference, and vague claims of flexibility are
not sufficient reasons.

## Stop on a decision

Stop when the planner can answer:

```text
Conventional answer:
Closest credible alternatives:
Requirement they fail:
What we will reuse:
What we will deliberately reject:
Experiment that could invalidate our departure:
```

Use `verification` to select the smallest credible falsifying experiment. Do
not keep researching merely because more related material exists.

Return a compact decision brief:

```text
Decision brief

Question:
Conventional answer:
Closest alternatives:
Internal precedent:
Operational evidence:
Reusable pieces:
Deliberate departure:
Remaining uncertainty:
Falsifying experiment:
Sources / provenance:
```

Pass forward the decision, constraints, evidence, and unresolved uncertainty,
not the entire research trail. Do not produce a bibliography without a
decision, enumerate weak alternatives to make one option inevitable, force
reuse when the abstraction is the research subject, or use “research” to exempt
commodity infrastructure from ordinary engineering discipline.
