---
name: program-stewardship-distilled
description: >-
  Choose a justified level of engineering investment, record consequential deferrals, or route a refactoring or hardening pass.
---

# Program stewardship

Use the global engineering-context rules. Default to the shortest useful
artifact and its cheapest discriminating check; eligibility for more assurance
does not create work.

Budget each axis separately:

- Changeability: reduce the cost of this or the clearly next change.
- Correctness: falsify the requested claim.
- Robustness: handle expected use and observed failures.
- Security: protect actual assets and trust boundaries.
- Operations: support actual live state, external dependence, or an explicit
  operational requirement.

Extra investment needs a named consumer or boundary, plausible failure,
material consequence, and a mechanism that changes the decision. Raising one
axis does not raise the others. Preserve existing promises unless their change
is authorized.

Route structural work to `program-craftsmanship-distilled` and operational
guarantees to `production-hardening-distilled`. Record consequential deferrals
in the existing mechanism with a reason and reopening event. Create no review
program or ledger without a separate need.

## Worked contrasts, one per axis

- Changeability — bad: a plugin/strategy abstraction for a single current
  call site "in case it varies later." Good: write the concrete version;
  abstract only when a second real caller needs to vary it.
- Correctness — bad: five additional edge-case tests for a claim nobody
  disputed. Good: one test that could actually fail if the claim were false.
- Robustness — bad: retry/backoff/circuit-breaker logic around a call that
  has never failed and has no plausible named failure mode. Good: handle the
  failures you've actually seen or can name a specific cause for.
- Security — bad: adding auth/rate-limiting to an internal, owner-only tool
  because "you should always." Good: protect the boundary an untrusted or
  external actor can actually reach.
- Operations — bad: a rollback/migration path for a one-off local script.
  Good: operational investment tracks actual live state or an explicit
  operational requirement, not the mere existence of a repo.

For posture fields and deferral examples, use
`agents path program-stewardship-reference`.
