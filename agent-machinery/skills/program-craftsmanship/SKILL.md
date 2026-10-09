---
name: program-craftsmanship
description: >-
  Refactor, clean up or port established code while preserving observable behavior; ports are written in the target language's idioms.
grounded: 2026-10-09
written: 2026-10-09
---

# Program craftsmanship

1. Identify preserved behavior, callers, interfaces and an existing drift check.
2. Limit changes to mechanical transformations without a credible behavior comparison.
3. Resolve friction encountered by current work using existing patterns.
4. Treat API, stored-format, security, concurrency or deployment changes as design changes.
5. Write ports in target-language idioms and compare them against kept contract tests and the source build.
6. Check the final diff for scope and behavior drift.

Read [refactor and port details](references/notes.md) when classifying a transformation.
