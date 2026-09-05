---
name: rust-development-reference
description: Full Rust notes on API design, errors, cancellation, unsafe boundaries, and build economics.
---

# Rust development: full notes

## API and control-flow choices

Prefer borrowed `&str`, `&[T]`, and `&Path` internally; widen bounds for real
callers, not hypothetical reuse. Enums/newtypes make distinct states explicit.
Use `From` only for infallible, lossless, value-preserving, obvious conversion;
use `TryFrom` for fallible conversion. Derive only traits whose laws hold.

Choose syntax for the decision: `?` propagates errors; `let ... else` binds a
required value with early exit; `if let` handles one interesting case;
`match` exposes alternatives and exhaustiveness. Use loops for mutation,
state, early exit, or difficult borrowing; iterator adapters for clear data
transformations. Do not collect merely to iterate again.

```rust
let Some(path) = request.path() else {
    return Ok(Response::not_found());
};
let records = lines.map(parse_record)
    .collect::<Result<Vec<_>, ParseError>>()?;
```

These fragments illustrate shape; surrounding types belong to the consumer.

## Errors, cancellation, and unsafe

Follow the repository's error stack. Reusable boundaries commonly expose typed
errors; application coordination may use contextual erased errors. Do not add
`thiserror` or `anyhow` reflexively. Use `expect` only in tests or for a locally
proved invariant, explaining why failure is impossible. Explain intentional
best-effort suppression.

Use the selected runtime's blocking adapter for blocking operations. Across an
await, determine what state survives cancellation and who owns cleanup. A
future being dropped is not proof that an external effect was cancelled.

Keep unsafe blocks small. A `// SAFETY:` comment explains the local proof;
a `# Safety` API section states caller obligations. Neither replaces enforcing
the invariants that safe code can enforce.

## Build economics

Compare equivalent package/workspace scope, target, features, profile, flags,
wrappers, output directory, cache state, and wall time. Keep hot-loop inputs
stable and vary one reversible input. Cargo timings or compiler output should
identify the cost before changing architecture.

Modules divide reasoning; crates divide compilation/cache boundaries. Split
crates only when repeated comparable evidence identifies a worthwhile
invalidation edge. Custom profiles, RUSTFLAGS, codegen, wrappers, and caches are
measured opt-ins, not universal settings. Preserve lane-local output ownership.

## Check selection

Repository commands take precedence. Without them, choose the relevant command,
not this entire list as a ritual:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Use `--all-features` only if simultaneous enablement is valid; cover required
non-default/target-specific configurations. Performance claims require a
representative measurement. Inspect automated fixes for unrelated churn,
generated changes, suppression, and accidental API, dependency, or MSRV shifts.
