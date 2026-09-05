---
name: competitive-development-loop-distilled
description: >-
  Measure and improve a named repeated edit-to-result loop when its latency materially affects iteration speed.
---

# Development-loop performance

Use this for a repeated loop with a named edit, result, consumer, expected use
count, and latency budget. One-off check selection belongs to
`verification-distilled`.

1. Measure the real end-to-end loop and retain individual samples. Label hot,
   warm incremental, cold/bootstrap, and full-gate runs separately.
2. Compare equivalent workload, hardware, toolchain, and cache states. Decompose
   phases only when that measurement changes the next optimization.
3. Identify mandatory work, invalidation fan-out, and the critical path.
   Compare a relevant authoritative benchmark when one is genuinely comparable;
   report a gap otherwise.
4. Change the smallest measured cause. Prefer existing incremental compilation,
   caches, selection, and reload facilities. Split files or crates only when
   invalidation savings exceed added parse/link/scheduling and maintenance cost.
5. Re-measure the same loop. Expected remaining uses times credible savings
   must repay investigation, implementation, and maintenance.

Never weaken correctness checks for speed. An overrun stops blind reruns, not
useful work already progressing; diagnose the changed phase before retrying.
Schedule recurring measurements only for a named baseline, regression action,
owner, consumer, and bounded capacity budget.

Full notes: [measurement and comparison](references/measurement.md),
[optimization economics](references/optimization.md), and
[regressions and recurring measurements](references/recurrence.md).
