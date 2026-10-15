# Numeric layout measurement

Measured 14 October 2026 on the consumer's stock Lua 5.3.6 `LUA_32BITS` runtime
(`math.maxinteger == 2147483647`), binary SHA-256
`d6493fead1709d5a96189923b3e5f890fc25518359e825e3702108444a076a5d`.

Eight entities start with `x=i`, `y=2*i`, `dx=0.125` and `dy=0.25`. Each pass
executes 4,096 ticks. The record pass loads `actor=actors[i]`, then updates
`actor.x=actor.x+actor.dx` and `actor.y=actor.y+actor.dy`. The parallel-array pass
updates `x[i]=x[i]+dx[i]` and `y[i]=y[i]+dy[i]`, with its arrays cached in locals
outside the loops. Inputs and the counter hook are allocated before measurement.

Each pass warms once, then runs with GC stopped and `debug.sethook` counting one
VM instruction per hook. Allocation is the whole call's
`collectgarbage('count')` delta in bytes. Final positions agree exactly.

| Layout | VM instructions | Whole-call allocation |
| --- | ---: | ---: |
| Per-entity records | 352,268 | 912 B |
| Parallel number arrays | 319,503 | 912 B |

Parallel arrays save 32,765 instructions, or 9.3%. This result supports that
numeric access pattern; it claims no allocation saving or native cache benefit.
The initial cold measurement included unequal stack overhead and is preserved
separately. Warm measurement source SHA-256:
`7fc796922ce500dcb0d1f43419bb3ce188c4cca8c6bedb5db09962b1708a3e80`.
