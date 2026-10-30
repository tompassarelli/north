---
name: performance-design
description: >-
  Design or improve code with a frame, tick, latency or throughput budget;
  measure its hot paths, organize passes and choose a runtime-specific data layout.
  Also measure and improve a named repeated edit-to-result development loop when
  its latency materially affects iteration speed.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Performance design

## Runtime budgets

1. State the budget and record a baseline with the consumer runtime's profiler or
   counter, because every optimization needs a before and after measurement.
2. Decompose work into passes over flat data, one kind of calculation per pass, so
   each pass stays cache- and branch-friendly. Move expensive or irregular work
   offline or to load time and ship precomputed tables.
3. Lay out data by the measured access pattern, since layout decides memory
   traffic. Prefer indices and flat arrays for hot data; compare plain records
   with separate field arrays. Keep cold fields apart, write separate output
   buffers and mutate between passes.
4. Keep allocation, dynamic dispatch, capturing closures, hashing and string
   construction out of the hot loop, because each one costs per iteration.
5. Keep predictable branches. Adopt a branchless form only when unpredictable
   input and a measured result justify it, since it often costs more otherwise.
6. Share read-only inputs between threads and give each thread its own output, so
   threads never contend. Keep locks outside hot loops. For one producer and one
   consumer, an SPSC ring still needs acquire/release ordering when it publishes
   and consumes data, or the consumer can read unpublished data.
7. Use the runtime's cost model, because costs differ by runtime. For native code,
   measure caches, branches, instruction cache and SIMD; follow
   [Rust development](../rust-development/SKILL.md)
   for data layout and measured branch changes. [Lua performance](../warcraft-modding/references/lua-performance.md)
   covers Lua 5.3 opcodes, table allocation and GC; those counters do not measure
   hardware cache effects. In JS, V8 or Bun, examine hidden classes, monomorphic
   call sites, typed arrays and GC.
8. Stop when the budget holds, and preserve readability and determinism, since
   speed past the budget buys nothing.

## Development loops

1. Name the repeated edit, result, consumer, use count and latency budget, because
   a loop with no repetition or budget is not worth optimizing.
2. Measure the end-to-end loop with separate hot, warm incremental, cold/bootstrap
   and full-gate samples, since averaging unlike modes hides the cost.
3. Compare equivalent workload, hardware, toolchain and cache states against
   mandatory work, invalidation and a comparable authoritative benchmark, so the
   gap is real and not a setup difference.
4. Change the smallest measured cause using existing incremental, cache, selection
   or reload facilities, and never weaken a check to gain time, because a faster
   wrong signal is worse.
5. Re-measure the same loop against investigation, implementation and maintenance
   break-even, since a speedup must repay its cost over the remaining uses.
6. Schedule recurrence only with a baseline, regression action, owner, consumer and
   capacity budget, because observation that changes no action wastes capacity.

Read [loop measurement](references/loop-measurement.md) when sampling or comparing
a loop, [loop optimization](references/loop-optimization.md) when choosing what to
change, and [loop recurrence](references/loop-recurrence.md) for a regression or
recurring observation.
