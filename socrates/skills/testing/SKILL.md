---
name: testing
description: >-
  Write, change, delete, run or speed up automated tests in any project: what
  earns a test, scaffolding versus durable tests, property, replay and golden
  tests, CPU cost per test, flaky tests, pruning a suite.
grounded: 2026-10-11
written: 2026-10-11
metadata:
  kind: playbook
---
# Testing

**The burden is on the test.** A test stays only when it is one of the five
kinds below and kills a mutant that no other kept test kills. Everything else
is scaffolding: delete it before landing, and delete it on sight in old suites.
Default to no new test; local contracts belong in types and assertions.

## Five kinds
1. **Recorded scenarios**: real or seeded input through the whole system,
   checking invariants and agreement; keep one injected fault that must trip it.
2. **Properties over a pure core**: generated inputs against bounds,
   conservation, round-trips or a reference model; fixed seeds locally, wide
   search in CI; keep a shrunk failure as a named case.
3. **Product measurements**: owner-decided numbers with their source.
4. **External references**: truth from a spec, reference implementation or
   recorded real output, cited in the title or fixture.
5. **One integration check per real boundary**, run end to end.

## Delete on sight (largest first, from seven repository reviews)
Example matrices re-testing one rule a property should own; our own prose,
constants or argv asserted verbatim; one scenario rebuilt per layer or run
twice; count pins on fixtures, catalogs or test totals; suites no runner
collects; sleeps used as coordination; fakes of our own code; oracles that pass
when nothing was checked; source greps and tombstones; single bugs pinned as
their own tests; asserts of what the types guarantee; live probes committed as
tests; full-width sweeps in the local suite. Count cases with the runner, never
a regex. Counts per class: `references/sources.md`.

## Mutation
- Mutate only in a scratch copy, never the lane. Every mutated line carries
  `MUTANT:<rule-id>`; `safe-push` refuses an added line with that token.
- A unique kill protects only the rules the mutant list covers. Before deleting
  a test, write a mutant for every rule it exercises; a sample is not enough.
- `safe-push` refuses an added `skip`, `only`, `#[ignore]` or `xfail` in a test
  file unless a commit carries a `Test-change:` trailer naming the requirement.
- Product code never branches on a test's literal inputs.

## Design for fewer tests
- Put logic in a pure, deterministic core (state + input → state); time,
  randomness and peers are its inputs. Side effects stay in a thin typed shell.
- Test through the interface a caller sees. Fake only what you don't control
  (clock, network, devices), fed to the core as input.
- No regression test for a bug fix unless the five kinds missed it; then extend
  the property, scenario or reference that should have caught it.

## Running and flakes
- On save: type check, affected tests and one fast end-to-end path. Every push
  runs the suite plus sweeps and properties on CI or the farm.
- Bound cost in a deterministic unit (instructions, frames, allocations), never
  wall seconds. Shrink inputs or move work to CI instead of raising a ceiling.
- Quarantine a flake at first sight with its seed, log and an issue; fix or
  delete within a week. Never retry to green, add sleeps or weaken an
  expectation.
