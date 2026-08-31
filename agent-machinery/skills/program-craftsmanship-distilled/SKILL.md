---
name: program-craftsmanship-distilled
description: >-
  Default distilled workflow for improving a bounded area of established code
  while preserving observable behavior. Use for explicit cleanup or refactoring
  of semantics, ownership, naming, structure, errors, tests, or maintainability.
---

# Program craftsmanship

Bound behavior, callers, authority/projections, and public/durable surfaces.
Read governing policy/history and choose existing `verification-distilled`; without a
credible comparison, make only mechanical changes or stop.

Fix current friction minimally, with coverage for non-mechanical reshaping.
Separate unrelated fixes; reject speculative generality. Never call API, stored
data, security, concurrency, compatibility, or deployment changes cleanup—use
`planning-distilled` or `production-hardening-distilled`.

Edit authority, never generated/vendor projections; regenerate normally. Check
coherent groups and final diff for drift/scope; remove unsupported changes.
Record debt only in an existing mechanism with a reopening event. Stop when
resolved, evidence fails, or taste begins.

For friction heuristics and refactor classification, run
`agents path program-craftsmanship-reference` and read its `SKILL.md` completely.
