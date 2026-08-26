# North semantic migration plan v1 (candidate)

Dependency: product mutation is blocked until `~/code/todo/beagle-store-bclj-source-authority.md`
is published and verified. This plan permits archaeology, contract review, fixtures, and slicing now.

## Phases and gates

0. **Identity/proving evidence.** Add typed Beagle North commands/events for Objective,
Assignment, Attempt joins; forecast/budget; turns/tests; handoff; terminal judgment; delivery and
coverage projections; proving query. Gate: exact joins, deterministic replay, unknown coverage visible.
1. **Semantic cutover.** Preflight census and fixtures; shadow projection parity; writer-fenced
atomic current-main cutover with all new writers explicit and no dual writes; dispatch, leases/recovery,
projections, messages/about, streams/adapters, docs/UI migrated in that order. A legacy read projection
is permitted only by an explicit operator exception for enumerated live consumers, tags inferred rows,
and makes them ineligible for automation. Gate: active-record worksheet, crash/restart evidence, and
exact removal predicates (`census=0` plus source/test search).
2. **Todo parity.** Link continuity records to canonical IDs; compare projections; repair coverage;
freeze manual execution telemetry after one proven window. Gate: no systemic missing joins.
3. **Report-only learning.** Projector/analyzer/evaluator/proposal artifacts in one maintenance host;
no mutation. Gate: proving query and coverage remain green over fixed window.
4. **One bounded experiment.** Objective-level randomization, one low-risk axis, fixed horizon,
intent-to-treat, human promotion/rollback. Gate: complete assignment and guardrail evidence.
5. **Reversible promotion.** Only low-risk config thresholds; immutable rollback target and effect receipt.

## Recovery and rollback

Every write is idempotent and fenced by Store generation/sequence. Crash recovery chooses the newest
valid checkpoint within the explicitly selected generation and replays its suffix; a crash after
durable force but before reply is reconciled by idempotency key. North rebuilds projections from
committed facts. Before the first canonical write, code deployment may roll back. After that
point-of-no-return, freeze admission on failure and roll forward on the same generation; never rewind
Store state or run dual writers. Projection rollback is allowed only when the prior projector is
proven to understand all facts. Policy rollback is a new authorized activation of an immutable prior
value with an effect receipt. Generation replacement requires fenced CAS and a new generation receipt.
Compatibility fallback is removed only after census=0 and source/test search proves no bypass.

## Immediate next action

Complete the four independent reviews against the contract candidate, reconcile their findings,
then commit this candidate and matrix in the owned North lane. Hold publication and product code
changes until the Beagle source-authority dependency closes and `/root` accepts the reconciled gate.
