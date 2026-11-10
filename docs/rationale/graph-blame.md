# Graph blame: the graph names the culprit, bisection is the fallback

Tom set the direction on 2026-11-11 (north#1039): blame for a red after-landing run should be near-instant, bisection
retired wherever the change graph can answer and informed by it where it cannot. This plan orders that work. Each
step names its cost, the bar it is measured by and the box on north#1039 that builds it. Step 1 is the n1039-blame box
already in progress; the others build on it.

## What the lander does now

`locate` (`crates/north-landing/src/land/post_land.rs`) asks `north graph ask blame` for suspects. It then searches
prefixes of the batch: `blame::culprit` when one check failed, `blame::culprits` when several did
(`crates/north-landing/src/land/blame.rs`). Three reds on the morning of 2026-11-11 (+07, Tom's laptop time) show
where that goes wrong. The logs are in `/tmp/land-lead-fixtures/` on cpx62b; the originals are on whiterabbit under
`~/.local/state/north/land-queue/de2e0e8479546ba9/logs/` and `~/.local/state/safe-push/`.

- **4:19 AM, one lane (n722-review, cdcb3cf776).** The after-landing run failed north-ops `lane_gc_reaping
  a_lane_with_a_recorded_rejection_is_bundled_and_reaped_and_a_half_removed_one_is_archived` in 0.25 s. At 4:19:08
  the lander logged "the change graph names no culprit ... so the whole batch is bisected". The base afbb3e19ce ran
  green at 4:23:32. `at_once` then took prefix 1, which is the head itself, as red without running it, and named
  n722-review on `land-check-mLf4WYGhXy.log`, the batch's own run from 4:19:03. The revert was not queued only because
  it no longer applied cleanly. The test was at fault; a separate worker is fixing it.
- **4:54 AM, one lane (n1081-canary, bd34b8ad5f).** The run failed north-base `platform::ssh_master::tests::
  a_master_slower_than_three_seconds_while_a_direct_connection_answers_is_closed_and_recreated` in 0.32 s. The test's
  health read was `Answered(144.094256ms)`, so the stall it sets up never happened. The graph named no culprit
  (4:57:16), the base deb4f2cb8e ran green (5:01:32) and the lane was named red on `land-check-zJuXk8pFoq.log`, again
  the batch run. The lander's own reruns of that test on 22803418f (5:02:25) and a70ac5dd (5:03:39), both holding the
  lane, passed. The test also passed 5 of 5 on the base on cpx62b at load 14 to 20. The lane changes no ssh_master
  code.
- **5:14 AM, three lanes (21746f8485..6179709333).** `land scan test-admission kills` was rerun over the whole batch
  range. It paired n1022-old-state's three added tests with ten mutants in
  `crates/north-dispatch/src/dispatch/survey/mainline.rs`, a file only the dispatch-one-deletion-read lane touches. The
  graph named no culprit (5:14:43), and the lander bisected the batch for a fault that no lane has. The check itself is
  being fixed under a north#892 box (worker cpx62-n892-kills-batch).

In all three, a lane is convicted on a run it never had. A one-lane batch is "bisected" by greening only its base,
because the one prefix to check is the head, and both search functions treat the head's red as already known.
`culprits` even seeds it ("costs no run"). For a flaky test, base green plus head red is exactly what a flake looks
like. In the first two reds, the right answer was no blame: rerun the test on the same tree, record the flake and
file it.

## Why the graph named no suspect, and when that can be trusted

Replayed on cpx62b on 2026-11-10 with today's graph (`north graph ask blame --json --log LOG LANE=BASE..HEAD`):

- **Red 1: the map lacked the edge.** The graph *named* n722-review, through the fallback for unknown tests
  (`by_failure` holds the lane). n722-review changes only three north-threads files. The lane-reaping test is not in
  the coverage map. Because north-ops depends on north-threads, `cone::unknown`
  (`crates/north-graph-change/src/cone.rs:162`) adds every unmapped test of every dependent crate to the lane's
  `added` list. So the graph could not rule the lane out, which is a weak answer, not a measured edge. The lander then
  folded it into "no culprit": `blame::suspects` returns `Batch` when the named lanes are all the lanes, and in a
  one-lane batch that is any named lane. The log line cannot tell "no lane reaches the test" from "the only lane is
  named".
- **Red 2: the graph was right.** `blame` exits 3: no lane's cone holds the test. The test is known to the map
  (`affected` over e3cf5bb81, the last ssh_master change, selects it by name). The lane's Rust paths (north-session-host
  and the root package) are all downstream of north-base, so no change of theirs is compiled
  into a north-base unit test. The lane's other paths (`structure.toml`, `structure/`, docs) are not recorded reads of
  the test.
- **Red 3: the check was not a test of the batch.** A lane-scoped check that pairs one lane's tests with another
  lane's mutants is wrong on any range wider than one lane. Its red is not evidence about the batch, so it gets no
  blame at all (step 0).

"No suspect" is trusted when all of these hold. Under them the graph has ruled out every way the batch's diff can
reach the test, through code it runs, data it reads, or what compiles into its binary:

- every failure is a test, not a compile error;
- every failed test is known to the graph (`:in-binary` or `:recorded-in`) and has a `:read-set`;
- the coverage commit is in the repository and no more than 3 days old, the same horizon as selection;
- no lane in the batch is untraced.

What stays unmodelled is reads outside the tree: `$HOME` state, other processes, the network, timing and load. Those
are the flake and environment classes, and step 2's rerun on the same tree decides them. When a trusted "no suspect"
fails again on that rerun, main is red for a reason outside the diff, and the sheriff's existing Environment finding
reports it to the proxy. When the graph names lanes only through the unknown-test, crate or untraced fallback, the
answer is a suspect list, not a trusted "no suspect" and not a measured edge. The confirmation runs decide it.

## The order of a red

1. Admit: a red from a lane-scoped check enters no blame (step 0).
2. In parallel: ask the graph (0.4 s), and rerun the failed tests on the red tree on the host that built it.
3. If the rerun passes, the test failed and passed on the same tree hash. That is a flake: no lane is blamed or
   reverted (step 2).
4. If the rerun fails, the graph's answer decides:
   - **No suspect:** a main breakage. Nothing is reverted, and the sheriff reports it.
   - **One suspect:** one run on its parent confirms it (step 1).
   - **Several suspects:** each suspect is probed alone (step 3).
   - **Every suspect green alone:** an interaction (step 4).

Prefix bisection over the whole batch is left only for an untraced batch and for a compile error whose suspects
exceed the free probe slots.

## Steps

Every step runs only the failed tests, so a probe costs a build of its tree plus the tests' own seconds. The full
after-landing run takes 315 to 418 s p90 over 20 passes (lander journal, 2026-11-11), and in both fixtures 4 min 23 s
to 4 min 25 s of it went to the base probe.

### Step 0: lane-scoped checks never enter blame (box n1039-blame-scope)

Lines in `.safe-push` fall into three classes for the after-landing run:

- **Tree checks.** The verdict is a property of the tree: the nextest lines, clippy, whole-tree habits, citations,
  check-owners, graph guards, the verbs and `tests/it` list checks, tree-policy, nix-entry, safe-push-verbs, check
  experiments and the nix lock check. These are batch-valid, and their reds go through the graph.
- **File-attributed range checks.** Each finding names a file: `code-health layers BASE HEAD` and `code-health habits
  --base/--new-since`. These are batch-valid, and the culprit is the lane that changed the named file. No probe is
  needed.
- **Lane-scoped range checks.** These pair items across the range or replay a lane's added tests against BASE:
  `land scan test-admission kills`, `land scan test-admission run BASE..HEAD` and `land scan gate-shadow`. A batch
  range is not a valid input for them. A line marks this with an `@lane` tag beside `@enqueue`. The after-landing run
  runs a tagged line once per landed lane's own range (`base..tip_0`, `tip_0..tip_1`, ...), and a red names that range's
  lane directly. Wherever the n892 kills box lands its per-lane run, this step only adds the tag and keeps tagged reds
  out of `locate`.

Cost: no probe at all. Building it is about 1 h of agent time. Bar: the share of after-landing reds settled with no
bisection round, which red 3 would have raised. Check: `cargo nextest run -p north-landing -E
'test(/a_red_lane_scoped_check_over_a_batch_range_enters_no_blame/)'`. Needs: none. Order: independent.

### Step 1: one suspect, confirmed by its parent and itself (box n1039-blame, in progress)

The lanes whose diff touches a file or item the failed test reaches are the suspects. One suspect is confirmed by one
run of the failed test on its parent and one on its own tree, then reverted, with no bisection. Its interface has to
grow in four places, which the later boxes build:

- `blame::suspects` must keep a named lane when every lane is named. Otherwise a one-lane batch never has one suspect,
  and neither fixture reaches this step (box n1039-blame-trust).
- `north graph ask blame --json` must say, per failure, whether the test is known to the graph, and per lane which
  edge holds it: `reaches`, `reads`, `added`, `unknown`, `crate` or `untraced`. The lander needs that to tell a trusted
  "no suspect" from an untraced one, and a measured suspect from a fallback (box n1039-blame-trust).
- The run "on it" for the last prefix is a fresh run of the head's tree, not the batch's log (box n1039-blame-rerun).
- `blame::Probe` must carry the probed tree's hash, so a pass and a fail can be matched by tree (box n1039-flake).

Cost: 2 probes, the head's on a warm build. Bars: time to a named culprit, and 0 innocent reverts.

### Step 1a: a trusted answer from the graph (box n1039-blame-trust)

The graph's blame JSON reports, per failure, `known` (the test has `:in-binary` or `:recorded-in` and a `:read-set`) and
the edge for each lane that holds it. `blame::suspects` returns three cases:

- **Named:** a list of lanes, kept even when it is every lane.
- **NoSuspect:** exit 3, with every failure known, the map fresh and no lane untraced.
- **Untraced:** anything else.

The lander's line names which case it saw and why. For red 1 that is "n722-review alone, through the unknown-test
fallback for lane_gc_reaping"; for red 2, "no lane reaches the known test ssh_master::...". Cost: one 0.4 s query, as
now. Building it is about 1.5 h of agent time. Check: `cargo nextest run -p north-landing -E
'test(/a_one_lane_batch_whose_lane_the_graph_names_is_one_suspect/)'`. Needs: none. Order: after n1039-blame lands,
since both edit `blame.rs`.

### Step 1b: no lane is convicted on the batch's own red log (box n1039-blame-rerun)

This is the north#892 box "The lander blames a lane for a red after-landing run only on a fresh run of that lane's
prefix", which moves here. Before any lane is named, the failed tests rerun on the red tree, on the host whose build
is still warm. `at_once`, `bisected` and `culprits` take the head prefix's verdict from that rerun and never from the
batch run. Replaying red 2's one-lane bisection gives no blame until a rerun of the lane's prefix fails. Cost: one
probe on a built tree. The test's own time is 0.25 to 0.32 s in the fixtures; cargo's freshness check and nextest's
listing add a few seconds, and the first bar reading replaces this estimate. Check: `cargo nextest run -p north-lander
-E 'test(/a_one_lane_batch_is_blamed_only_on_a_rerun_of_its_prefix/)'`. Needs: none. Order: after n1039-blame.

### Step 2: no suspect, and the flake it proves (box n1039-flake)

When the rerun of step 1b passes, the test has failed and passed on one tree hash. The lander tells the graph
`(test:ID :failed-on TREE)` and `(test:ID :passed-on TREE)` (writer `lander`, the tree hash, not the commit). No lane is
blamed or reverted, and the red is settled as a named flake. With a trusted "no suspect" whose rerun fails again,
nothing is reverted either: the sheriff's Environment finding tells the proxy that main is broken. Both 2026-11-11
fixtures end here. Red 2's reruns on lane-holding trees passed, so one warm rerun on the red tree would have named
the flake instead of convicting the lane 4 min 23 s after the red. Cost: no probe beyond step 1b's rerun. Check:
`cargo nextest run -p north-landing -E 'test(/a_test_that_passes_on_the_tree_it_failed_on_is_recorded_flaky_and_blames_no_lane/)'`.
Needs: none. Order: after n1039-blame-rerun.

### Step 2a: quarantine and file the flaky test (box n1039-quarantine)

A test with a flake fact still runs after landing, and its result is still recorded, but its failure is never
blamed. It is filed once, deduplicated by test id, on an issue that its owning crate's lead picks up.

The quarantine leaves when a commit naming that issue lands and the test then passes 3 after-landing runs. This gives
an exit that the existing pre-landing quarantine lacks (`docs/rationale/state-machines.md`, entry 18). The cap is 8
quarantined tests, as in Fowler's quarantine rule (P3 in `harness/skills/verification/references/testing-sources.md`).
Past the cap, the proxy is told. This is not retrying a flake to green: the red stays recorded, the test stays
visible, and an issue owns its fix. Cost: no probe. Check: `cargo nextest run -p north-landing -E
'test(/a_quarantined_test_fails_after_landing_and_no_lane_is_blamed/)'`. Needs: none. Order: after n1039-flake.

### Step 3: several suspects, each probed alone (box n1039-suspects)

A prefix holds every lane before a suspect, so ranking cannot reorder a prefix bisection. Instead, each suspect is
probed alone: base plus that lane's own commits, the tree its alone pass checked. Probes run in rank order and in
parallel up to the free probe slots.

- **Rank:** first the share of the failed test's reach that the suspect touches (reached items it edits, over the test's
  reached items, with a measured edge before a fallback one), then the test's recorded flake rate from step 2. A test
  that flakes often needs a second failing run before a conviction, as flake-aware culprit finding weighs builds by
  flake probability ([FACF](../references.md#flake-aware-culprit-finding)).
- **Reuse:** a suspect whose alone pass ran the failed test on the batch base, or on a base the graph says cannot
  reach the test, is not probed again. Its recorded pass stands.
- **Fallback:** prefix bisection runs only over suspects, and only when the slots are fewer than the suspects.

On 2026-11-08 the graph's candidates averaged 2.3 lanes per test failure and 10.1 per compile error
(`docs/rationale/change-graph.md`), so test failures settle in one parallel round. Cost: up to k probes in parallel,
one build each. The wall time is one probe instead of about log2(n) sequential ones. Check: `cargo nextest run -p
north-landing -E 'test(/several_suspects_are_probed_alone_in_rank_order_and_the_failing_one_is_named/)'`. Needs:
none. Order: after n1039-blame-trust and n1039-flake (it reads both edges and flake rates).

### Step 4: lanes green alone and red together are an interaction (box n1039-interaction)

When every suspect probed alone passes the failed test, and the head's rerun still fails, the failure is an
interaction. The lander asks `north graph ask overlap --json` for the pairs of suspects whose cones share the failed
test, and confirms the top pair with one run of base plus both lanes. It then reverts the later-landed lane of the
pair, and tells both lanes' sessions the shared test and the paths each one touches. It covers the replay's four misses,
compile errors in a file one lane added that blame named on the other side of the interaction. With no sharing pair, the batch falls back
to prefix bisection over the suspects, and the red records the overlap the graph missed. Cost: one probe. Check:
`cargo nextest run -p north-landing -E
'test(/two_lanes_green_alone_and_red_together_are_named_as_an_interaction/)'`. Needs: none. Order: after
n1039-suspects.

### Step 5: the bars (boxes n1039-reds and n1039-bars-hold)

Each settled red writes one row beside its culprit-search span (`crates/north-lander/src/land/watch.rs`, `conclude`):

- the class: lane-scoped, flake, no suspect, one suspect, several suspects, interaction or bisected;
- the time of the red verdict and of the settle;
- the probes run and the bisection rounds;
- the lanes reverted.

`north land reds --since DATE` prints each red and the three bars, and `--check` exits 1 when one misses:

- **Time from red to a named culprit or a named flake:** p50 at most 60 s and p90 at most 6 min. The p50 is a warm
  rerun on the red tree. The p90 is bounded by one parent or alone probe, about one after-landing build. Baseline: 4
  min 23 s and 4 min 25 s in the fixtures, both to a wrong verdict.
- **Reverts of a lane later shown innocent:** 0 over 7 days. A revert is innocent when the reverted lane relands with
  the same patch id and the failed test then passes, or when the failed test gains a flake fact on a tree holding the
  lane. Baseline: 2 convictions of innocent lanes on 2026-11-11 alone, both kept from a revert only by a conflict.
- **Share of reds settled with no bisection round:** at least 80% over 7 days. Bisection stays for untraced batches,
  compile errors past the free slots, and interactions with no sharing pair.

Checks: `north land reds --since 2026-11-10` exits 0 and prints the three bars (n1039-reds; Needs: none; order: after
n1039-flake, since the classes need its outcomes). Then, not before 2026-11-20: `north land reds --since 2026-11-13
--check` exits 0 (n1039-bars-hold; Needs: none; order: after every other step).

## Rejected

- **Rerun the whole after-landing run before blaming.** That costs 315 to 418 s p90 per red. The failed tests alone
  answer the same question in seconds on a built tree.
- **Full Bayesian culprit finding over the batch** ([FACF](../references.md#flake-aware-culprit-finding)).
  Suspects average 2.3 lanes for test failures, so probing each alone in one parallel round settles the batch faster
  than a noisy binary search. The flake rate enters as rank and as the second-failure rule. Reopen if the bisected
  share stays above 20% after step 4.
- **Revert on a single differential run.** SafeRevert ([SafeRevert](../references.md#saferevert)) reverts only when
  the culprit signal is confident. Here a conviction always holds a fresh failing run of the suspect's tree and a
  passing run of its parent, and a lane is never convicted on the batch's own log.
