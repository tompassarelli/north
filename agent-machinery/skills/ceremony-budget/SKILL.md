---
name: ceremony-budget
description: >-
  Decide how much hardening, security, signing, compatibility or test isolation a change needs before adding any, and harden one named guarantee when it does. Use when tempted to add process, safeguards or "while I'm here" fixes.
---

# Ceremony budget

The default is zero ceremony.

## Start from the profile

The bootstrap's profile table (prototype, tooling or client) sets the
baseline. It's a lookup, not a judgment. On prototype, the answer to "should I
add this?" is no. On tooling, add only what keeps the machine working
tomorrow. On client, follow that repo's rules. When Tom asks to ship or names
a deadline, keep only the work needed for the usable result.

Escalate one mechanism only when you can name the consumer who depends on it
now, the failure it closes and what that failure costs. A missing fact means
no escalation. One escalated axis never raises another. Feeling uneasy
mid-task is not a new fact.

## These do not count as facts

- **A future self or another agent might need this.** Not a consumer until
  one is actually depending on the artifact right now.
- **It looks unfinished, unprofessional, or not-by-the-book without it.**
  Felt correctness is not a failure mode.
- **The repo is public, versioned, or on GitHub.** Visibility is not
  external dependence.
- **The surrounding code is polished, or is a mess.** Matching the local
  ceremony level is mimicry, not a reason. Existing mess is not a cleanup
  mandate; existing polish is not a standard your diff must match.
- **It would make things more reproducible, auditable, or verifiable in the
  abstract.** Wanting a property is not the same as a consumer who breaks
  without it.
- **You noticed a gap while working on something else.** A gap that doesn't
  block the requested artifact is a deferred note, not a task. Fixing it now
  is scope creep even when the fix is easy and even when it later turns out
  to matter.

## Worked contrasts

- Bad: adding an SBOM, signing, or attestation step to a research CLI
  because it's public. Good: adding one content hash once a real automated
  downstream consumer pulls the artifact unauthenticated and you can name
  the tampering scenario it closes.
- Bad: hardening error handling, retries, and input validation across a
  one-shot local script because "production code should do this." Good:
  hardening only the exact boundary a named caller actually crosses
  untrusted or unreliable input across.
- Bad: rewriting a flaky test into full per-case isolation with a fresh
  fixture each time because that is the by-the-book way to test. Good:
  batch tests together (`verification`) and isolate only the one
  case with an observed mutable-fixture or concurrency hazard.
- Bad: fixing two other things you noticed while implementing the requested
  change, without being asked. Good: name each one in the report and stop;
  let the owner decide whether either is worth a follow-up.

## Hardening one named guarantee

When a named fact does call for hardening, harden the one boundary whose
failure brought you here and leave its neighbours alone. State the guarantee,
the degradation you allow and the state that must survive; trace only the
resources, retries, cancellation, persistence and diagnostics that can break
it. Repair the weakest boundary, bound retries, and add idempotence where a
repeated effect would do harm. Exercise the named failure with
`verification`; a happy path shows nothing about resilience. Report the
guarantee you observed. This grants no deployment, credential, production-write
or communication authority. Read `references/hardening.md` when writing the
guarantee down or choosing which failure paths to test.

This skill lowers unrequested assurance. It never lowers a real gate, weakens
a test to make it pass, or excuses a known defect.

Read [references/rationale.md](references/rationale.md) only when revising this skill or questioning one of its rules.
