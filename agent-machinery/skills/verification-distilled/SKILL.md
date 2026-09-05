---
name: verification-distilled
description: >-
  Choose, run, and interpret proportionate checks, builds, tests, reproductions, and performance evidence.
---

# Verification

Name the exact claim and the next action a pass or failure would change.
Use the nearest existing relevant check once; a pass closes that decision.
Keep real gates intact. Extra confidence alone does not justify more work.

## Run the useful check

- For a usable journey, choose its smallest operator-visible path before
  implementation and run it as soon as safe. Component checks do not prove
  end-to-end behavior. Do not make the operator its first tester.
- Preserve verdict-sensitive launcher, directory, environment, executable,
  TTY, and fixture state. A broken driver is diagnostic, not a product failure.
- Before a development-loop command, estimate duration and whether reducing
  its cost pays over remaining uses. Keep this internal for ordinary checks.
- Use the lowest layer that decides the claim. Add broader assurance only for
  an explicit request or a concrete exposed boundary; unknown exposure does
  not imply production.
- Inspect the first observable divergence. Fix its owning cause and other
  occurrences on the delivery path before another expensive run. Do not
  concurrently test against one mutable fixture.
- After two failures before the advertised boundary, stop repeating that
  attempt: identify the earliest unproven boundary and change the diagnostic
  or repair strategy. Preserve the failure evidence; never retry into proof.

## Preserve useful progress

At an unexpected delay, inspect the existing run's phase and progress.
For downloads, use remaining volume and measured throughput when available.
Silence or elapsed time alone does not prove a stall.

An estimate or checkpoint is not a kill timer. Reassess at roughly twice the
estimate; continue safe progress. Restart only for an observed failure or a
supported corrective change that justifies losing in-flight work. Actual
resource limits and explicit cancellation remain binding.

One owner supervises and reaps each run. Report the observed result and
unobserved dimensions, then stop checking. A missing harness capability does
not authorize unrelated infrastructure work.

For pricing, evidence selection, or difficult run diagnosis, resolve
`agents path verification-reference` and read only the relevant topic.
