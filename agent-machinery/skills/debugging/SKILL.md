---
name: debugging
description: >-
  Diagnose and fix a non-trivial bug, regression, crash, or unexplained behavior by researching the symptom, isolating one variable at a time, and diffing good against bad runs. Also investigate and close unexplained agent admission, startup, death, liveness, control, or reporting failures at their owning cause.
grounded: 2026-10-15
written: 2026-10-15
---

# Debugging

1. Reproduce the exact symptom beside the closest working case, because the difference between them bounds the cause.
2. Search the exact error with product, version and platform before the second fix or the first on an unfamiliar system, because most failures are already documented.
3. List good/bad differences and change one variable per predicted experiment, because a two-variable change cannot say which one mattered.
4. Trace both runs identically with existing instrumentation and diff normalized output to the first divergence, because the first divergence is closer to the cause than the symptom.
5. Repair that cause and run the failing and working cases once each. When a fix lands, fix every other instance of the same cause in the same change and add a check that fails on the class, because the reported case is rarely the only one.
6. After two failed fixes, write down the assumption both fixes shared and test it before a third fix or a model escalation, because two failures from one premise mean the premise, not the patch, is wrong. If the tested premise does not unblock the fix, stop and return one supported recommendation.

Read [observation tools](references/observation-tools.md) when choosing a trace.

## Agent runtime incidents

An unexplained agent admission, startup, death, liveness, control or reporting failure is an incident with its own lifecycle, separate from delivery of the work, because finishing the work by another route does not repair the fault.

- Capture the intended and emitted operation, exact error, runtime mode and affected seam before any retry or replacement, because a retry can duplicate an external effect or destroy the only evidence.
- Create or join exactly one incident record per stable cause signature in the authorized durable continuity record, excluding credentials and unrelated transcripts, so retries count as recurrences instead of new incidents.
- Quarantine only the defective seam and keep containment, delivery and repair evidence separate, because a successful replacement contains impact without identifying the defect.
- Reproduce one falsifiable hypothesis, classify the lifecycle seam and cause, and assign the exact upstream owner, because a fallback hides the defect instead of owning it. Without a safe reproduction, keep the incident open as `reproduction-blocked` rather than blind-retrying.
- Move through `seeded → contained → investigated → classified → owned → repaired → regressed → activated → primary-canaried → closed` without skipping a state's evidence, because each state's evidence is what the next one relies on.
- On recurrence after closure, reopen with a new generation and gather all evidence again, because a recurrence refutes the earlier repair claim.
- Close only after the repair lands, the focused regression and the coverage required by the [verification skill](../verification/SKILL.md) pass, the repair is activated, the preferred topology is restored and a primary-path canary exercises the triggering seam, all in the current generation, because fallback success shows only that another route works.
- Let the runtime settle runs, fallback and restoration debt, and record its facts as incident evidence, because a second debt model would drift from the runtime's.

Read [runtime incidents](references/runtime-incidents.md) for the signature tuple, classification values, reopening and upstream reports, and [incident cases](references/runtime-incident-cases.md) when unsure whether an observation seeds, joins or splits an incident.
