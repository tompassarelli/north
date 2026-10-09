---
name: debugging
description: >-
  Diagnose and fix a non-trivial bug, regression, crash, or unexplained behavior by researching the symptom, isolating one variable at a time, and diffing good against bad runs.
---

# Debugging

1. Reproduce the exact symptom beside the closest working case.
2. Search the exact error with product, version and platform before the second fix or the first on an unfamiliar system.
3. List good/bad differences and change one variable per predicted experiment.
4. Trace both runs identically with existing instrumentation and diff normalized output to the first divergence.
5. Repair that cause and run the failing and working cases once each.
6. Stop after two failed fixes with one supported recommendation.

Read [observation tools](references/observation-tools.md) when choosing a trace.
