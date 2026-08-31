---
name: rust-development-distilled
description: >-
  Default distilled workflow for writing, editing, reviewing, debugging,
  testing, or optimizing Rust and Cargo projects, including APIs, ownership,
  errors, async, unsafe code, build loops, and concurrent Rust worktrees.
---

# Rust development

Repository policy, manifests, toolchains, CI, code, and tests are authority.
Preserve edition, MSRV, targets, features, unsafe policy/runtime. Map
packages, callers, configurations, tests, and boundaries; never edit generated
code.

Change minimally. Never silently alter APIs, durable formats, CLI, defaults,
dependencies, MSRV, or unrelated behavior. Add abstraction/dependency/async/
synchronization only for proven value. Borrow temporarily; own for retention;
never add `clone`, `Arc`, or `Mutex` to appease the compiler. Prefer private
concrete APIs, valid states, and native paths.

Use `Option` for absence, `Result` for expected failure, panic only for
programmer/invariant failure, and never `unwrap` ordinary production failure.
Preserve error sources. Respect `forbid(unsafe_code)`; isolate unsafe,
state/test/document invariants, and use supported Miri. Never add needless async,
block its executor, lock across `.await`, allow unbounded work, or detach tasks.

Test deterministically; document public error/panic/safety contracts. Run
targeted then required checks. Measure like-for-like before build-policy change.
Each concurrent Rust lane needs distinct `CARGO_TARGET_DIR`; sharing artifacts
is a correctness failure. Never claim an unrun check.

For syntax choices, conversion/error guidance, async/unsafe detail, build-loop
measurement, and fallback Cargo commands, run
`agents path rust-development-reference` and read its `SKILL.md` completely.
