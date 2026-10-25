---
name: verification
description: >-
  Choose which proportionate evidence proves a claim and interpret its result,
  including delayed or ambiguous runs; write, change, delete, run or speed up
  automated tests in any project: picking the cheapest test that protects a
  named guarantee, what earns a test, property, replay and golden tests, test
  tiers and CPU cost, committed records, flaky tests, pruning a suite, landing
  gates and reuse by input hash.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Verification

## Evidence for a claim
1. Name the stable acceptance claim and the action its result changes, because a check whose pass and fail select the same action is waste.
2. Run the smallest operator-visible journey before deeper component checks, because component passes do not prove the combined path.
3. Hold the candidate and its verdict-sensitive launcher, environment, TTY and fixture state stable, because a changed wrapper invalidates the observation.
4. Reuse passing results until a relevant behaviour change invalidates them; run each check once, and once more after a fix, because reruns of unchanged inputs buy nothing.
5. Resolve ambiguous observations and repair the first actual divergence before another expensive run, because later errors are often consequences.
6. Reassess at roughly twice the estimate and restart only for observed failure or a supported corrective change, because silence is not deadlock and restarts lose progress.

Read [evidence selection](references/evidence-selection.md) when choosing checks, [pricing and progress](references/pricing-and-progress.md) for delayed runs, and [failure diagnosis](references/failure-diagnosis.md) for a failed check or broken driver. Read [notes](references/notes.md) when a project picks or revisits its test posture, or when the landing tool refuses a test change or a CI budget.

## Tests: the burden is on the test
Iteration speed is everything, so every test names the guarantee it protects, in its title, as a behaviour a person would notice breaking. Keep the cheapest test that protects that guarantee; delete the rest.

- Before writing a test, ask what is the fastest check that would fail if the guarantee broke (a type, a table, a property over a pure core, one seeded scenario) and write that one, because cheaper tests keep the loop fast. Reach for a sweep, whole-roster run, native pair or committed record only when nothing cheaper can fail.
- Tighten only where a bug escaped, extending the property, scenario or reference that should have caught it, because escapes, not fear, buy strictness.
- Never lower an assertion, threshold or seed count to make a failing change pass, and never retry a flake to green, because right-sizing is not weakening.
- Delete or merge on sight tests whose title names no guarantee, duplicates, tests of removed features, implementation-detail asserts, stale snapshots and checks that only ever failed when data legitimately moved; name the kept test that still protects the guarantee, or say nobody cares about it, so no guarantee is lost silently.
- Remove the need first: one table over every case, types that rule out bad states, one source of truth generating the rest, because a bug that can't be written needs no test.
- Keep logic in a pure deterministic core fed time, randomness and peers as inputs; test through the caller's interface and fake only what we don't own, because fakes of our own code drift from it.

## Five kinds of test worth keeping
1. **Recorded scenarios**: real or seeded input through the whole system, checking invariants and agreement; keep one injected fault that must trip it.
2. **Properties over a pure core**: generated inputs against bounds, round-trips or a reference model; locally a fixed seed and a few hundred cases; wide search after landing; keep shrunk failures named.
3. **Product measurements**: owner-decided numbers with their `Provenance:` record ([planning](../planning/SKILL.md), the `Provenance:` rule); a missed soft one lands with `Target-debt:`.
4. **External references**: truth from a spec, reference implementation or recorded real output, cited in the title or fixture.
5. **One integration check per real boundary**, run end to end.

Agents verify every mechanical check themselves on VM pairs and native capture (input, sizes, frame timing, a sound on its frame, a build in the game list, menus), never through the operator, because a human's time is the scarcest input.

## Tiers
- **Edit loop (under ~60 s, local)**: type-check plus the tests of what changed; a worker runs this before pushing.
- **Landing gate**: tests whose inputs changed, one seeded representative of each sweep family whose guarantee no ordinary test covers, and one seconds-long end-to-end path, fast enough to land changes 1.5x faster than they arrive, because a queue that grows never drains. A new sweep runs after landing unless it earns a gate slot.
- **After landing**: every sweep, full roster and wide property search on each landed SHA, on runners that exist; bisect a failure, confirm it on the parent, auto-revert the culprit. Check a tier ran before moving work into it, because a tier nobody runs protects nothing.
- **Nightly / manual**: soaks, native pairs, cost censuses.

## Committed records
- A snapshot, baseline or record that unrelated changes make stale costs every lane a refresh, so compute it on CI from the last green run, key it per row so only touched rows move, or keep only the absolute bound and drop the drift record.
- Delete a record no check reads, because it protects nothing.

## Delete on sight (largest first, from seven repository reviews)
Example matrices re-testing one rule a property should own; our own prose, constants or argv asserted verbatim; one scenario rebuilt per layer or run twice; count pins on fixtures, catalogs or test totals; suites no runner collects; sleeps used as coordination; fakes of our own code; oracles that pass when nothing was checked; source greps and tombstones; single bugs pinned as their own tests; asserts of what the types guarantee; live probes committed as tests; full-width sweeps at the landing gate. Count cases with the runner, never a regex, because regex counts were refuted by runners. Counts per class: [testing sources](references/testing-sources.md).

## Mutation and landing-tool rules
- A mutant settles doubt about whether a test is the only one protecting a guarantee; it is not a toll on every deletion.
- Mutate only in a scratch copy, never the lane, and mark every mutated line `MUTANT:<rule-id>`, because reviewers have committed product mutants; the landing tool refuses an added line with that token.
- Record the mutant patch in the scratch copy before claiming a kill; the landing tool refuses an empty record and requires the patch to apply to the landing base or head.
- Product code never branches on a test's literal inputs, because that passes tests while breaking the product.

## Landing fast
- Reuse every build and test result whose inputs are unchanged, keyed by a content hash of those inputs, and never run one suite twice for one landing, because rerunning unchanged inputs is waste.
- Give each repeated stage between finished work and the owner's hands (build, test, land, install) a wall-time budget, and have the tool that already logs its time open or update one issue for an agent when a run exceeds it, because unreused work is found from timings already logged, not by the owner or a schedule; such budgets alert and never gate.
- Call an external tool once per batch, not per file, row, test or branch; fall back per item only for exact diagnostics on failure, because process start dominates small calls.
- Bound cost in a deterministic unit (instructions, frames, allocations), never wall seconds, because wall time flakes under load; shrink inputs or move work after landing instead of raising a ceiling. Run a timing check once, under an exclusive capacity lease.
- Quarantine a flake at first sight with its seed, log and an issue; fix or delete it within a week, because a flake hides real failures. Never retry to green, sleep or weaken an expectation.
