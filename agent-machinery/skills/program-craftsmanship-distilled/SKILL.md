---
name: program-craftsmanship-distilled
description: >-
  Refactor, clean up or port established code while preserving observable behavior; ports are written in the target language's idioms.
---

# Program craftsmanship

Identify the behavior, callers, authoritative source, and relevant interfaces.
Choose an existing check that can detect drift. Without a credible comparison,
limit changes to demonstrably mechanical transformations.

Fix friction encountered by current work. Prefer existing patterns and avoid
speculative abstraction. Non-mechanical restructuring needs behavior coverage.
An API, stored-format, security, concurrency, or deployment change is a design
change; route it through the applicable planning or hardening workflow.

Edit source and regenerate projections normally. Check coherent batches and
the final diff for scope and behavior drift. Record consequential deferrals in
the existing mechanism with a reopening condition.

Stop when the named friction is resolved.

When the requested outcome is a port or rewrite into another language, the
whole ported unit is in scope and its behavior, not its structure, is what you
preserve. Write the target language's idioms: model data with its type system,
make invalid states unrepresentable, and drop ceremony that only served the
source language. Keep a source-era shape only where the target runtime demands
it, and name that constraint where it is kept. Prove equivalence with an oracle
the port cannot game: the kept tests plus a differential check against the
source build. Port each test only for a contract worth keeping.

Bad: renaming unrelated variables, extracting a "cleaner" helper, or
reorganizing adjacent files while fixing one bug, because you noticed them
along the way. Good: fix the friction actually blocking current work; name
anything else noticed and leave it, don't fold it into the same diff.

Bad: a port that keeps Java-style classes with getters, positional
`assign(a, b, c, d, e, f, g)` setters and sentinel `-1` values because the
source had them. Good: typed records, unions and functions over data in the
target's style, with a comment only where the runtime forces another shape.

For useful friction patterns and refactor classification, use
`agents path program-craftsmanship-reference`.
