# Pruning tests and records

Read when deleting, merging or auditing tests or a suite, or adding, refreshing or deleting a committed snapshot, baseline or record.

## Delete on sight
- Delete or merge on sight tests whose title names no guarantee, duplicates, tests of removed features, implementation-detail asserts, stale snapshots and checks that only ever failed when data legitimately moved; name the kept test that still protects the guarantee, or say nobody cares about it, so no guarantee is lost silently.
- Largest classes first, from seven repository reviews: example matrices re-testing one rule a property should own; our own prose, constants or argv asserted verbatim; one scenario rebuilt per layer or run twice; count pins on fixtures, catalogs or test totals; suites no runner collects; sleeps used as coordination; fakes of our own code; oracles that pass when nothing was checked; source greps and tombstones; single bugs pinned as their own tests; asserts of what the types guarantee; live probes committed as tests; parity checks against code that is deleted or now an exec; full-width sweeps at the landing gate. Count cases with the runner, never a regex, because regex counts were refuted by runners. Counts per class: [testing sources](testing-sources.md).

## Admission applies to what stays
- Re-argue any test you touch against the admission rule in [writing tests](writing-tests.md#admission) and delete it if no named bug, recorded mutant or external contract holds it, because a carried-forward test costs every landing and verification until someone argues for its deletion.

## Committed records
- A snapshot, baseline or record that unrelated changes make stale costs every lane a refresh, so compute it on CI from the last green run, key it per row so only touched rows move, or keep only the absolute bound and drop the drift record.
- Delete a record no check reads, because it protects nothing.

## Mutants, on demand
- A mutant settles doubt about whether a test is the only one protecting a guarantee; it is not a toll on every deletion.
- Mutate only in a scratch copy, never the lane, and mark every mutated line `MUTANT:<rule-id>`, because reviewers have committed product mutants; the landing tool refuses an added line with that token.
- Check a doubted kill on demand: `mutant record <id>` in the scratch copy, then `mutant verify BASE HEAD`, which refuses an empty record or a patch that applies to neither side and, where the repository configures a mutant runner, requires the claimed test to pass on the change and fail on the mutant.
