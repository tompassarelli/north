---
name: competitive-development-loop-distilled
description: >-
  Design, diagnose, measure, or optimize a repeated agent development feedback
  loop when compile, test, link, startup, harness, hot-reload, or incremental
  invalidation latency materially controls iteration speed. Do not use for an
  ordinary one-off command, correctness-check selection, or a profiling
  campaign without a named recurring loop.
---

# Competitive development loop

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

## Establish evidence before a cause

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

## Optimize the paying path

Keep correctness signals lean without weakening them. Use the nearest
decision-changing check selected by `verification-distilled`, dependency-aware
test selection when supported, and one owner for each reachable failure wave.
Do not repeatedly run a broad suite after a narrower result already determines
the next action, and never lower an assertion or gate to improve timing.

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

## Handle regressions without blind repetition

An observed loop over its budget, or roughly twice an established expectation,
stops blind reruns. Preserve the mode and measurements, classify the changed
phase or invalidation edge, and reprice the loop before retrying, widening a
timeout, or running a broader suite. Fix an observed owning cause; do not infer
one from the symptom.

## Admit recurrence only when it pays

Daily light sampling, weekly deeper profiling, or spare-capacity/night work is
eligible only when all of these facts exist: the named repeated loop, a current
baseline, a regression threshold, exact owner and consumer, a bounded capacity
budget, and a pass/fail result that changes an action. Recurrence is an optional
operating mode, not permission to create a scheduler, daemon, thread, dashboard,
or generalized performance program.

A minimal durable record contains the loop identity and workload, mode and
environment, owner and consumer, use frequency and budget, baseline samples and
date, lower-bound/invalidation hypothesis, comparable source and revision,
regression threshold, capacity budget, latest result, and the resulting action.
If those facts do not support a different next action, do not schedule the run.

Report the named loop, mode, baseline and budget, lower bound and comparable,
measured bottleneck, change, observed result, break-even, and residual
uncertainty. Do not add distribution or release ceremony.
