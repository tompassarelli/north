---
name: program-craftsmanship
description: >-
  Refactor, clean up or port established code while preserving observable behavior, and keep module structure within the repo's structure.toml when a code-health gate or edit-time message fires.
grounded: 2026-10-15
written: 2026-10-15
---

# Program craftsmanship

1. Identify preserved behavior, callers, interfaces and an existing drift check.
2. Limit changes to mechanical transformations without a credible behavior comparison.
3. Resolve friction encountered by current work using existing patterns.
4. Treat API, stored-format, security, concurrency or deployment changes as design changes.
5. Write ports in target-language idioms and compare them against kept contract tests and the source build.
6. Keep each module to its one-line responsibility and call other modules only through their public entry and the `may_use` edges in `structure.toml`.
7. Answer a `code-health` message by splitting, moving the import behind an allowed interface, or changing `structure.toml` in the same change.
8. Check the final diff for scope and behavior drift.

Read [refactor and port details](references/notes.md) when classifying a transformation, and [structure and code-health](references/structure.md) when a gate or hook fires or a module boundary moves.
