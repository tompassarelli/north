---
name: program-craftsmanship
description: >-
  Refactor, clean up or port established code while preserving observable behavior, and keep module structure within the repo's structure.toml when a code-health gate or edit-time message fires. Also organize project files by real authority, lifecycle and retrieval needs, including data pipelines and handoff layouts.
grounded: 2026-10-15
written: 2026-10-15
---

# Program craftsmanship

1. Identify preserved behavior, callers, interfaces and an existing drift check, because a refactor without one cannot show it preserved anything.
2. Limit changes to mechanical transformations without a credible behavior comparison, since only those carry their own equivalence argument.
3. Resolve friction encountered by current work using existing patterns, because speculative cleanup costs review without paying back.
4. Treat API, stored-format, security, concurrency or deployment changes as design changes, since callers and data outlive the refactor.
5. Write ports in target-language idioms and compare them against kept contract tests and the source build, so the port is checked against the original rather than itself.
6. Keep each module to its one-line responsibility and call other modules only through their public entry and the `may_use` edges in `structure.toml`, because the gate measures against that map.
7. Answer a `code-health` message by splitting, moving the import behind an allowed interface, or changing `structure.toml` in the same change, so the map and the code never disagree.
8. Place files by authority, lifecycle and retrieval, keep native layouts and move every consumer with the files, so a newcomer can find source, inputs, outputs and the rerun command.
9. Check the final diff for scope and behavior drift.

Read [refactor and port details](references/notes.md) when classifying a transformation, and [structure, layout and code-health](references/structure.md) when a gate or hook fires, a module boundary moves, or files, data pipelines or handoff layouts need placing.
