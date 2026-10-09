---
name: rust-development
description: >-
  Develop, review, debug, test, or optimize Rust and Cargo code, including ownership, errors, async, unsafe boundaries, and build behavior.
---

# Rust development

1. Preserve repository edition, MSRV, features, targets, APIs, formats and unsafe policy within the requested scope.
2. Borrow temporary data and own retained data without compiler-silencing `clone`, `Arc` or `Mutex`.
3. Use `Option` for absence, `Result` for expected failure and panic for invariant failures.
4. Preserve error sources and document public error, panic and safety contracts.
5. Respect `forbid(unsafe_code)` or isolate unsafe invariants with relevant supported Miri checks.
6. Keep async work owned, bounded and cancellation-safe without blocking executors or holding locks across `.await`.

Read [Rust and Cargo details](references/notes.md) when selecting idioms, async/unsafe checks or build commands.
