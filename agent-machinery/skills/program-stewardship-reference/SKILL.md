---
name: program-stewardship-reference
description: Full notes on independent quality budgets, deliberate deferral, and reopening triggers.
---

# Stewardship: full notes

## Completion starvation

This failure is different from doing no useful work. An agent can deliver many
real fixes while continually moving to the next untested circumstance, so none
of its execution tasks reaches a terminal state. Broad issues make the failure
harder to see: one issue may combine a code repair, hardware measurements,
platform support and whole-product acceptance. Consolidating duplicates is
useful; collapsing independent completion predicates merely conceals progress
and multiplies the dependencies of every closure.

Observed in a platform-fighter project on 5 October 2026: eight hours of work
produced input retention, lifecycle and playable-build improvements while all
ten consolidated issues remained open. Several were roadmap-sized buckets.
The earlier policy already required proportionate verification and an early
playable checkpoint; repeating those slogans did not prevent the agent from
treating each newly uncovered case as its next assignment. This correction
therefore acts when defining work, consolidating it and selecting the next
action, as well as when deciding whether a passing check is sufficient.

Matching case: a controller-reconnect fix passes its required device-identity,
neutral-rearm and fresh-input journey. Record that outcome as complete in the
existing task structure. Other platform support retains its own unfinished
scope; it does not silently expand the reconnect fix. If the original request
explicitly required reconnect on those platforms, that requirement still holds.

Nonmatching case: the requested deliverable is a cross-platform input guarantee,
and only a Linux simulation has passed. Keep the broader claim open and state
which required observations are missing. An eight-hour deadline does not make
the missing observations true. Similarly, an observed dropped input on the
supported path remains a blocking defect, even if many other cases pass.

Preserve the complete project scope and existing real gates. Do not manufacture
tiny historical tickets, mark consolidation as engineering completion, transfer
requirements without a destination, or relabel a partial result as the original
promise. Use the minimum tracking needed to distinguish finished outcomes from
remaining work. No fixed test-count, elapsed-time closure rule, new audit role,
mandatory status document or automatic closure hook follows from this lesson.

## Quality is several decisions

Changeability, claim correctness, robustness, security, and operational
assurance have different costs and triggers. Strong investment in one does not
raise the others. A clean internal model can coexist with deliberately narrow
edge-case coverage; a security boundary can require strict validation without
requiring a public-release program.

Resolve the posture from actual consumers, state, break tolerance, and exposure.
Missing facts default to owner-controlled research, not worst-case production.

## Useful posture questions

What is the current purpose and expected lifetime? What does failure or change
cost? Which interfaces or state are already durable? What evidence establishes
the claim? Which cleanup pays for the current or next change? Which debt is
acceptable, and which violates an existing boundary?

These answers can stay internal unless a decision or handoff requires them.
Do not create a profile artifact simply to authorize ordinary work.

## Deliberate deferral

Private duplication and provisional structure can be cheaper than guessing a
future abstraction. Defer a nonblocking concern with its consequence and a
specific reopening event; preserve a concrete defect's reproduction where
available. Deferral is not a claim that the defect is fixed.

Useful events include a second implementation, repeated coordinated edits,
promotion to a persisted/public boundary, an incident, or measured maintenance
cost. “Revisit later” lacks an executable trigger.

## Routing the next pass

Use craftsmanship for demonstrated maintenance friction while preserving
behavior. Use hardening for a named operational guarantee under failure or load.
Neither is a finishing ritual for every feature. End the current work once its
requested artifact and bounded correctness check are complete.
