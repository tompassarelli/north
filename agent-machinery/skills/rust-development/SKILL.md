---
name: rust-development
description: >-
  Develop, review, debug, test or optimize Rust and Cargo code.
grounded: 2026-10-15
written: 2026-10-15
---

# Rust development

1. Preserve repository edition, MSRV, features, targets, APIs, formats and unsafe policy within the requested scope, because consumers depend on them and a silent shift breaks builds downstream.
2. Borrow temporary data and own retained data without compiler-silencing `clone`, `Rc<RefCell>`, `RefCell` or `Arc<Mutex>`; pass `&mut` down, split borrows by field, index into a `Vec` or arena, or move ownership to the caller, because shared mutability hides the ownership mistake and adds cost, runtime panics or deadlock risk.
3. Add a trait or generic parameter only for a second real implementation or a test seam in use, because a single-implementation abstraction costs every reader and compile time; split crates for parallel builds per [compile times](references/compile-times.md), not into chains.
4. Use `Option` for absence, `Result` for expected failure and panic for invariant failures, so callers can tell recoverable cases from bugs.
5. Preserve error sources and document public error, panic and safety contracts, so callers can diagnose failures and uphold obligations the types cannot express.
6. Respect `forbid(unsafe_code)` or isolate unsafe invariants with relevant supported Miri checks, because the compiler cannot verify them.
7. Keep async work owned, bounded and cancellation-safe without blocking executors or holding locks across `.await`, because blocked executors stall every task and held locks deadlock.
8. Start a new subsystem as its own crate in `crates/` with a flat layout and a `lib.rs` that states what the crate may and may not depend on, after rust-analyzer's architecture invariants; a crate over its `crate_lines` cap in `structure.toml` gets no net growth and new code goes to a new crate, because one giant crate makes every leaf edit rebuild and retest everything.

Clippy's clone lints and the code-health Rust habit checks enforce rules 2 and 3 at edit time and landing; a justified exception is an `#[allow(clippy::x, reason = "...")]` or a `[[rust_habits.allow]]` entry in `structure.toml`.

Inner loop is `cargo check -p CRATE` or `cargo clippy -p CRATE --tests`; build and run the tests once at the end, because a test build links every test binary and starves the machine for an answer the type checker gives in seconds.

## Data-oriented hot paths

Use [performance design](../performance-design/SKILL.md) when a runtime budget calls for optimization.

- Sweep `Vec` records or parallel field arrays in passes and choose the layout from access patterns and measurements, because a 64-byte record is not a universal target.
- Keep cold fields separate and use indices instead of pointer graphs in hot data, so the hot loop touches only the memory it needs.
- Write independent output buffers and keep allocation and dynamic dispatch out of the hot loop, so passes stay predictable and parallelizable.
- Check a benchmark or assembly before claiming a branchless win, because compilers often already emit the branchy form equivalently.
- Share read-only inputs and private outputs between threads, and give SPSC publication acquire/release ordering, so readers never see a half-written value.

Read [Rust and Cargo details](references/notes.md) when selecting idioms, async/unsafe checks or build commands.
Read [compile times](references/compile-times.md) before changing linkers, profiles, caches, crate splits or test layout for build speed.
The project toolchain: nightly with Cranelift and `-Zthreads=8` for dev and test, LLVM for release (`rust-toolchain.toml`, `.cargo/config.toml`).
