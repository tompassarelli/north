---
name: rust-development
description: >-
  Develop, review, debug, test, or optimize Rust and Cargo code, including ownership, errors, async, unsafe boundaries, and build behavior.
grounded: 2026-10-15
written: 2026-10-15
---

# Rust development

1. Preserve repository edition, MSRV, features, targets, APIs, formats and unsafe policy within the requested scope, because consumers depend on them and a silent shift breaks builds downstream.
2. Borrow temporary data and own retained data without compiler-silencing `clone`, `Arc` or `Mutex`, because those hide the ownership mistake and add cost or deadlock risk.
3. Use `Option` for absence, `Result` for expected failure and panic for invariant failures, so callers can tell recoverable cases from bugs.
4. Preserve error sources and document public error, panic and safety contracts, so callers can diagnose failures and uphold obligations the types cannot express.
5. Respect `forbid(unsafe_code)` or isolate unsafe invariants with relevant supported Miri checks, because the compiler cannot verify them.
6. Keep async work owned, bounded and cancellation-safe without blocking executors or holding locks across `.await`, because blocked executors stall every task and held locks deadlock.

## Data-oriented hot paths

Use [performance design](../performance-design/SKILL.md) when a runtime budget calls for optimization.

- Sweep `Vec` records or parallel field arrays in passes and choose the layout from access patterns and measurements, because a 64-byte record is not a universal target.
- Keep cold fields separate and use indices instead of pointer graphs in hot data, so the hot loop touches only the memory it needs.
- Write independent output buffers and keep allocation and dynamic dispatch out of the hot loop, so passes stay predictable and parallelizable.
- Check a benchmark or assembly before claiming a branchless win, because compilers often already emit the branchy form equivalently.
- Share read-only inputs and private outputs between threads, and give SPSC publication acquire/release ordering, so readers never see a half-written value.

Read [Rust and Cargo details](references/notes.md) when selecting idioms, async/unsafe checks or build commands.
Read [compile times](references/compile-times.md) before changing linkers, profiles, caches, crate splits or test layout for build speed.
