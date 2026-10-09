---
name: effect-development
description: >-
  Design, implement, review, or migrate TypeScript systems using the Effect
  ecosystem. Use for Effect APIs, service composition, typed errors, schemas,
  resources, concurrency, streams, and Effect-specific tooling. Also load it
  before designing any host tool, command or runner in a repository that has
  Effect installed when the tool starts processes, waits, retries, holds a
  resource or parses outside data.
grounded: 2026-10-09
written: 2026-10-09
---

# Effect development

1. When adding Effect to a project, run `effect-kit init` and keep `effect-kit check` in its CI; it vendors upstream source at the installed version, fails on version drift and Effect diagnostics, and enforces the host-tool rule.
2. Inspect installed Effect source, `LLMS.md`, tests and local usage before selecting APIs.
3. Use Effect for typed failures, cancellation, resource lifetimes, concurrency and retries while keeping pure transformations in TypeScript.
4. Compose service requirements and `Layer` implementations at the application boundary.
5. Decode external values with `Schema` and preserve expected failures and their causes in the error type.
6. Use scoped resources, bounded concurrency and recoverable, bounded retries.
7. Verify affected behavior on the target runtime, including Warcraft's vendor and Lua/TSTL rules where applicable.

Read [composition details](references/composition.md) when choosing resource, stream, retry or observability APIs.
