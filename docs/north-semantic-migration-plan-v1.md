# North semantic migration plan v1 (candidate)

Dependency: product mutation is blocked until `~/code/todo/beagle-store-bclj-source-authority.md`
is published and verified. This plan permits archaeology, contract review, fixtures, and slicing now.

## Phases and gates

0. **Identity/proving evidence.** Add typed Beagle North commands/events for Objective,
Assignment, Attempt joins; forecast/budget; turns/tests; handoff; terminal judgment; delivery and
coverage projections; proving query. Gate: exact joins, deterministic replay, unknown coverage visible.
1. **Semantic cutover.** Explicit kinds/targets for new writes; bounded legacy read projection;
dispatch, leases/recovery, projections, messages/about, streams/adapters, docs/UI migrated in that
order. Gate: active-record worksheet approved; parity and crash/restart evidence; no title-inferred writes.
2. **Todo parity.** Link continuity records to canonical IDs; compare projections; repair coverage;
freeze manual execution telemetry after one proven window. Gate: no systemic missing joins.
3. **Report-only learning.** Projector/analyzer/evaluator/proposal artifacts in one maintenance host;
no mutation. Gate: proving query and coverage remain green over fixed window.
4. **One bounded experiment.** Objective-level randomization, one low-risk axis, fixed horizon,
intent-to-treat, human promotion/rollback. Gate: complete assignment and guardrail evidence.
5. **Reversible promotion.** Only low-risk config thresholds; immutable rollback target and effect receipt.

## Recovery and rollback

Every write is idempotent and fenced by Store generation/sequence. Crash recovery chooses newest
valid checkpoint and replays suffix; a crash after durable force but before reply is reconciled by
idempotency key. North rebuilds projections from committed facts. Migration rollback reopens the
prior verified Store generation and prior North projection code; never mix generations or run dual
writers. Compatibility fallback is removed only after current consumers pass parity and the explicit
removal gate is recorded.

## Immediate next action

Complete the four independent reviews against the contract candidate, reconcile their findings,
then commit this candidate and matrix in the owned North lane. Hold publication and product code
changes until the Beagle source-authority dependency closes and `/root` accepts the reconciled gate.
