---
name: build-vs-reuse-reference
description: Full notes on build/reuse ownership, lifetime cost, and decision evidence.
---

# Build or reuse: full notes

## Decide ownership, not ideology

Choose which behavior the project must control and which supporting behavior
it can delegate. Reuse reduces implementation ownership but introduces
integration, upgrade, license, and replacement obligations. Building removes
some dependency constraints while making correctness and maintenance ours.

A component's small install size or large feature list does not decide fit.
Compare the exact required behavior and the cost of the smallest viable
integration. A bounded spike is useful only when its result selects an option.

## Candidate worksheet

Use only fields that distinguish credible options:

```text
Option and evidence source:
Required behavior satisfied / missing:
Behavior and operations we would own:
Initial integration cost:
Upgrade, migration, and replacement cost:
License and distribution duties:
Important lock-in:
```

Include the existing repository pattern when viable. A library, platform
facility, and small local implementation may be different ownership models,
not merely different package names.

## Decision record

State the selected ownership model, distinctive behavior kept in-house,
supporting pieces reused, decisive rejected constraints, ongoing obligations,
replacement seam, and the check that established fit. This can be a short
paragraph; the worksheet is not a mandatory report format.

## Reconsideration

Reopen for an observed missing behavior, repeated integration work, unacceptable
maintenance, or a changed requirement. Do not design a universal adapter merely
because replacement is imaginable; preserve a small natural seam when it is
already useful.
