---
name: program-stewardship
description: >-
  Bound delivery work, diagnose stalled completion, choose engineering investment, and consolidate issues without losing independently finishable outcomes.
---

# Program stewardship

Use the global engineering-context rules. Default to the shortest useful
artifact and its cheapest discriminating check; eligibility for more assurance
does not create work.

## Keep completion reachable

Before implementation, identify the finite requested outcome, supported scope,
and evidence that would finish this unit of work. Keep this in the existing
task or internally; no new form is required. Separate the product promise from
agent-proposed test ideas. Preserve actual requirements, but do not promote an
advisory target, unmeasured possibility, or desirable coverage into a new gate.

Consolidation merges duplicate ownership, not independent completion predicates.
Distinguish roadmap/epic issues from execution tasks. Preserve independently
finishable outcomes and their evidence in the smallest existing tracking
structure; do not bury a completed fix beneath unrelated platform, hardware,
or whole-project acceptance. Do not create one issue per commit or close an
umbrella to manufacture a better count. A transfer of unfinished scope needs
an explicit destination and remains unfinished.

At meaningful delivery checkpoints, and immediately when the operator reports
prolonged activity without completion, reconcile what satisfies the existing
criteria, what still blocks them, and what is only additional coverage. Do not
wait for another readiness question. Close satisfied work promptly. For unmet work, choose
the next owning repair that advances that same outcome; a newly discovered
concern does not automatically become the next assignment. Keep the complete
project goal intact. Separate agent-completable criteria from human judgment or
unavailable access in the existing task. Finish and deliver the ready checkpoint,
then ask for the specific bounded human action through an available channel.
Keep that acceptance pending; synthetic proxies cannot close it. Continue
independent work without manufacturing substitute tests or more issues.

The failure class is completion starvation: successive useful increments and
ever-wider evidence collection without a terminating deliverable. Counter it
with an execution or closure decision, not another promise, process document,
parallel investigation, or cosmetic redefinition of done.

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

Route structural work to `program-craftsmanship` and operational
guarantees to `production-hardening`. Record consequential deferrals
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
`references/notes.md`.
