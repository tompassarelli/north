# Change graph replay: carry-over, missed tests and blame on 2026-11-08's queue

The change graph (north#917) answers three questions about a lane: which tests its change can reach (`north graph ask
affected`), which lanes share a reachable test (`overlap`), and which lanes a failed test or compile error points at
(`blame`). This note records how those answers scored when replayed against the north landing queue of 2026-11-08,
run with the landed queries (0b858b9384) and edges (e9412f10a4). It matched the replay run on the unlanded code exactly.

## Inputs

- The north queue from midnight to 11:20 AM +07: 274 landed lanes in 96 pushes, each lane's head and the merged tree
  it landed in, and 62 red probes with a failed test or compile error in their check logs.
- Ground truth: a failure's culprit is *verified* when the lane's withdrawal or refusal names that failure (50
  failures), and *inferred* when it is the only withdrawn or refused lane in its probe (23 failures).
- Edges: the lander's coverage map (2026-11-06, 774 tests), the crate graph at origin/main, and per-test read sets
  recorded on a build host at main bde3db9c68 (1,151 of 1,166 tests; the 15 north-land-sandbox tests run unrecorded
  by design).

## Carry-over: alone verdicts that still hold on the merged tree

For each landed lane, the share of its alone-checked tests that no other lane in its merged tree can reach, so the
lane's alone verdict on them holds after the merge:

- 245 of 274 lanes were traced. The other 29 change flake.nix, which the graph counts as a toolchain change that
  reaches every test.
- Mean carried share: 53.1% over all lanes, 59.4% over traced lanes. In 25 traced lanes, no other lane in the batch
  reaches any test in the lane's cone.
- Carried tests that failed in a real probe: **0**. Of the 607 cases where a probe failed a test that a lane other
  than the culprit had passed alone, 594 were not carried (the graph saw another lane reach the test) and 13 were
  untraced. None was a carried test.

## Missed tests

For each verified culprit, the test it broke is in the set `affected` returns for its own commit: 13 of 13, with no
miss and no untraced answer.

## Blame

Per failure, the culprit compared with the lanes `blame` names out of the lanes in that probe:

- Verified ties: the culprit was among the named lanes 50 of 50 times. In 15 it was the only lane named; in 35 it was
  one of several.
- Inferred ties: 10 exact, 9 among, 4 where blame named other lanes, and 4 where blame named no lane.
- Across the 50 verified ties, blame named 3.4 lanes on average out of 14.3 in the probe: 2.3 of 13.8 for test
  failures, and 10.1 of 17 for compile errors, where every lane that changes the file or a crate it depends on stays
  named.

The withdrawn lanes this replay was asked about:

- n882-climb, which broke the runtime reuse test: named alone (1 of 23 lanes), both times.
- n842-groups, which failed to compile north-lander: named among 2 of 2, 8 of 14, 10 of 16 and 11 of 23.
- n903-losses: named alone (1 of 23) twice, and among 2 of 38 twice.
- n909-relay: named among 2 of 38, twice.
- n906-payoff: no failed test or compile error was tied to it. The tool-removal gate refused it, a check on its own
  diff that runs before the probe's tests. It appears only as one of 11 lanes named for a north-lander compile error
  whose culprit was n842-groups.

## Selection on the live graph

At 2:00 PM +07 the laptop's graph.db, the one the lander's land check asks, held the crate facts (`crates`, writer `cargo`)
but no recorded reads: the gex45 recording sat in `graph-917/north`, outside the `land-reuse` and `verdict-cache/v2`
paths the `reads` source reads. 9 of the last 20 lanes on main changed a non-Rust path and answered untraced, so their
land checks kept the crate-level selection. With the recording placed in those paths and ingested (25,698 facts), and
each recorded test told `:recorded-in` its binary so a test coverage never reached is still known:

- The 20 lanes on main from 2649c5a3a2 to f3b0513fa0: 20 of 20 traced, where 11 traced before. Over all 274
  lanes of the morning's queue, each lane's own commit: 273 traced, the other changes `flake.nix`.
- Missed tests: the 13 failures of verified culprits above, and the 2 tests that failed a lane alone after 11:20 AM
  (n842-classify's habits test, n892-hostfault2's carry matrix test), are each in the lane's own set: 15 of 15.
- The after-landing runs at 2:03 PM ran graph selections, f3b0513fa0 39 tests of the 243 in the binaries it built.

## What follows

- Blame runs before the bisect in the after-landing culprit search: one lane named reverts it, several bisect only
  among them, untraced bisects the whole batch (north#902, worker q902-gblame).
- `affected` narrows each land check's test selection (north#902, worker q902-gselect).
- Scoping a merged-tree run to the lanes' overlapping tests waits until each lane's own reachable tests run before it
  queues: under land-first the merged run is the first place a lane's tests run, so a run limited to the overlap
  would skip them (north#902).

## Tests before queueing (north#943)

Before north#943 the checks before queueing were clippy and the `@enqueue` scans; no test ran until the after-landing
run on the merged tree, so a lane that broke its own test landed and was reverted. When `north graph ask affected` traces
a lane, safe-push now adds the nextest land check to those checks, and it runs the graph's selection: the tests the
change can reach, the tests the lane adds and the tree-wide tests/it modules, logged as "the N of the M tests the change
graph knows". An untraced lane (a toolchain input, or an answer with no runnable test) runs the crate-level fallback with the tree-wide
modules and the unit tests in tests/it/tree_wide_units.txt: queueing it with no test let agent_text_reads go red on main
after 05b4a73da2. The after-landing run still checks every landed batch. A check before queueing has no landing round, so a failure there is not rerun on the base tree: on 2026-11-08 the first landing (5b279d03ba) was reverted when main went red on agent_text_reads, a tree-wide module every selection includes, and refused n902-treewide for main's failure; it was landed again once main was green, and 06624105dd judges a failing test against the lane's base before queueing. The 15 of 15 failures above were each in the failing lane's own set,
so each would have stopped its lane before it queued.
