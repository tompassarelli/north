---
name: verification-distilled
description: >-
  Choose, run, and interpret proportionate checks, builds, tests, reproductions, and performance evidence.
---

# Verification

Name the exact claim and the next action a pass or failure would change.
Use the nearest existing relevant check once; a pass closes that decision.
Keep real gates intact. Extra confidence alone does not justify more work.

For each ticket, separate required gates from advisory measurements and
optional assurance. Do not promote the latter into publication blockers.
When required gates pass, publish and close, or perform the concrete remaining
acceptance step. No extra suite, soak, review or CI wait belongs between that
pass and delivery unless an existing requirement or observed relevant failure
requires it. Do not create follow-up tickets for unmeasured uncertainty during
a delivery push.

Keep the acceptance claim stable while checking it. A verification plan is a
means of testing the requested behavior, not authority to enlarge the promise.
Before adding a case, distinguish a required part of that claim, a relevant
observed counterexample, and optional additional coverage. Only the first two
extend current acceptance work. Scope ordinary expected use honestly; never
drop a failed required case or substitute a narrower product to obtain a pass.

A pass closes its check. For tracked delivery, tick the Done-when box it
satisfies and close the task when every box passes. A pass that ticks no box is
not progress toward closure, so don't run it unless it debugs a failing box.
When the owner asks a general question such as "what can I claim about X?",
build the one aggregate check whose output answers it. Source, integration,
physical-device, cross-platform and broader operating guarantees are distinct
claims, not an automatic ladder every change must climb.

## Run the useful check

- For a usable journey, choose its smallest operator-visible path before
  implementation and run it as soon as safe. Component checks do not prove
  end-to-end behavior. Complete agent-accessible checks before requesting human
  acceptance; subjective feel or unavailable hardware needs the actual human
  or access, not another synthetic proxy.
- When delivery is overdue or the operator repeats a readiness question,
  spend the next verification effort on that usable path or its first observed
  blocker. Do not answer by expanding the evidence inventory. Advisory metrics
  stay advisory; passing the requested bounded check ends verification for
  that claim, even when stronger guarantees remain unproved.
- Hold the exact candidate stable during its acceptance attempt. Parallel work
  may use another artifact or lane. Preserve prior results at their observed
  scope; a new build invalidates only claims its relevant changes can affect.
  Name that causal reach before repeating checks, without adding a new ledger.
  An unrelated merge or new commit ID alone invalidates no passing check.
  Reuse the existing result; run only checks for behavior the change can affect.
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
- Validate the observation that would justify a product change. Missing OCR
  text, a timeout, an inferred API identity, or a helper's successful call is
  not yet the corresponding product failure or success. Resolve that ambiguous
  boundary directly before repairing the product or repeating the full journey.
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

Bad: re-running a passing check a second and third time "to be sure" with no
new code change and no new claim. Good: a pass closes that decision; re-run
only after a relevant change, or when the check itself is suspected flaky — name the
suspicion, don't repeat blindly.

For pricing, evidence selection, or difficult run diagnosis, resolve
`agents path verification-reference` and read only the relevant topic.
