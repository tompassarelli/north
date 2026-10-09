---
name: competitive-development-loop
description: >-
  Measure and improve a named repeated edit-to-result loop when its latency materially affects iteration speed.
grounded: 2026-10-09
written: 2026-10-09
---

# Development-loop performance

1. Name the repeated edit, result, consumer, use count and latency budget.
2. Measure the end-to-end loop with separate hot, warm incremental, cold/bootstrap and full-gate samples.
3. Compare equivalent workload, hardware, toolchain and cache states against mandatory work, invalidation and a comparable authoritative benchmark.
4. Change the smallest measured cause using existing incremental, cache, selection or reload facilities.
5. Re-measure the same loop against investigation, implementation and maintenance break-even.
6. Schedule recurrence only with a baseline, regression action, owner, consumer and capacity budget.

Read [measurement](references/measurement.md), [optimization](references/optimization.md) or [recurrence](references/recurrence.md) for that decision.
