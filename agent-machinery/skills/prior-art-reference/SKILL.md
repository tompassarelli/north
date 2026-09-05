---
name: prior-art-reference
description: Full notes on prior-art evidence, bounded comparisons, and decision reporting.
---

# Prior art: full notes

## Research must choose an action

Name the protocol, representation, dependency, interface, or infrastructure
decision. State required properties, constraints, the uncertain claim, and
what evidence is enough to decide. Searching without a decision tends to
accumulate context without changing implementation.

## Source selection

Prefer specifications, maintained project docs, exact-revision source,
first-party engineering accounts, and reproducible measurements.
Recency matters for active APIs; operational history matters for mature
infrastructure. Local conventions belong in the candidate set, not above it.

Separate a source's claim from what it demonstrates. A benchmark with a
different workload is a hypothesis for local testing, not the local result.
License checks precede using external expression as an implementation reference.

## Compare credible options

For each serious candidate ask how it meets the requirement, its integration
and operating cost, its important failure or limitation, its exit path, and the
source of those claims. Include the conventional solution before concluding
custom infrastructure is necessary.

A small spike is appropriate for a decisive unknown. Do not build every
candidate, expand into an exhaustive survey, or keep researching after evidence
selects the next action.

## Report and revisit

Lead with the decision, then the local evidence and decisive alternative.
Name reusable components, remaining uncertainty, a disproving check, and source/
license references where relevant. Worksheets are memory aids, not mandatory
sidecars.

Revisit on a changed constraint or contradictory observation. Popularity,
elegance, and novelty alone do not overturn demonstrated fit.
