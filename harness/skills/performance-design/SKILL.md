---
name: performance-design
description: >-
  Design or improve code with a frame, tick, latency or throughput budget;
  measure its hot paths, organize passes and choose a runtime-specific data layout.
grounded: 2026-10-14
written: 2026-10-14
metadata:
  kind: playbook
---

# Performance design

1. State the budget and record a baseline with the consumer runtime's profiler or
   counter. Every optimization needs a before and after measurement.
2. Decompose work into passes over flat data, one kind of calculation per pass.
   Move expensive or irregular work offline or to load time and ship precomputed
   tables.
3. Lay out data by the measured access pattern. Prefer indices and flat arrays
   for hot data; compare plain records with separate field arrays. Keep cold
   fields apart, write separate output buffers and mutate between passes.
4. Keep allocation, dynamic dispatch, capturing closures, hashing and string
   construction out of the hot loop.
5. Keep predictable branches. Adopt a branchless form only when unpredictable
   input and a measured result justify it.
6. Share read-only inputs between threads and give each thread its own output.
   Keep locks outside hot loops. For one producer and one consumer, an SPSC ring
   still needs acquire/release ordering when it publishes and consumes data.
7. Use the runtime's cost model. [Rust development](../../../agent-machinery/skills/rust-development/SKILL.md)
   covers native caches, branches, instruction cache and SIMD. [Lua performance](../lua-performance/SKILL.md)
   covers Lua 5.3 opcodes, table allocation and GC; those counters do not measure
   hardware cache effects. In JS, V8 or Bun, examine hidden classes, monomorphic
   call sites, typed arrays and GC.
8. Stop when the budget holds. Preserve readability and determinism.
