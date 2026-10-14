---
name: rust-development
description: >-
  Develop, review, debug, test, or optimize Rust and Cargo code, including ownership, errors, async, unsafe boundaries, and build behavior.
grounded: 2026-10-09
written: 2026-10-09
---

# Rust development

1. Preserve repository edition, MSRV, features, targets, APIs, formats and unsafe policy within the requested scope.
2. Borrow temporary data and own retained data without compiler-silencing `clone`, `Arc` or `Mutex`.
3. Use `Option` for absence, `Result` for expected failure and panic for invariant failures.
4. Preserve error sources and document public error, panic and safety contracts.
5. Respect `forbid(unsafe_code)` or isolate unsafe invariants with relevant supported Miri checks.
6. Keep async work owned, bounded and cancellation-safe without blocking executors or holding locks across `.await`.

## Data-oriented hot paths

Use [performance design](../../../harness/skills/performance-design/SKILL.md) when
a runtime budget calls for optimization. Sweep `Vec` records or parallel field
arrays in passes, choosing the layout from access patterns and measurements;
a 64-byte record layout is not a universal target. Keep cold fields separate,
use indices instead of pointer graphs in hot data, and write independent output
buffers. Keep allocation and dynamic dispatch out of the hot loop. Check a
benchmark or assembly before claiming a branchless win. Share read-only inputs
and private outputs between threads; SPSC publication still needs acquire/release
ordering.

Read [Rust and Cargo details](references/notes.md) when selecting idioms, async/unsafe checks or build commands.
Read [compile times](references/compile-times.md) before changing linkers, profiles, caches, crate splits or test layout for build speed.
