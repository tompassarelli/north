---
name: effect-development
description: >-
  Design, implement, review, or migrate TypeScript systems using the Effect
  ecosystem. Use for Effect APIs, service composition, typed errors, schemas,
  resources, concurrency, streams, and Effect-specific tooling. Also load it
  before designing any host tool, command or runner in a repository that has
  Effect installed when the tool starts processes, waits, retries, holds a
  resource or parses outside data.
---

# Effect development

1. Inspect installed Effect source, `LLMS.md`, tests and local usage before selecting APIs.
2. Use Effect for typed failures, cancellation, resource lifetimes, concurrency and retries while keeping pure transformations in TypeScript.
3. Compose service requirements and `Layer` implementations at the application boundary.
4. Decode external values with `Schema` and preserve expected failures and their causes in the error type.
5. Use scoped resources, bounded concurrency and recoverable, bounded retries.
6. Verify affected behavior on the target runtime, including Warcraft's vendor and Lua/TSTL rules where applicable.

Read [composition details](references/composition.md) when choosing resource, stream, retry or observability APIs.
