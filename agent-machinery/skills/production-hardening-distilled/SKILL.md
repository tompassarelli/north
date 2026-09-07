---
name: production-hardening-distilled
description: >-
  Establish a named operational guarantee under relevant failure, load, attack, concurrency, persistence, or deployment conditions.
---

# Production hardening

Start with a concrete consumer, exposed boundary, failure mode, and consequence.
A public repository, daemon, or durable local file alone does not justify a
hardening pass. Apply only the lifecycle work admitted by global policy.

Define one guarantee, allowed degradation, assumptions, state to preserve,
recovery requirement, and deciding evidence. Trace only the resources,
effects, cancellation, persistence, and diagnostics that can break it.

Repair the weakest owning boundary. Bound retries and use idempotence or
deduplication where repeated effects require them. Preserve existing recovery
and safe missing-configuration behavior. Route consequential contract changes
through `planning-distilled`; this skill grants no deployment, credential,
production-write, or communication authority.

Use `verification-distilled` to exercise the named failure; a happy path does
not establish resilience. Report the observed guarantee and its limits.

Bad: arriving here because a change "touches production-ish code," then
hardening every boundary in the file. Good: harden the one boundary whose
named failure mode brought you here and leave its neighbours alone — reaching
this skill escalates one axis at one seam, never the posture of the
surrounding code (`ceremony-budget-distilled`).

For scenario fields and failure categories, use
`agents path production-hardening-reference`.
