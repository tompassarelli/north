# Effect composition

- Keep installed vendored Effect source read-only unless changing the vendor is requested.
- Import public package APIs rather than vendored source paths.
- Provide concrete services at the application boundary without process-wide domain globals.
- Acquire and release sockets, files, database sessions and fibers with scope-aware APIs.
- Keep validation, transformation and encoding beside each external boundary.
- Reserve defects for violated internal invariants rather than routine I/O or input failures.
- Choose concurrency limits from the actual resource or latency constraint.
- Preserve collection ordering only when the consumer needs it.
- Use interruption-aware racing for competing work.
- Retry recoverable failures with an intentional schedule bounded by attempts or elapsed time.
- Require idempotence before retrying effects that could repeat harmful actions.
- Add logs, metrics or tracing at meaningful operation boundaries with stable operation context.
- Use `Stream` for incremental sources or backpressure and ordinary Effect composition for one-shot values.
- Consult upgrade guidance and exercise affected project boundaries when adopting a new release.
- Verify runtime support independently of TypeScript type support.
