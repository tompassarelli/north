---
name: program-craftsmanship-distilled
description: >-
  Refactor or clean up established code while preserving observable behavior and reducing current maintenance friction.
---

# Program craftsmanship

Identify the behavior, callers, authoritative source, and relevant interfaces.
Choose an existing check that can detect drift. Without a credible comparison,
limit changes to demonstrably mechanical transformations.

Fix friction encountered by current work. Prefer existing patterns and avoid
speculative abstraction. Non-mechanical restructuring needs behavior coverage.
An API, stored-format, security, concurrency, or deployment change is a design
change; route it through the applicable planning or hardening workflow.

Edit source and regenerate projections normally. Check coherent batches and
the final diff for scope and behavior drift. Record consequential deferrals in
the existing mechanism with a reopening condition.

Stop when the named friction is resolved. For useful friction patterns and
refactor classification, use `agents path program-craftsmanship-reference`.
