---
name: effect-development
description: >-
  Design, implement, review, or migrate TypeScript systems using the Effect
  ecosystem. Use for Effect APIs, service composition, typed errors, schemas,
  resources, concurrency, streams, and Effect-specific tooling. Also load it
  before designing any host tool, command or runner in a repository that has
  Effect installed when the tool starts processes, waits, retries, holds a
  resource or parses outside data.
grounded: 2026-10-15
written: 2026-10-15
---

# Effect development

1. When adding Effect to a project, run `effect-kit init` and keep `effect-kit check` in its CI, because it vendors upstream source at the installed version and fails on version drift, Effect diagnostics and host-tool rule violations.
2. Inspect installed Effect source, `LLMS.md`, tests and local usage before selecting APIs, because the installed version, not memory, defines what exists.
3. Use Effect for typed failures, cancellation, resource lifetimes, concurrency and retries while keeping pure transformations in TypeScript, so Effect is paid for only where it adds guarantees.
4. Compose service requirements and `Layer` implementations at the application boundary, so inner code stays swappable and testable.
5. Decode external values with `Schema` and preserve expected failures and their causes in the error type, so callers can handle failures without losing the original cause.
6. Use scoped resources, bounded concurrency and recoverable, bounded retries, because unbounded work leaks resources and unlimited retries hide outages.
7. Verify affected behavior on the target runtime, including Warcraft's vendor and Lua/TSTL rules where applicable, because type support does not prove runtime support.

Read [composition details](references/composition.md) when choosing resource, stream, retry or observability APIs.
