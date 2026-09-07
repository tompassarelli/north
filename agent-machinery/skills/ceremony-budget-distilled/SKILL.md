---
name: ceremony-budget-distilled
description: >-
  Decide how much lifecycle ceremony, hardening, security control, provenance/attestation, or test-isolation rigor is proportionate right now, before adding any of it. Use when tempted toward provenance, attestation, signing, SBOMs, compatibility layers, security paranoia, speculative "while I'm here" fixes, or maximally isolated/immutable test discipline.
---

# Ceremony budget

The default is zero ceremony. Escalate one mechanism at a time, and only when
you can name all four facts for it right now: the actual consumer or
boundary, the plausible failure, the material consequence, and the smallest
mechanism that changes the decision (`agent-machinery:doctrine.md` § Admit
only useful work). A missing fact means no escalation — not "probably fine,"
not "better safe than sorry."

## These do not count as facts

- **A future self or another agent might need this.** Not a consumer until
  one is actually depending on the artifact right now.
- **It looks unfinished, unprofessional, or not-by-the-book without it.**
  Felt correctness is not a failure mode.
- **The repo is public, versioned, or on GitHub.** Visibility is not
  external dependence.
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
  batch tests together (`verification-distilled`) and isolate only the one
  case with an observed mutable-fixture or concurrency hazard.
- Bad: fixing two other things you noticed while implementing the requested
  change, without being asked. Good: name each one in the report and stop;
  let the owner decide whether either is worth a follow-up.

## When escalation is real

A named external consumer with an actual trust boundary, live or durable
state an operator already depends on, or an explicit operator instruction —
escalate exactly that one axis (`production-hardening-distilled`,
`verification-distilled`), never the whole posture. One escalated axis never
raises another.
