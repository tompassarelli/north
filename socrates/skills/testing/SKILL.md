---
name: testing
description: >-
  Write, change, delete, run or speed up automated tests in any project: designing
  a test away, what earns a test, property, replay and golden tests, CPU cost,
  flaky tests, pruning a suite, landing gates and reuse by input hash.
grounded: 2026-10-11
written: 2026-10-11
metadata:
  kind: playbook
---
# Testing

**The burden is on the test.** A test stays only when it is one of the five
kinds below and kills a mutant no other kept test kills; everything else is
scaffolding, deleted before landing and on sight in old suites.

## Remove the need first
- Before writing or keeping a test, change the design so the bug can't be
  written: one table over every case instead of per-case copies, types that
  rule out bad states, one source of truth generating the rest.
- Keep logic in a pure deterministic core fed time, randomness and peers as
  inputs; test through the caller's interface and fake only what we don't own.
- A missed bug extends the property, scenario or reference that should catch it.

## Five kinds
1. **Recorded scenarios**: real or seeded input through the whole system,
   checking invariants and agreement; keep one injected fault that must trip it.
2. **Properties over a pure core**: generated inputs against bounds, round-trips
   or a reference model; wide search after landing; keep shrunk failures named.
3. **Product measurements**: owner-decided numbers with their source.
4. **External references**: truth from a spec, reference implementation or
   recorded real output, cited in the title or fixture.
5. **One integration check per real boundary**, run end to end.

Agents verify every mechanical check themselves on VM pairs and native capture (input, sizes, frame timing, a sound on its frame, a build in the game list, menus), never through Tom.

## Delete on sight (largest first, from seven repository reviews)
Example matrices re-testing one rule a property should own; our own prose,
constants or argv asserted verbatim; one scenario rebuilt per layer or run
twice; count pins on fixtures, catalogs or test totals; suites no runner
collects; sleeps used as coordination; fakes of our own code; oracles that pass
when nothing was checked; source greps and tombstones; single bugs pinned as
their own tests; asserts of what the types guarantee; live probes committed as
tests; full-width sweeps in the local suite. Count cases with the runner, never
a regex. Counts per class: `references/sources.md`.

## Reference grounding
Per `references/domains.md`, use throughput to find 2-3 high-throughput projects
in the domain, newer and AI-heavy first; study their test and landing architecture
and accepted trade-offs, and record how we could get there. Numbers only select.

## Mutation
- Mutate only in a scratch copy, never the lane, and mark every mutated line `MUTANT:<rule-id>`; `safe-push` refuses an added line with that token.
- Run `mutant record <id>` in the scratch copy before claiming a kill or deleting a test; an empty diff refuses, and `safe-push` requires the recorded patch to apply to the landing base or head.
- A unique kill protects only the rules the mutant list covers. Before deleting a test, write a mutant for every rule it exercises; a sample is not enough.
- Product code never branches on a test's literal inputs.
- `safe-push` refuses an added `skip`, `only`, `#[ignore]` or `xfail` without a `Test-change:` trailer naming the changed requirement, and an added test without a `Test-kind: <name> <kind> kills <mutant>` trailer.
- While the repository CI wall median exceeds its `.safe-push` ci.budget, `safe-push` refuses a landing unless a speed-up carries `CI-budget: <workflow> <before>s -> <after>s` or, while main is red, every commit names the failing run in `Main-red: <run id>`.

## Landing fast
- **Reuse every build and test result whose inputs are unchanged**, keyed by a
  content hash of those inputs. Rebuilding or rerunning unchanged inputs is waste.
- Gate a landing on checks whose inputs changed plus one seconds-long end-to-end
  path, fast enough to land changes 1.5x faster than they arrive.
- After every landing run the full suite, sweeps and wide property search;
  bisect the failing shard, confirm it on the parent, auto-revert the culprit.
- Bound cost in a deterministic unit (instructions, frames, allocations), never
  wall seconds; shrink inputs or move work after landing instead of a ceiling. Run a timing check once, under an exclusive machine-capacity lease.
- Quarantine a flake at first sight with its seed, log and an issue; fix or
  delete within a week. Never retry to green, sleep or weaken an expectation.
