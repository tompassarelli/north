---
name: rust-development-reference
description: >-
  Inactive detailed reference for rust-development-distilled. Use only when
  that workflow requests Rust idioms, error and conversion guidance,
  async/unsafe detail, build-loop analysis, or fallback Cargo commands, or when
  the user explicitly requests rust-development-reference.
---

# Rust-development reference

## APIs and control flow

Prefer `&str`, `&[T]`, and `&Path` internally. Use broader bounds only for real
callers. Prefer enums or newtypes to ambiguous booleans. Implement `From` for
infallible, lossless, value-preserving, obvious conversions and `TryFrom` for
fallible ones. Derive only honest traits; avoid speculative builders and
generics.

Use `?` for propagation, `let ... else` for a required binding with early exit,
`if let` for one interesting pattern, and `match` for alternatives or
exhaustiveness. Prefer `for` for straightforward iteration, adapters for a clear
transformation, and an explicit loop for state, early exit, mutation, or tricky
borrowing. Do not collect only to iterate again.

```rust
let Some(path) = request.path() else {
    return Ok(Response::not_found());
};

let records = lines
    .map(parse_record)
    .collect::<Result<Vec<_>, ParseError>>()?;
```

## Errors, async, and unsafe details

Follow the repository's error stack. Reusable boundaries commonly expose typed
errors; application coordination may use contextual erased errors. Add neither
`thiserror` nor `anyhow` reflexively. `expect` is suitable only in tests or for
a locally proven invariant and should explain why failure is impossible.
Document intentional best-effort suppression.

Use the runtime's blocking adapter for blocking work. Preserve valid state under
cancellation. Keep unsafe in the smallest block, use `// SAFETY:` for the local
invariant, and a `# Safety` section for caller obligations.

## Build-loop analysis

Record package/workspace scope, target, features, profile, flags and wrappers,
target directory, cache state, and wall time. Compare only equivalent runs.
Keep recurring hot-loop inputs stable and change one reversible variable at a
time. Use Cargo timings or compiler output before restructuring. Modules are
reasoning boundaries; crates are compilation/cache boundaries. Split a crate
only after repeated comparable evidence identifies an invalidation edge.

Treat custom profiles, `RUSTFLAGS`, wrappers, codegen, and cache changes as
measured opt-ins rather than global defaults.

## Fallback completion commands

Adapt these only when the repository supplies no stronger authority:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Use `--all-features` only when simultaneous enablement is valid. Cover required
non-default and target-specific configurations. Before performance claims, run
a representative benchmark or profile. Inspect automated fixes and the final
diff for unrelated churn, debug code, generated files, suppressed warnings,
and accidental API, dependency, or MSRV changes.
