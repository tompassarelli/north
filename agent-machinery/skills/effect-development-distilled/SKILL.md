---
name: effect-development-distilled
description: >-
  Design, implement, review, or migrate TypeScript systems using the Effect
  ecosystem. Use for Effect APIs, service composition, typed errors, schemas,
  resources, concurrency, streams, and Effect-specific tooling.
---

# Effect development

Use the Effect version installed by the project. Before relying on a remembered
API or an example from another major version, inspect the repository's vendored
Effect source, `LLMS.md` when present, tests, and existing project usage. Treat
vendored source as read-only reference material unless the task explicitly
changes that vendor. Do not import vendored paths into application code.

Model asynchronous work as Effect programs at the seams where composition
helps: typed failures, cancellation, resource lifetime, concurrency, retries,
or observability. Keep straightforward pure transformations as ordinary
TypeScript. Do not wrap everything in Effect just to make the dependency
visible, and do not build a parallel home-grown service or error framework.

## Compose around the boundary

- Define service requirements where code needs capabilities. Provide concrete
  implementations at the application boundary and compose them there; keep
  domain operations independent of process-wide globals.
- Use `Layer` for shared dependencies and resource construction. Acquire and
  release sockets, files, database sessions, fibers, and other scoped resources
  with Effect's scope-aware APIs so cleanup follows the owning computation.
- Use `Schema` at untrusted boundaries: decode configuration, persisted data,
  network payloads, and external responses once into trusted domain values.
  Keep validation, transformation, and encoding rules next to the boundary.
- Make expected failures part of the effect's error type. Preserve causes and
  context when mapping errors; use defects only for violated internal
  invariants, not routine I/O or user-input failures.
- Use bounded concurrency when parallelizing collections or requests. Choose
  limits from the actual resource or latency constraint, preserve ordering only
  when the consumer requires it, and use interruption-aware racing for
  competing work.
- Retry only failures that can plausibly recover. Bound attempts or elapsed
  time, add an intentional schedule, and avoid retrying validation failures or
  non-idempotent operations without an idempotency mechanism.
- Add logs, metrics, or tracing at meaningful operation boundaries. Carry
  stable operation context and avoid logging secrets or duplicating low-level
  details at every layer.
- Use `Stream` when a source is incremental or needs backpressure. For one-shot
  values, prefer the simpler Effect composition.

## Learn the installed API

Effect evolves quickly. Search its source and tests for the exact installed
version and the closest equivalent use case, then follow local project
conventions. Prefer examples that demonstrate the same runtime, cancellation,
error, and resource semantics. Check types and run the smallest relevant test
before widening validation. When adopting a newer release, consult its upgrade
guidance and exercise affected project boundaries rather than assuming source
compatibility.

Runtime restrictions are project-specific. Do not infer that an API works in
all JavaScript hosts or compilers from its TypeScript types; verify it on the
target runtime. For Warcraft Live / Smashcraft, also follow
`warcraft-typescript-development-distilled` and the repository's Effect vendor
policy, which describe its Lua/TSTL boundary and source refresh cadence.
