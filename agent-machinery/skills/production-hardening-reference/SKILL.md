---
name: production-hardening-reference
description: Full notes on operational guarantees, failure selection, and bounded hardening evidence.
---

# Hardening: full notes

## Define the guarantee before the mechanism

Hardening is work for a specific operational promise under stated conditions.
Separate guarantees already provided from guarantees requested. Identify the
consumer, plausible failure, consequence, and smallest mechanism that changes
the decision; do not raise unrelated quality axes.

## Assurance envelope

Use the needed fields, not a mandatory form:

```text
System, consumer, and scenario:
Guarantee sought and allowed degradation:
Hostile inputs or actors in scope:
Load, timing, and concurrency assumptions:
State that must survive:
Recovery or rollback requirement:
Evidence that decides the claim:
```

A claim without workload or failure assumptions can be impossible to falsify.
Narrow it until a real check could show it wrong.

## Select failure paths

Consider only paths bearing on that guarantee: authorization and trust;
limits, queues, backpressure, timeout/retry amplification; races, ordering,
duplicates, partial completion and cancellation; atomicity, corruption,
restore, migration and version skew; representative capacity/latency;
operator detection and the corresponding recovery action.

These are prompts, not a coverage quota. For example, duplicate payment
delivery needs an idempotency boundary; a local parser change does not acquire
a disaster-recovery workstream because this list mentions restore.

## Evidence and completion

Put a durable guarantee in the existing contract or test surface. Exercise the
named failure under representative conditions and report the observed bound,
not “production ready” without qualification. Fix the owning cause; never lower
a gate to make the result pass.

A one-off review needs no new attestation system. Broader load, adversarial,
migration, or recovery testing enters only when another exact promise needs it.
