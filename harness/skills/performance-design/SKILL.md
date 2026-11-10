---
name: performance-design
description: >-
  Code under a frame, tick, latency or throughput budget: hot paths, data layout; a slow edit-to-result loop.
grounded: 2026-11-10
written: 2026-11-10
metadata:
  kind: playbook
---

# Performance design

## Runtime budgets

1. State the budget and record a before-and-after baseline with the consumer
   runtime's profiler or counter, because every optimization needs both readings.
2. Decompose work into passes over flat data, one calculation per pass, so each
   pass stays cache- and branch-friendly; move expensive or irregular work
   offline or to load time and ship precomputed tables.
3. Lay out data by the measured access pattern, since layout decides memory
   traffic: indices and flat arrays for hot data, plain records compared against
   separate field arrays, cold fields apart, output buffers separate and mutated
   between passes.
4. Keep allocation, dynamic dispatch, capturing closures, hashing and string
   construction out of the hot loop, because each costs per iteration.
5. Keep branches predictable; adopt a branchless form only for unpredictable
   input that a measurement justifies, since it often costs more otherwise.
6. Share read-only inputs between threads and give each thread its own output, so
   threads never contend; keep locks outside hot loops and publish with
   acquire/release ordering, so a consumer never reads unpublished data.
7. Use the runtime's cost model: native caches, branches, instruction cache and
   SIMD; [Lua performance](../warcraft-modding/references/lua-performance.md)
   for Lua 5.3 opcodes, table allocation and GC, counters that do not measure
   hardware cache effects; hidden classes and monomorphic call sites in V8,
   structures and inline caches in JavaScriptCore (Bun), typed arrays and GC.
   [Rust development](../rust-development/SKILL.md) applies these layout, branch
   and publication rules to Rust.
8. Stop when the budget holds; keep readability and determinism, since speed past
   the budget buys nothing.

## Development loops

1. Name the repeated edit, result, consumer, use count and latency budget; a loop
   with no repetition or budget is not worth optimizing.
2. Measure the end-to-end loop in hot, warm incremental, cold/bootstrap and
   full-gate modes, since averaging unlike modes hides the cost.
3. Compare equivalent workload, hardware, toolchain and cache states against
   mandatory work, invalidation and a comparable authoritative benchmark, so the
   gap is real and not a setup difference.
4. Change the smallest measured cause with the existing incremental, cache,
   selection or reload facilities, and never weaken a check for time, because a
   faster wrong signal is worse.
5. Re-measure the same loop and price investigation, implementation and
   maintenance against its remaining uses under [pricing](../verification/references/pricing-and-progress.md),
   because a speedup must repay its cost.
6. Schedule recurrence only with a baseline, regression action, owner, consumer and
   capacity budget, because observation that changes no action wastes capacity.

Read [loop measurement](references/loop-measurement.md) to sample or compare a
loop, [loop optimization](references/loop-optimization.md) to choose what to
change, and [loop recurrence](references/loop-recurrence.md) for a regression or
recurring observation.
