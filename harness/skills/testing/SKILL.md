---
name: testing
description: >-
  Write, change, delete, run or speed up automated tests in any project: picking
  the cheapest test that protects a named guarantee, what earns a test, property,
  replay and golden tests, test tiers and CPU cost, committed records, flaky
  tests, pruning a suite, landing gates and reuse by input hash.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---
# Testing

**Iteration speed is everything; the burden is on the test.** Every test names
the guarantee it protects, in its title, as a behaviour a person would notice
breaking. Keep the cheapest test that protects that guarantee; delete the rest.

## Pick the minimal sufficient test first
- Before writing a test, name the guarantee and ask what is the fastest check
  that would fail if it broke: a type, a table, a property over a pure core,
  one seeded scenario. Write that one. Reach for a sweep, a whole-roster run,
  a native pair or a committed record only when nothing cheaper can fail.
- Try to get away with the cheaper test. When a bug escapes that a stricter
  test would have caught, tighten at that spot: extend the property, scenario
  or reference that should have caught it. Escapes, not fear, buy strictness.
- Right-sizing is not weakening: never lower an assertion, threshold or seed
  count to make a failing change pass, and never retry a flake to green.
- Delete or merge on sight: tests whose title names no guarantee, duplicates
  of a kept test, tests of removed features, implementation-detail asserts,
  stale snapshots, and checks that have only ever failed because a change
  legitimately moved data. Name the kept test that still protects the
  guarantee, or say the guarantee is one nobody cares about.

## Remove the need first
- Change the design so the bug can't be written: one table over every case
  instead of per-case copies, types that rule out bad states, one source of
  truth generating the rest.
- Keep logic in a pure deterministic core fed time, randomness and peers as
  inputs; test through the caller's interface and fake only what we don't own.

## Five kinds
1. **Recorded scenarios**: real or seeded input through the whole system,
   checking invariants and agreement; keep one injected fault that must trip it.
2. **Properties over a pure core**: generated inputs against bounds, round-trips
   or a reference model; locally a fixed seed and a few hundred cases;
   wide search after landing; keep shrunk failures named.
3. **Product measurements**: owner-decided numbers with their `Provenance:` record ([program-stewardship](../../../agent-machinery/skills/program-stewardship/SKILL.md) 6); a missed soft one lands with `Target-debt:`.
4. **External references**: truth from a spec, reference implementation or
   recorded real output, cited in the title or fixture.
5. **One integration check per real boundary**, run end to end.

Agents verify every mechanical check themselves on VM pairs and native capture (input, sizes, frame timing, a sound on its frame, a build in the game list, menus), never through Tom.

## Tiers
- **Edit loop (under ~60 s, local)**: type-check plus the tests of what
  changed. This is what a worker runs before pushing.
- **Landing gate**: tests whose inputs changed, one seeded representative of
  each sweep family whose guarantee no ordinary test covers, and one
  seconds-long end-to-end path; fast enough to land changes 1.5x faster than
  they arrive. A new sweep runs after landing unless it earns a gate slot.
- **After landing**: every sweep, full roster and wide property search on each
  landed SHA, on runners that actually exist; bisect a failure, confirm it on
  the parent, auto-revert the culprit. A tier nobody runs protects nothing:
  check it ran before moving work into it.
- **Nightly / manual**: soaks, native pairs, cost censuses.

## Committed records
- A committed snapshot, baseline or record that unrelated changes make stale
  costs every lane a refresh; that is churn, not a guarantee. Compute it on
  CI from the last green run, key it per row so only the rows a change touched
  move, or keep only the absolute bound and drop the drift record.
- A record no check reads is deleted.

## Delete on sight (largest first, from seven repository reviews)
Example matrices re-testing one rule a property should own; our own prose,
constants or argv asserted verbatim; one scenario rebuilt per layer or run
twice; count pins on fixtures, catalogs or test totals; suites no runner
collects; sleeps used as coordination; fakes of our own code; oracles that pass
when nothing was checked; source greps and tombstones; single bugs pinned as
their own tests; asserts of what the types guarantee; live probes committed as
tests; full-width sweeps at the landing gate. Count cases with the runner, never
a regex. Counts per class: `references/sources.md`.

## Reference grounding
Per `references/domains.md`, use throughput to find 2-3 high-throughput projects
in the domain, newer and AI-heavy first; study their test and landing architecture
and accepted trade-offs, and record how we could get there. Numbers only select.

## Mutation
- A mutant settles doubt about whether a test is the only one protecting a
  guarantee; it is not a toll on every deletion. Mutate only in a scratch copy,
  never the lane, and mark every mutated line `MUTANT:<rule-id>`; `safe-push`
  refuses an added line with that token.
- Run `mutant record <id>` in the scratch copy before claiming a kill; an empty diff refuses, and `safe-push` requires the recorded patch to apply to the landing base or head.
- Product code never branches on a test's literal inputs.
- `safe-push` refuses an added `skip`, `only`, `#[ignore]` or `xfail` without a `Test-change:` trailer naming the changed requirement, and an added test without a `Test-kind: <name> <kind> kills <mutant>` trailer.
- A `.safe-push` ci.budget may name a ceiling after its target; a CI wall median between them lands and is logged as debt. While the median exceeds the ceiling, or the target when none is set, `safe-push` refuses a landing unless a speed-up carries `CI-budget: <workflow> <before>s -> <after>s` or, while main is red, every commit names the failing run in `Main-red: <run id>`.

## Landing fast
- **Reuse every build and test result whose inputs are unchanged**, keyed by a
  content hash of those inputs. Rebuilding or rerunning unchanged inputs is waste;
  so is running the same suite twice for one landing.
- Call an external tool once per batch, not once per item (file, row, test,
  branch); fall back per item only for exact diagnostics on failure.
- Bound cost in a deterministic unit (instructions, frames, allocations), never
  wall seconds; shrink inputs or move work after landing instead of a ceiling. Run a timing check once, under an exclusive machine-capacity lease.
- Quarantine a flake at first sight with its seed, log and an issue; fix or
  delete within a week. Never retry to green, sleep or weaken an expectation.
