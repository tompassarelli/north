---
name: rust-development-distilled
description: >-
  Develop, review, debug, test, or optimize Rust and Cargo code, including ownership, errors, async, unsafe boundaries, and build behavior.
---

# Rust development

Use repository code, manifests, toolchain, and required checks as authority.
Preserve edition, MSRV, features, targets, APIs, formats, and unsafe policy
unless the requested change includes them.

Borrow for temporary use and own retained data. Do not add `clone`, `Arc`, or
`Mutex` merely to silence the compiler. Prefer private concrete interfaces,
valid-state types, and native path types; justify new dependencies and
abstractions with current callers.

Use `Option` for absence, `Result` for expected failure, and panic only for
programmer or invariant failures. Preserve error sources. Document public
error, panic, and safety contracts.

Respect `forbid(unsafe_code)`. Otherwise isolate unsafe code, document and test
its invariants, and use supported Miri checks when relevant. Keep async work
bounded, cancellation-safe, and owned; do not block the executor, hold locks
across `.await`, or detach tasks casually.

Use lane-local build output, targeted checks, and repository-required gates.
Measure comparable runs before changing build policy. For idioms, async/unsafe
detail, and Cargo command selection, use
`agents path rust-development-reference`.
