# Measurement and comparable evidence

## Scope

Engineer the speed and invalidation economics of a named repeated
edit-to-signal or edit-to-behavior loop. `verification-distilled` still chooses
and interprets any individual correctness check; this skill owns the repeated
loop's latency budget, measurement, dependency shape, and optimization.

## Admit and distinguish the real loop

Name the exact edit, signal or behavior, expected uses per work period, human or
agent consumer, latency budget, and action fork. A useful fork is: within budget
means keep the loop; over budget means measure and reprice its owning costs. No
repetition, consumer, budget, or changed action means no optimization work.

Keep these modes separate and label cache, toolchain, hardware, and workload
state for every sample:

- **hot** — a resident watcher, process, or hot-reload path handles the edit;
- **warm incremental** — reusable artifacts exist, but a compile/test process
  may restart;
- **cold/bootstrap** — required artifacts or processes start absent; and
- **full boundary** — the explicitly requested whole-project or broad gate.

Never average unlike modes or present measurements from different scopes,
hardware, cache states, or toolchain revisions as comparable.

Positive triggers include designing a source edit to admitted game-frame loop,
diagnosing why a warm incremental compile regressed, or deciding whether test
selection and module boundaries can repay faster iterations. “Run this test
once,” an ordinary one-off build, and choosing which correctness check proves a
change remain outside this skill; do not turn them into profiling campaigns.

## Measurement and causal hypotheses

Measure the end-to-end loop first with enough repeated observations to expose
ordinary variance, retaining individual samples and a stated summary such as a
median. Decompose compile front end, code generation, linking, process startup,
harness discovery, test execution, reload, or rendering only when the result
changes the next optimization. Measure before blaming file size, the compiler,
tests, linking, or cache behavior.

Ground a deliberate optimization against both:

1. the local or theoretical lower bound: mandatory work, the invalidation graph,
   dependency fan-out, parallel critical path, and irreducible startup; and
2. at least one relevant project or toolchain using like-for-like authoritative
   measurements when they are available.

Record the comparable's command/workload scope, mode, hardware, toolchain
revision, and source. If no like-for-like authoritative evidence exists, state
the gap instead of substituting an incomparable number. A peer is a reference
point, not a ceiling: a lower bound or invalidation graph may show that every
available peer is underoptimized.

## Interpreting the lower bound

Mandatory work and the dependency critical path constrain possible savings.
A peer benchmark can expose a missed technique, but unlike workloads cannot
rank implementations. Keep individual observations so variance or cache drift
cannot disappear inside an average. Stop decomposing when the next change is
already selected.
