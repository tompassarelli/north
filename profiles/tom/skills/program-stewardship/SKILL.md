---
name: program-stewardship
description: >-
  Select or revisit the engineering quality posture for a bounded workstream.
  Use when the operator asks to set a craft budget or lifecycle quality floor,
  record a meaningful deferral, or route an explicit craftsmanship or
  production-hardening pass; do not use for routine feature work.
---

# Program stewardship

Deliver quickly without allowing accidental structure to become durable
authority. Separate the authoring posture used during ordinary work from an
explicit maintenance pass.

## Classify only the bounded scope

Use the smallest subset of these fields that changes a decision:

```yaml
scope:
objective:
problem_regime: exploration | product | mature | research
craft_budget: permissive | balanced | intensive
expected_lifetime: disposable | provisional | durable
protected_surfaces: []
required_evidence: []
allowed_debt: []
forbidden_debt: []
```

Never assign one posture to a whole repository by default. A spike optimizes
for learning and needs a disposable boundary or promotion gate. Ordinary build
work ships quickly while keeping touched code habitable. Intensive stewardship
is a separately requested pass for valuable or load-bearing scopes. Research
may move quickly while still requiring conceptual precision.

## Preserve the quality floor

Every posture preserves:

- buildability or type correctness and the relevant behavioral proof;
- existing data, durability, security, and compatibility guarantees unless the
  change explicitly owns them;
- one clear authority for each durable fact, transition, and effect;
- coherent public names, persisted concepts, effect boundaries, and error
  semantics; and
- no speculative framework introduced merely to appear well designed.

Apply a ratchet to touched code: inherited debt may remain, but a change must
not silently make its relevant quality dimension worse.

During ordinary work, clean up directly touched code only when the improvement
is obvious, local, and nearly free. Tolerate reversible private duplication,
provisional private abstractions, imperfect file layout, and incomplete
secondary-path polish. Do not broaden feature work into an aesthetic rewrite or
create a shared abstraction before a stable concept and change pattern exist.

Record a deferral only when it may become load-bearing, and give it a reason
plus a concrete reconsideration trigger such as a repeated coordinated edit,
second independent implementation, promotion to a public or persisted surface,
incident, measured friction, or failed invariant. Use the repository's existing
debt or continuity mechanism; do not invent a central stewardship ledger or one
sidecar per file.

## Route explicit maintenance

Use `program-craftsmanship` for a bounded behavior-preserving improvement to
vocabulary, authority, abstractions, modularity, file structure, APIs, errors,
tests, or local code shape.

Use `production-hardening` for a bounded guarantee about failure, load, attack,
concurrency, cancellation, upgrade, migration, persistence, or operation.
Hardening may intentionally change behavior and requires stronger evidence.

Never hide public-contract, persistence, compatibility, security, concurrency,
or operational changes inside cleanup. Use `planning`, `repo-safety`, and
`verification` for their owned decision, write, and evidence procedures.

## Avoid autonomous review machinery

Review freshness depends on relevant content, dependency context, policy, and
reviewed dimensions—not elapsed time alone. Consult an existing repository
record when it exists; otherwise current source, tests, diff, and commit history
are sufficient inputs.

Do not create a nightly or weekly selector, recurring monitor, automatic pass,
or quality sidecar merely because this skill describes stewardship. Scheduled
monitoring or recurring execution requires explicit operator authorization and
uses the harness's existing monitoring and continuity mechanisms.

Stop when the next change is speculative, crosses a protected surface, lacks
evidence, or offers diminishing return.
