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

For posture fields and deferral examples, use
`agents path program-stewardship-reference`.
