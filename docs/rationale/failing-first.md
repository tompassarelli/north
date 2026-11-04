# Failing-first rationale

Cold-path reasoning behind the rule that a bug fix lands with a test seen failing on the code before the fix, and a
refactor declares `Behavior: unchanged` with its changed tests passing on the old code. Agents load the rule from the
verification skill's Every use list and the workers skill's brief reference; this file holds why, the measure and what
would overturn it.

## Why

Tom chose the strictest version of the rule on 2026-11-04, after two measurements that day:

- A 13-property suite over the admission code took about 30 minutes to write. It re-found the day's costliest bug,
  admission writing `CPUQuota=0%` on a 1-core attended host, as a shrunk counterexample, and found a second copy of the
  same bug. Review had missed both.
- A mutation sweep over the landing code generated 188 bugs; tests caught 117 and missed 71.

A test that never failed on the old code may not reach the bug at all, and the mutation result says that is common in
this tree. Seeing it fail first is the cheapest proof that it does. The same check run the other way keeps a refactor
honest: a changed test that fails on the old code means behaviour changed.

Test kind follows where the bug lives, because the cheapest test that can fail differs: a property over decision logic,
a fixture recreating the environment for cgroup, process, host, git or timer bugs, and a case in the stateright landing
model for protocol bugs once that model exists.

## Enforcement

The fails-first landing gate (lane fails-first-gate, built in parallel) enforces both directions. Until it lands, the
brief's Done when and the worker's pasted failure and passing run carry the rule.

## Review

Two weeks after adoption (north#524), compare from the ledger the rework (reverts, follow-up fixes, red-main culprits) of
fix lanes with and without a failing-first test over 2026-11-04 to 2026-11-18, and keep, change or drop the rule on that
result.
