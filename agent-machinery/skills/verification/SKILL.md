---
name: verification
description: >-
  Pick proportionate evidence for a claim and read its result, including slow,
  silent or ambiguous runs; write, change, prune, run or speed up automated
  tests in any project: what earns a test, property, replay and golden tests,
  tiers and CPU cost, committed records, mutants, flaky tests, landing gates
  and reuse by input hash.
grounded: 2026-10-15
written: 2026-11-04
metadata:
  kind: playbook
---

# Verification

## Every use
- Name the stable acceptance claim and the action its result changes, because a check whose pass and fail select the same action is waste.
- Reuse passing results until a relevant behaviour change invalidates them; run each check once, and once more after a fix, because reruns of unchanged inputs buy nothing.
- Every test names the guarantee it protects, in its title, as a behaviour a person would notice breaking. Iteration speed is everything, so keep the cheapest test that protects that guarantee and delete the rest.
- A bug fix lands with at least one test that fails on the old code, and the report pastes that failure and the passing run after the fix, because a test never seen failing may not reach the bug. Pick it by where the bug lives: decision logic (a pure function or state machine) gets a proptest property whose shrunk counterexample shows the bug; an environment or integration bug (cgroups, processes, hosts, git state, timers) gets a fixture that recreates the environment, such as a 1-CPU systemd scope or a temp repo with two clones; a protocol bug gets a case in the protocol model. A refactor declares `Behavior: unchanged` in its commit message, and its changed tests must pass on the old code. A fails-first landing gate enforces both directions.
- Never lower an assertion, threshold or seed count to make a failing change pass, and never retry a flake to green, sleep or weaken an expectation, because right-sizing is not weakening. Quarantine a flake at first sight with its seed, log and an issue; fix or delete it within a week, because a flake hides real failures.
- Agents verify every mechanical check themselves on VM pairs and native capture (input, sizes, frame timing, a sound on its frame, a build in the game list, menus), never through the operator, because a human's time is the scarcest input.

## Read when
- Choosing which checks prove a claim, or whether an end-to-end journey must run before component checks: [evidence selection](references/evidence-selection.md).
- A run is slower than estimated, silent, or tempting to restart: [pricing and progress](references/pricing-and-progress.md).
- A check failed, its observation is ambiguous, or its driver broke: [failure diagnosis](references/failure-diagnosis.md).
- Before writing or changing a test: [writing tests](references/writing-tests.md).
- Deleting, merging or auditing tests or a suite, touching a committed snapshot, baseline or record, or doubting a test is the only guard (mutants): [pruning tests](references/pruning-tests.md).
- Placing a test in a tier, choosing what to run before landing or what the landing gate or CI runs, or a build, test or landing is slow: [tiers and landing](references/tiers-and-landing.md).
- A project picks or revisits its test posture, or the landing tool refuses a test change or a CI budget: [notes](references/notes.md).
