---
name: production-hardening
description: >-
  Establish bounded operational guarantees. Use when the operator requests
  hardening, production readiness, or an assurance review involving failure,
  load, attack, concurrency, cancellation, untrusted input, persistence,
  migration, upgrade, release, or operation; do not use for routine testing or
  cosmetic cleanup.
---

# Production hardening

Improve guarantees under realistic and adversarial operating conditions.
Hardening may intentionally change behavior, so define the intended guarantee
before changing code.

## Establish the bounded contract

Record only the fields relevant to the decision:

```yaml
objective:
current_guarantees: []
required_guarantees: []
protected_surfaces: []
threat_and_failure_assumptions: []
representative_workloads: []
rollout_and_rollback_constraints: []
required_evidence: []
```

When evidence is unavailable, state conservative assumptions and residual
uncertainty. Never imply an untested guarantee.

## Inspect relevant failure surfaces

Select from these dimensions rather than turning all of them into a mandatory
campaign:

1. Failure detection, propagation, containment, recovery, and partial progress.
2. Resource ownership, cleanup, limits, backpressure, and exhaustion.
3. Trust boundaries, authentication, authorization, validation, secrets, and
   unsafe input.
4. Concurrency, ordering, cancellation, races, deadlocks, and duplicate work.
5. Timeouts, retries, idempotence, deduplication, and retry amplification.
6. Persistence, atomicity, durability, corruption, migration, restore, and
   rollback.
7. Compatibility, version skew, upgrade, downgrade, and feature gates for real
   consumers.
8. Diagnostics and observability needed for an operator to act under failure.
9. Representative performance, capacity, latency tails, and degradation.
10. Deployment controls, kill switches, safe defaults, and adversarial cases.

## Change guarantees deliberately

- Act on local reversible hardening only when the guarantee and validation are
  unambiguous.
- Use `planning` for public contracts, persistence, compatibility, security,
  concurrency, rollout, or cross-service behavior.
- Keep hardening separate from semantic cleanup and feature work.
- Prefer the smallest mechanism that establishes the guarantee and preserves a
  real recovery path.
- Never add retries without timeout, idempotence, backoff, and amplification
  analysis.
- Never claim resilience from happy-path evidence alone.

This skill does not authorize credentials access or disclosure, deployment,
rollout, production mutation, external coordination, or new outside
commitments. Obtain the authority required by the current task and governing
repository. Route new deterministic enforcement through `guard-authoring`;
for Beagle source use `beagle-authoring`.

## Prove only the requested guarantee

Use `verification` to select the lowest deterministic evidence layer that can
decide the claim. Depending on the named guarantee, evidence may include a
negative or adversarial case, cancellation or ordering case, migration or
restore rehearsal, matched representative measurement, failure diagnostics, or
one focused system journey. Do not construct every form, silently broaden the
assurance profile, or repeat a failed/flaky check into proof.

Put established guarantees in their authoritative contract, invariant, or
test. Otherwise report the bounded subject, evidence, assumptions, rollback,
and residual risk directly; do not create a bespoke attestation ledger by
default.

An incident does not by itself authorize a durable policy mutation. Route a
reusable normative rule through the harness's feedback-to-policy owner only
when explicit, repeated, or strongly emphasized operator feedback supplies it.

Stop when the requested guarantees are evidenced, remaining risk is explicit,
and further change requires a larger design or authority decision.
