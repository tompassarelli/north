# Optimization economics

## Correctness boundary

Keep correctness signals lean without weakening them. Use the nearest
decision-changing check selected by `verification-distilled`, dependency-aware
test selection when supported, and one owner for each reachable failure wave.
Do not repeatedly run a broad suite after a narrower result already determines
the next action, and never lower an assertion or gate to improve timing.

## Locate the cost owner

Treat incremental compilation and invalidation as first-class. Observe which
units rebuild and why; then reduce invalidated work, the critical path, linker or
startup cost, harness overhead, or redundant checks at the smallest owning
boundary. Prefer existing compiler caches, incremental modes, resident
processes, hot reload, and dependency metadata when their measured semantics fit
the loop.

Split or merge files, modules, packages, or crates only when measured fan-out,
parallelism, or per-unit overhead predicts a net win and the next sample can
decide it. Line count alone proves nothing. Reject fragmentation that trades
less invalidated work for more parsing, linking, scheduling, API surface, or
maintenance cost.

Price measurement, implementation, and ongoing maintenance against the expected
remaining uses: `uses × credible saved latency` must repay the total cost within
the named horizon. Stop at the useful 80/20 point. A theoretical speedup without
a paying repeated loop is not work.

## Alternatives and tradeoffs

Fewer invalidated units can mean more per-unit scheduling/linking overhead.
A resident process trades startup for lifecycle and memory cost. A cache trades
recomputation for identity and invalidation obligations. Choose the mechanism
whose measured savings repay its actual maintenance horizon; none is the default
answer to every slow loop.
