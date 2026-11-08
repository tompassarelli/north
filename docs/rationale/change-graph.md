# Change graph: which tests a lane can affect, built over Git

Tom asked on 2026-11-08 to build the change graph for landing now, alongside the lander fixes (north#917): edges in
north-graph from a change to the tests it can affect, so the lander runs only those tests, scopes the check of a
merged tree to where lanes' cones overlap, and blames a failure on one lane.

## Decision: stay on Git, build the semantic layer over it (north#918)

Tom chose on 2026-11-08 to keep Git as north's storage and interchange format and build the semantic layer over it,
rather than move to a graph-store version-control system in its own repository. The lander review of the same day
(south agents/decisions/harness/lander-review-2026-11-08.md) found landing bounded by batch width, lanes that never
passed alone, colliding edits and conflict resubmissions (85 of 582 tickets in 24 h ended as conflicts, mostly
both-sides appends to list-shaped files), not by Git storage. Git is also the interchange format for GitHub, Actions,
Nix flakes, Cargo and every agent's tooling, so a new store would cost months before it moved the landing number.

Reopen the question when, with the change graph and the lander's semantic merge of list-shaped files live, either:

- conflict endings stay at 5% of tickets or more over 24 h (14.6% on 2026-11-08) and a definition-level merge would
  have resolved at least half of them, read from the queue's `done/` conflict verdicts and their hunks; or
- Git's own operations (fetch, rebase, merge, push) take 10% or more of the landing queue's 1 h p90, read from the
  queue's `logs/spans.jsonl`.

## Edges, not a Beagle port

The same review judged which edges matter for a change: the crate graph for what compiles, function coverage for
code a test runs, and recorded file reads for data a test reads. Beagle's cones (bin/test/branch-compile-corpus) are
compiler invalidation cones over Beagle's own language: they hold no test edges and no runtime reads, so they are
the model for definition identity and early cutoff, not an edge source. The day's most frequent test breaker,
n706-cost-words, changed only `harness/defaults.toml` and broke five north-fleet-supervisor `gate_rows` tests that
no code edge reaches; only a recorded read links them.

## Edges

north-graph holds three kinds of edge for a change, each told by the writer that knows it (docs/graph.md, Ingest):

- code a test runs: `(test :reaches fn:FILE:START-END)` from the coverage map (writer coverage);
- data a test reads: `(test :reads FILE)`, `:lists DIR`, `:expects-absent FILE` and one `:read-set SETID` per
  recorded test, from the verdict-reuse recorder's per-test path sets (writer recorder). Reads outside the tree are
  left out: every recorded test makes them (terminfo, the loader probing `target/`, `$HOME` state), and a lane's diff
  changes only tree paths;
- what compiles with what: `(crate:A :depends-on crate:B)`, `(crate:A :root DIR)` and `(SRC :embeds FILE)` for each
  `include_str!` and `include_bytes!`, read from the manifests at `origin/main` (writer cargo).

`north graph ask affected|overlap|blame` answer from those edges (docs/graph.md, Change queries). `Cargo.lock` and the
root `Cargo.toml` are diffed by package and by table, so a lane that adds a crate or bumps one dependency falls back to
the crates that use it; `flake.nix`, toolchain files and any other root table stay untraced.

## Replay of 2026-11-08

Data: the north land queue from midnight to 11:20 AM (laptop time): 274 landed lanes in 96 pushes, 62 red probes with
a failed test or compile error in their check logs, each ticket's head and verdict, and the land.batch decision log
for each probe's lanes. Edges: the coverage map the lander had (2026-11-06, commit 900c3793, 774 tests), the crate
graph at `origin/main`, and per-test read sets recorded once on gex45 at main bde3db9c (1151 of 1166 tests; the 15
north-land-sandbox tests run unrecorded by design). Code: main with the edge and query lanes of north#917 applied.
Ground truth: a failure's culprit is verified when the lane's withdrawal or refusal names that failure (50 failures),
and inferred when it is the only withdrawn or refused lane in the probe (23 failures).

- **Carry-over.** For each landed lane, a test's alone verdict carries to the merged tree it landed in when no change
  between the lane's head and that tree reaches the test. With the map the lander had, 53.1% of the 1151 tests carry
  over all 274 lanes (59.4% over the 245 the graph traces); with a map recorded after the day (1150 tests reaching
  38,083 functions at c2fe2dec56), which sees more of the code the day added, 38.2% (42.7%). 29 lanes are untraced
  because `flake.nix` changed in between. Of the 607 pairs of a failed test and a lane in the same red probe that did
  not cause it, 594 were not carried and 13 were untraced under either map; none carried a verdict the probe
  contradicted. The lander review set the bar for building the 60 s path on carried verdicts at half of checks with no
  disagreement over 100 landings: met with the stale map, not with the fresh one, so the day does not settle it.
- **Overlap.** On the 40 multi-lane red probes (9.8 lanes on average), the tests in some pair of lanes' cones average
  477 of 1151 (41%): the share of the suite a merged-tree check needs when every lane has passed alone. Lanes rarely
  fall into separate groups: 9 tests read at least 95% of the 2122 Rust files (the Rust-habits, embeds and agent-text
  scans), and end-to-end tests such as golden_path run most of the lander and fleet code the day's lanes edited.
- **No missed test.** For every failure with a verified culprit, the culprit's own affected set holds the failed test,
  through a recorded read, a coverage region or a test the lane adds (13 of 13 culprit and test pairs). No test failed
  after a landed lane: the 74 after-landing runs were green and the one red was a graph-guard check fixed at the tip.
- **Blame.** On the 50 verified failures the culprit is always among the graph's candidates, and the candidates are
  exactly the culprits for 15 (30%). Candidates average 3.4 of 14.3 lanes per probe: 2.3 of 13.8 for test failures
  and 10.1 of 17 for compile errors, where every lane changing the file or a crate it depends on stays a candidate.
  n882-climb is named alone in both red probes it caused, through the embeds test's read of lineage.rs. n903-losses is
  named alone where it rode without n909-relay, and the pair exactly where both rode a 38-lane probe. n842-groups is
  among 2 to 11 candidates of 2 to 23. n906-payoff failed the tool-removal gate, which judges the lane alone. On the 23
  inferred failures the culprit is among the candidates for 19. The other 4 are compile errors in files that one lane
  added, where blame named the lane on the other side of the interaction or gave no answer.

The first run of this replay found six faults in the queries, each fixed before landing: blame dropped untraced lanes,
ignored a lane's own new test, read only part of a test's log block, let a block run into the next check's output and
counted the panic location as a named path; and, read whole, `Cargo.lock` untraced 155 of the 274 carry checks
and then the root `Cargo.toml` 68, until each was diffed by package and by table.
