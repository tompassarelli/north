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

## Delete on sight (counts across seven repository reviews, 11 Oct)
- Our own prose, constants or argv asserted verbatim: ~125 asserts, 46 files.
- Example matrices re-testing one rule a property should own: ~320 cases.
- One scenario or artifact rebuilt per layer or file, or run twice: ~35 sites.
- Count and census pins on fixtures, catalogs, test totals or own cost: 17 files.
- Suites no runner collects: 18 files, plus 47 that run only on one machine.
- Wall-clock sleeps used as coordination: 11 files and 3 workflows.
- Fakes of our own code or binaries: 13 files.
- Weak or correlated oracles that pass when nothing was checked: 11 files.
- Source greps, tombstones and code lifted out by text: 10 files, 36 asserts.
- Review findings or single bugs pinned as their own tests: 15 tests.
- Asserts of what the type checker guarantees, or of internals: 13 asserts.
- Live probes or machine state committed as tests: 5 files.
- Full-width sweeps in the local suite: 101 of 132 s in one repository.
Count with the runner, never a regex: two "empty file" censuses (18 and 34
files) were wrong because registries and `test.each` hide the cases.

## Mutation
- Mutate only in a scratch copy or throwaway worktree, never the lane. Every
  mutated line carries the token `MUTANT:<rule-id>`, so a leaked mutant cannot
  land.
- A unique kill protects only the rules the mutant list covers. Before deleting
  a test, write a mutant for every rule it exercises; a sample is not enough.
- Report mutants killed per kept test, and CPU per test before and after.

## Gaps and the checks that hold them
- No green by tampering: `safe-push` refuses an added `skip`, `only`,
  `#[ignore]` or `xfail` in a test file unless a commit has a `Test-change:`
  trailer naming the changed requirement. Deleting a test needs no trailer.
- Every kept test fails when its unit breaks: the unique-kill mutant run above;
  no CI mutation gate yet.
- Every test file runs: north's `socrates.yml` collects every `*.test.sh` and
  `*.test.py` by glob, minus its listed machine-bound files.
- Product code never branches on a test's literal inputs: review only.
- Mutants stay in scratch copies: `safe-push` refuses any added line carrying
  `MUTANT:` followed by a rule id.

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
  wall seconds; wall clock is only a generous hang timeout. Shrink inputs or
  move work to CI instead of raising the ceiling; print the seed on failure.
- Quarantine a flake at first sight with its seed, log and an issue; fix or
  delete within a week; cap the quarantine at 8. Never retry to green, add sleeps or weaken an
  expectation.

Sources: `references/sources.md`, read only when revising a rule.
