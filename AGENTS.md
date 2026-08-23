# North contributor guidance

This file governs changes to the North repository: architecture ownership,
stewardship, code boundaries, and verification. It is not an operator guide and
does not activate North's coordination or orchestration features. Consumer
instructions live in switchboard-controlled sets. Product documentation
under `docs/` is reference material, not agent policy. The coordination graph
is canonical; `threads/` is a projection.

## Runtime boundaries

- Coordination, posture, telemetry, concerns, and supervision belong to North.
- Provider SDK/CLI code belongs only under `sdk/src/providers/`.
- Orchestration owns semantic task routing; provider adapters resolve semantic tiers to models.
- MCP is the shared data/tool boundary for interactive provider sessions.
- Never add a provider model ID to provider-neutral orchestration code.
- Keep consumer behavior out of repository `AGENTS.md` files. Put optional
  runtime guidance in the set or skill that owns the behavior.

## Safe writes and verification

- Assume concurrent agents may be working in the same checkout.
- Preserve unrelated dirty work.
- Run `bun run check && bun run test` from `repo:sdk` in the current worktree
  for SDK changes. The package script owns the hermetic preloads and test
  isolation; do not bypass it.
- The bridge TUI is headless-testable: `createTestRenderer` from
  `@opentui/core/testing` renders the real widget tree and captures snapshots, so
  TUI changes get assertions rather than a screenshot.
- A provider fallback is permitted only before side effects are observable.

## Research projects break forward

- North and sibling personal projects support the current `main` line only.
- No code, data reader, build path, API path, alias, fallback, migration,
  fixture, or test may exist solely for compatibility with an older revision or
  a removed design.
- A breaking change updates every actual in-tree caller in the same change and
  deletes the replaced implementation completely. Do not leave shims,
  tombstones, removal errors, apology comments, or archaeology notes.
- Do not preserve historical source, build, or test snapshots or recovery
  manifests. Git history is the recovery and archaeology mechanism; generated
  artifacts describe only the current source.
- A content-addressed pin may name an active dependency. Once actual consumers
  move, delete the replaced pin and every path that existed only to support it.
