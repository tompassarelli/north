# North Semantic Contract v1 (candidate)

Status: review candidate; not settled doctrine until four independent reviews and the
typed JVM Store source-authority gate close. Source input: `~/north-plan.md` (1694 lines),
recovered rulings in `~/code/todo/north-semantic-model-review.md`, and Store boundary
evidence in `~/code/todo/beagle-store-north-architecture-contract.md`.

## Authority and spine

The JVM Store, authored in typed Beagle and generated into runtime code, owns durable
mechanics: transactions, opaque identity/occurrence mechanics, indexes, commit order,
durability, recovery, integrity, idempotency, and fencing. North owns coordination meaning:
WorkItem, Objective, Attempt, Thread, Actor, Assignment, authorization, judgment, and
projections. The host owns process lifecycle and external effects. Native/Clause paths are
experimental conformance consumers and never gate or enter production North/Store.

The semantic spine is `Referent → immutable Value → Occurrence → StoreCommit → Projection`.
Referent, content, occurrence, Store sequence, and North semantic revision are distinct.
Visibility, persistence confirmation, and automation eligibility remain independent.

## Canonical referents

Exactly five base kinds are normative: `work_item`, `objective`, `attempt`, `thread`,
`actor`. Roles (`project`, `goal`, `incident`, `experiment`, `maintenance`, …) are plural
and evolvable; title and namespace are labels/origin, never type. Plan and Goal are immutable
values (`PlanSpec`, `GoalSpec`) unless independently addressable. Project is a WorkItem role;
Task is the user-facing name for Objective. Plan→Project is adoption of a value, not identity
mutation. Thread is strictly conversation/continuity and may be `about` any referent; it does
not own work or imply completion. Attempt is the existing `runId`, immutable and belonging to
one Objective and Assignment; provider sessions and turns are scoped occurrences.

Current code exposes both a content-addressed `@attempt:<manifest>` wrapper and `runId`/
`execution_attempt_run`; this is an unresolved implementation seam. Phase 0 must choose one
canonical Attempt identity, provide an explicit one-to-one mapping if retaining both temporarily,
and prove multiple successor Attempts under one Objective before any claim of completion.

Create a distinct referent only when independently addressable, owned, authorized, related,
discussed, revised, or recovered. Equal content never merges referents or occurrences.

## Occurrences, authority, and effects

Typed commands validate invariants and atomically emit occurrences. Runtime may assert direct
observations, but only an accepting authority emits Objective judgments. Assignment admission
pins ObjectiveSpec, acceptance contract, actor/delegator, route, policy, forecast, budget, and
optional experiment arm. Every effect is `intent → authorization → attempt → receipt`; missing
receipt is unknown and requires reconciliation, never guessed success/failure.

Statuses (`ready`, `blocked`, `active`, `done`) are projections over facts, not mutable fields.
Unknown/partial evidence is preserved. No join may use title, timestamp proximity, filename,
or filesystem location.

## Phase 0 contract

Implement only exact Objective/Assignment/Attempt joins, separate forecast and budget, turn and
test occurrences with Wire sequence and artifact revision coverage, handoff predecessor/successor,
terminal Objective judgment, objective-delivery and coverage projections, and the proving handoff
query. Do not activate autonomous experiments. The proving query must reconstruct predecessor and
successor wall/tokens/turns/tools/tests/artifacts, final accepted revision, and acceptance result
from Store facts alone.

## Migration and recovery

Forward-only current-main migration. New writes require explicit base kind and canonical targets.
Legacy title fallback is read-only and tagged `legacy_classification_source`; ambiguous history is
`legacy_unclassified` and only active/recovery-relevant records are reviewed. Migrate consumers in
order: dispatch/assignment; attempts/leases/recovery; objective projections; messages/about;
streams/adapters; docs/UI; then remove fallback after parity. No bulk rewrite, dual writer, second
store, broker, scheduler platform, or compatibility forest. Store recovery reopens the newest valid
generation/checkpoint and replays suffix; North replays semantic facts and rebuilds projections.

## Learning and controls

Learning is report-only until coverage and proving query pass. Randomize at Objective level, one
axis per experiment, fixed horizon, intent-to-treat, explicit guardrails, and human promotion;
automatic promotion is limited to reversible low-risk configuration. Maintenance objectives are
excluded from ordinary experiments. Use one bounded maintenance executable, not a daemon swarm.

## Normative reconciliation matrix

| Proposal ruling | Recovered/current fact | Verdict | Reason/material change |
|---|---|---|---|
| Five-part kernel (referent/value/occurrence/commit/projection) | Store contract separates occurrence, Store sequence, North revision; projections rebuildable | Accept/amend | Accept split; name Store sequence and North revision separately. |
| WorkItem→Objective→Attempt spine | Current code overloads thread; runId and terminal projections exist | Accept | Add exact joins; retain runId as Attempt. |
| Goal/Plan/Project/Task four-way model | Operator record left open; current tree has no authoritative typed layer | Amend | Goal/Plan values, Project role, Task=Objective; no mandatory new kinds. |
| Thread as continuity carrier | Current title-bearing thread fallback and thread-targeted consumers | Accept/reject overload | Thread remains conversation only; compatibility read projection is temporary and non-authoritative. |
| Explicit kind and roles | Current code has `entity_kind`/`kind` precedence plus docs denying tags | Amend | Collapse to one canonical stable base-kind predicate; roles separate. |
| Assertions/judgments/effects distinct | Operator ruling requires authorization, attempt, receipt | Accept | Typed occurrences and authority table retained. |
| Compatibility projection for section 19 | Root policy requires current-main-only migration and forbids hypothetical legacy readers | Amend/hold | Prefer atomic current-main cutover; legacy projection requires explicit operator exception, census of real consumers, automation ineligibility, and removal predicates. |
| Clause identifier split | Clause is prior art; JVM Store is production | Accept/amend | Independently define North contract and fixtures; no Native/Clause dependency. |
| Independent learning workers | Proposal decomposes projector/analyzer/etc. | Amend | Functions in one bounded maintenance host; no daemon swarm. |
| Store scheduling mechanics | Store architecture owns leases/claims; North owns readiness meaning | Accept | Keep mechanical schedule in Store, semantic eligibility in North. |

## Executable invariants

1. One stable base kind per referent; title never determines kind.
2. Every Attempt has exactly one Objective and Assignment; Assignment names actor, delegator, route, policy.
3. Tests name Attempt, Wire sequence, suite digest, artifact revision, and coverage.
4. Handoffs name predecessor, successor, reason, and one Objective.
5. Acceptance names contract, artifact revision, supporting oracle, and authority.
6. Experiment assignment precedes provider execution and is evaluated at Objective level.
7. Unknown evidence never becomes false, zero, or failure.
8. Projection rebuild is deterministic; runtime-owned telemetry is not agent-authored.
