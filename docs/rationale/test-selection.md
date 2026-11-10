# Test selection: every test a change can reach runs before it lands

On 2026-11-10 lane ds-1033 (5f42fb5463, north#1033) moved the worker sandbox's path policy from north-runtime into
north-base across 16 files. Its pre-landing selection ran 4 tests, and its after-landing run ran the same 4 and read
green. GitHub Actions' Cargo workflow then failed
`worker_sandbox::a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state`
on that commit (run 42730398159). Main stayed red while more lanes landed, and the after-landing culprit search of the
next landing blamed the innocent dash lane 95afbfbca. The lane was reverted as 399762036. Tom asked (2026-11-10) for
the most proper fix to how changes to north's trust boundaries land. This note decides where selection reads
reachability from, what a miss does, what it costs and how it enters service (north#1039). It changes no code; the
lanes under Build make the changes.

## What the replay shows

Measured on gex45 at main 595d993e2. Sources: `north graph ask affected --json --repo . 5f42fb5463^ 5f42fb5463`, the
graph facts of each worker_sandbox test (`north graph show`), the land queue's `lander-post-land.tsv` and the GitHub
Actions logs.

- The failing test is unknown to the selection. It was added in 07d0491e4 on 2026-11-09 at 6:19 PM. The graph's tests
  are the 1151 recorded at bde3db9c6 on 2026-11-08 at 10:49 AM (`reads:north :reads-commit`). The test has no
  fact at all, so no edge, fallback or carried verdict can ever select it. 3 of the 5 tests in
  `tests/it/worker_sandbox.rs` are unknown in the same way.
- The tree holds about 1397 `#[test]` functions against the graph's 1151, and main has added 307 `#[test]` lines
  since the recording. A test added after the last recording runs before landing only in the lane that adds it.
- The selection never consults the crate-graph fallback that would have caught the test. `tests/it/crates.txt` maps the
  worker_sandbox module to north-runtime, and the module spawns the north binary, so `it_modules::select` picks it for
  any change to north-runtime. `it_modules::selection_asking` returns the graph's answer whenever there is one and reads
  that table only when the graph gives no answer. A single answer therefore covers the whole lane.
- The coverage map is absent where checks run. `~/.local/state/north/coverage` on gex45 is empty, and its graph holds
  no `coverage:north` commit, so every Rust change there takes the crate fallback over the known tests. For this lane
  that fallback picks 1104 of the 1151 known tests. They include 2 worker_sandbox tests, but not the one that failed.
- Nothing ran the whole suite on the land path. The after-landing run is also a selection: the 302 north rows of
  `lander-post-land.tsv` from 2026-11-08 to 2026-11-10 ran a median of 7 tests (p90 39), and 5f42fb5463's row reads
  green after 4. The only full run is GitHub Actions' Cargo workflow, about 6 minutes per push, and nothing in the
  lander reads its result.
- Misses are read but not learned from. `north land missed --since DATE` lists each failing Cargo run on main and
  the failed tests that the after-landing selection of its landing range left out, but nothing runs it on a schedule,
  records an incident or changes selection. `north test coverage miss` runs only when the prefix search names a
  culprit, and it reads a map file that gex45 does not have. The lane's pre-landing selection is not stored at all:
  `runner/selection.rs` writes the `selected` record only for after-landing runs, and only for a graph answer.
- Blame trusts a base whose own check never ran the failed test. Replayed today, `north graph ask blame` for the failed
  test against both lanes exits 3, because no lane's cone holds an unknown test, so the prefix search decides alone.
  It named 95afbfbca after the base, 5f42fb5463, had read green on 4 tests. Invariant 2 of
  [landing-by-construction.md](landing-by-construction.md) asks for a differential fail, meaning the same check
  passes on the parent, and the same check here means the same tests.

## Decision: run every test that cannot be proven unaffected

Today, selection includes only the tests that an edge names. It becomes the reverse: every test in a package the
change can compile into runs unless the map proves the change cannot reach it. An unknown test, a moved file, an
untraced input and a stale map then make selection run more tests, never fewer. Ekstazi
([references](../references.md#ekstazi)) uses the same rule: a test with no recorded dependencies always runs.

A test runs before landing when any of these holds:

1. **Guard floor.** The lane changes a path under a guard suite in `harness/guard-suites.toml`, and the test is in
   that suite. Three suites start the table:
   - Worker sandbox: `crates/north-runtime/src/worker_sandbox/`, `crates/north-land-sandbox/` and
     `tests/it/worker_sandbox.rs`. The suite is those modules' tests, the land-sandbox `enforce` target and the
     `worker_sandbox::` it module, about 40 tests.
   - Privacy gate: the outgoing denylist (`crates/north-land-scans/src/outgoing/`,
     `crates/north-landing-gates/src/landing/denylist*` and `crates/north-safe-push/src/publication/outgoing.rs`), the
     private kinds (`crates/north-base/src/platform/private_text.rs`) and the privacy judge
     (`crates/north-runtime/src/privacy/`). The suite is the tests of those modules.
   - Landing machinery: the whole packages north-lander, north-land-checks and north-safe-push, about 90 tests.
     Its paths include north-landing, where the post-land run and its argument order live, and its `cases` line names
     `north-lander::it land_chaos::chaos_lander_stopped_by_sigterm_mid_after_run_adopts_its_live_runner_and_reverts_the_red_lane`,
     so a lane changing north-landing builds north-lander's integration binary and runs that test; on 2026-11-09
     bb9237cbd0 changed north-landing's post-land runner and left the test out of every selection, and this edge makes
     the suite run it (north#1039). `north land missed` holds a failed test this floor selects for its landing range,
     so that historical miss reads as held by the guard suite rather than left out; the floor and the rejudging
     landed as 4ae28f7fb1 (north#1039).

   Two checks keep the table true. Every path in it must exist at the lane's head, the rule
   `harness/rust-reach-tests` already enforces. And a lane is refused when `git diff -M` shows a file under a guarded
   path renamed or deleted while the rename's target, or the code added in the same change, lies outside every path
   of that suite. 5f42fb5463 kept `mod.rs`, `broker.rs` and `tests.rs` under `crates/north-runtime/src/worker_sandbox/`
   but moved `lane_only.rs`, `readable.rs`, `removable.rs`, `except.rs` and `executable.rs` to north-base, so the
   existence check alone would have passed it. The rename check refuses it until the table lists
   `crates/north-base/src/platform/worker_sandbox/`. The floor reads no map, so it holds while the map is missing,
   stale or wrong.
2. **Learned edge.** The graph holds `(test :guards FILE)` for a file the lane changes. Only a selection miss writes
   that fact (below).
3. **Unprovable in a reached package.** The test's package depends, through Cargo's dependency graph at the lane's head,
   on a crate the lane changes (the crate closure), and the map cannot prove the test unaffected. The map proves a test
   unaffected only when all of these hold:
   - The map knows the test, recorded at a commit M at most 3 days old.
   - The test is fully profiled. Coverage reaches a spawned process only through an inherited `LLVM_PROFILE_FILE`
     (`crates/north-coverage/src/exec.rs`), and the worker_sandbox tests start north under `bwrap --clearenv` or
     `env_clear()`, so their map entry omits the code they guard. The record runs under the reads recorder, which sees
     every `execve`, and names profiles per process (`%p`) instead of pooling them (`%4m`). A test is fully profiled
     only when its exec record is complete and every run of a workspace-built instrumented binary, matched by build id
     rather than path (a hard-linked copy of north counts), left a profile written by that same process. A test the
     recorder cannot trace, such as the north-land-sandbox tests that ptrace cannot nest under, is unprofiled. An
     unprofiled test is cleared only through the crate closure, never through the map. 131 of the 146 `tests/it`
     tests spawn north, and 5 modules clear the environment.
   - Every file the test reached is unchanged between M and the lane's head, except changes inside regions it did not
     reach. Diffing only BASE..HEAD is not enough: after M, a landed lane can make a reached function call a new one,
     and a later lane that edits only the new function would then prove the test unaffected.
   - No changed Rust region between M and the lane's head, carried as `north_coverage::hunks::carry` does now, lies
     in a function the test reached.
   - No changed line between M and the lane's head falls outside every function region in any file of a crate the
     test's binary links. A constant, a static, a type or a macro cannot be traced to one function, and it can sit
     in a file the test never reached while a reached function uses it. On the 80-commit sample below, 56 of the 74
     lanes that changed non-test Rust changed at least one `use`, `struct`, `enum`, `const`, `static`, `type`, `mod`,
     `trait`, `impl` or macro line (field lines not counted), so this condition alone sends most lanes to the crate
     closure. An item-reference index narrows it: `rust-analyzer scip`, recorded with the map at M, names the
     functions whose bodies refer to each item, through other items transitively. A changed item then makes
     unprovable only the tests whose reached functions refer to it. A changed `use` line counts as a change to every
     function of its file, and an item the index does not know keeps the strict rule. The index records the symbol a
     name resolves to, so a call through a trait (`T::READ`, a `dyn` method) names the trait's item, not the impl
     that defines it, and derived impls carry no coverage. A changed line inside an `impl Trait for T` block
     therefore counts as referenced by every function that refers to that trait item or to `T`, and a changed derive
     or attribute on a type by every function that refers to the type.
   - The test's own source file is unchanged between M and the lane's head.
   - The lane changes no `Cargo.toml`, `build.rs` or lock entry of a crate in the closure of the test's package.

   A renamed or moved file counts as a deletion plus an addition. The deleted side selects every test that reached it.
   The added side has no regions, so it proves nothing, and every test in the closure of its crate runs. That is the
   5f42fb5463 case.
4. **Recorded read.** The test recorded reading, listing or expecting absent a path the lane changes, or its source
   embeds one, whatever package it is in. The change graph's cone already selects these through
   `readers` and `embedders`, and this is how `harness/defaults.toml` reaches the five north-fleet-supervisor tests.
5. **Untraced input.** The lane changes a non-Rust path, and the test has no recorded read set. This rule exists
   already as `always`, and unknown tests now join it. Unknown tests can be in any package, so when this rule fires
   the check builds and lists every workspace package that has a test target, not only the closure.
   Today the Rust test check line in `.safe-push` fires only on Rust inputs, so a lane changing only
   `harness/defaults.toml` runs only the seven named tree-wide tests, not the ones rules 4 and 5 pick.
   The line fires whenever the selection is non-empty, whatever the lane changed.
6. **Added.** The lane adds the test. This rule exists already.

A test in a package outside the crate closure runs only through rules 1, 2, 4, 5 or 6. A Rust change cannot compile
into such a test, and data reads reach it through rules 4 and 5.

The universe is every test target of the reached packages. Until n1039-select-rest the check line built only
`--lib --bins --test it` (`.safe-push`), and `graph_selection::built` dropped every other integration binary, so
`north-ops`'s `health_filing` and `north-land-sandbox`'s `record` could never be selected. Every selection now
passes `--tests` in `SAFE_PUSH_LAND_IT`, which builds every test target of the scoped packages, and the `built` filter
is gone.

The filter is built by inclusion, not by subtraction. A subtraction filter for a hub lane would name about 1100
tests at about 117 bytes each, past `graph_selection`'s 96 KiB limit. The included tests are the known tests no rule
proves unaffected, plus every test that `cargo nextest list` finds at the lane's head and the map does not know. The
check already builds those binaries, so listing adds only the list run. Whole packages or binaries that are wholly
included collapse to one `package` or `binary_id` term. Past the limit, the selection falls back to the whole
closure, never to fewer tests. The lane that builds this measures the list run and the filter length on the 9 hub
commits of the cost sample below.

## The map stays fresh where checks run

- **Record.** `north test coverage record` keeps its method: an LLVM-instrumented build, one profile per test, and
  function names and spans. It runs on the build hosts, gex45 and cpx62, which placement now offers fleet builds to
  first (north#1036). It runs off the land path in a background slot. The same pass also runs under the verdict-reuse
  recorder, so one run yields both reaches and reads. Recording reads roughly doubles a run
  ([verdict-reuse.md](verdict-reuse.md): 209.7 s to 415.6 s for 1115 tests), and instrumentation adds an LLVM build
  beside the Cranelift dev build. The lane that builds the record measures its cost before the cadence below is fixed.
- **Ingest.** Each record is told to every host's graph with its commit. The lander host and both build hosts must
  hold the same `coverage:north` commit. A host whose graph is older than the map reports so on the lander's round
  line, as the build lag does (invariant 4).
- **Cadence.** A refresh starts when the newest map is 24 hours old, when a selection miss is recorded, or when unknown
  tests pass 10% of the tests the lane's build lists. One refresh runs at a time per repository, as now.
- **Gaps.** Until the map is fresh, rule 3 covers the tests the map does not know, and the M..HEAD condition
  covers the tests whose reached files changed since M, so with phase 2's full proof (below) a stale map
  costs test time but not safety. On 5f42fb5463's day, every unknown test would have run as a crate-closure test.

## A selection miss is an incident that corrects the map

- **Ground truth.** GitHub Actions' Cargo workflow already runs every test binary and case once per push (its job
  checking that "the shards together run every cargo test binary and case once"). The instrumented record is a second
  full run, on the build hosts. Both are read, and neither is added to the landing path.
- **Stored selection.** Every alone check records the lane's pre-landing selection, of every kind (graph answer,
  module table, inclusion filter, guard floors), as a `selected` record beside the lane's outcome. Today only
  after-landing graph answers are recorded.
- **Judging.** `north land missed` already compares failing Cargo runs on main with the after-landing selection of
  their landing range. It is extended to read the pre-landing records and to run after every Cargo run on main. When
  a full run fails a test on main at commit C, it finds the landed lanes between the last green full run and C, and
  the lander bisects that range on the failed tests alone, by exact name. The responsible lane is the first whose tip
  turns red where its parent passes. If that lane's pre-landing selection did not hold the failed test, that is a
  miss.
- **Judging as built (n1039-miss).** Until the lander bisects, `north land missed` names the lane from Actions alone:
  among the first-parent commits after the last passing Cargo run, the suspects run from the last commit whose own
  run passed (or did not fail the test) to the first whose run failed it, and the lane is named only when those
  suspects carry exactly one lane in the lander's `post-land.tsv`; a batch waits for the bisection. The stored
  selection it reads is the `selected` record whose range is exactly that lane's landing (its parent to its commit),
  which the after-landing check computes from the lane's own diff, or reuses from its alone check. `--until` judges a
  settled window from a mirror stamped at least an hour after it, so the 2026-11-10 replay runs off the lander host.
  Failures of `north test coverage record` are not read yet: the record does not keep its failed tests (n1039-map).
  The `:guards` edge names the map's `test:BINARY::NAME` node when the graph holds exactly one, else `test:NAME`, since
  the Actions log names a test without its binary; the rule 2 reader matches both forms.
- **Record.** `north ledger incident record --key selection-miss:LANE:TEST --class selection-miss --noticed-by lander
  --repo north --subject LANE --detail "TEST; FILES"` records the lane, the test and the lane's changed files, and
  `north ledger incident report` on the ledger host lists the incident. When the guard lands,
  `north ledger incident guard --class selection-miss --guard SHA --landed-at EPOCH --issue north#1039` names it, so
  `recurred_after_guard` counts the misses after this design lands. Its weekly reading is the miss rate this design
  is judged by.
- **Correction.** The miss tells every host's graph `(test :guards FILE)` for each Rust or tree file the lane
  changed, so the next lane touching those files runs the test (rule 2), and it starts a refresh. An edge is dropped
  only when its file is deleted. The responsible lane is reverted like any lane that turned main red, because it is
  differentially red on the exact failed tests (invariant 2).
- **Blame.** Today `blame::culprit` returns a single graph-named suspect without any probe, `among` never probes the
  base, and graph blame's `Cone::holds` reads the old allow-list cones, so a test it does not know names no lane or
  the wrong one. Three changes fix this. First, every prefix probe, the base included, runs exactly the failed
  tests. Second, a single named suspect is confirmed by probing its parent before it is blamed. Third, a red base no
  longer ends the search with "main was already red": the search walks back over earlier landings, each probed on the
  same failed tests, and reverts the first lane that is red where its parent passes (in shadow first, below).
  A probe of a tree where a failed test does not exist yet counts as a pass for that test, never as red, so the lane
  that added a broken test is the one blamed. nextest reads an empty run as a failure by default, so the probe lists
  the failed tests at that tree with `cargo nextest list`, counts the absent ones as passes and runs the rest.
  `Cone::holds` moves to the rules above. This closes the 95afbfbca misattribution and reverts ds-1033, the lane
  actually at fault.

## Cost per batch

Measured on gex45 over the last 80 first-parent commits of main, 5f42fb5463 among them:

- Graph query: `north graph ask affected` takes 0.41 s, against an alone check's p50 of 156 s. Rules 1 to 3 are set
  arithmetic on the same facts.
- The graph as gex45 holds it today, recorded reads plus the crate fallback over known tests: a median of 337 tests,
  p75 697 and p90 1104 of the 1151 it knows. It misses every test added since 2026-11-08.
- Crate closure only, every `#[test]` in a package whose Cargo dependencies reach a changed crate: a median of 275
  tests, p75 610 and p90 1141 of about 1187. 9 of the 80 commits reached 40 or more of the 55 packages, which is the
  whole suite.
- Coverage reach, from the 2026-11-06 map of 773 tests: a change to one function selects a median of 0 tests, p90 4
  and p99 25. A lane takes the union over the functions it changes.
- The whole workspace suite: 209.7 s of nextest wall time on gex45, warm and uninstrumented
  ([verdict-reuse.md](verdict-reuse.md)).

With a fresh map, a batch pays for a coverage-sized selection of the mapped tests, a crate-closure-sized selection of
only the tests added since the last record and the unprofiled tests (rule 3), and the guard floors of the areas it
touches. The `north` package depends on nearly every crate, so the unprofiled `tests/it` tests run for almost every
Rust lane. GitHub Actions ran 49 tests of that binary in 18 s on one shard. The record lane counts the unprofiled
tests, and if they cost more than the landing p90 target allows, the fix is to make them leave profiles (an
`LLVM_PROFILE_FILE` the test passes through its cleared environment), not to drop them. The worker sandbox
floor is about 40 tests and the landing-machinery floor about 90, and each runs only when its paths change. With no
map at all, the cost is the crate-closure figures above, which is the safe ceiling.

The design therefore lands in two phases, each with its own shadow review:

- Phase 1 closes the 2026-11-10 class at a small cost: rules 1, 2, 4, 5 and 6, plus rule 3 for unknown and
  unprofiled tests, and the M..HEAD condition for known, profiled tests, whose proof is otherwise today's. Phase 1
  adds the guard floors, the tests added since the last record, the unprofiled spawning tests and the tests whose
  reached files changed since M to what runs now; the shadow measures that last share, which the 24-hour refresh
  bounds. Phase 1 keeps one known gap: a constant, static or type changed in a file the test never reached, while a
  reached function uses it. Today's selection has the same gap, and phase 2 closes it.
- Phase 2 replaces today's proof with rule 3's full conditions once the item-reference index exists. Before the
  index, the outside-function condition alone would put 56 of 74 Rust lanes at crate-closure cost (median 275
  tests, p90 1141). Phase 2 enforces only if its shadow shows an extra p90 within the landing p90 target.

## Alternatives

- **Crate closure only.** This is reverse-dependency selection over the build graph, as in Google's TAP
  ([references](../references.md#tap-scale)). It is sound for compiled code, needs no map and catches 5f42fb5463. But
  at p90 it costs the whole suite: 9 of the 80 recent lanes touched a hub crate, which puts about 210 s of tests on
  the critical path, against the landing p90 target of 60 s (2026-11-08). It also misses data reads, which the
  2026-11-08 replay found to be that day's most frequent breaker (n706-cost-words changed only
  `harness/defaults.toml`). It stays as the fallback (rule 3) and as the ceiling.
- **Coverage only, as built.** This is the approach of Microsoft's Test Impact Analysis and of Ekstazi
  ([references](../references.md#test-impact-analysis)). It is the cheapest when the map is fresh, but it is a
  whitelist: a test the map does not know never runs, which is the 2026-11-10 miss. It stays as the proof that a test
  is unaffected, not as the source of what runs.
- **Learned prediction.** Meta's predictive test selection ([references](../references.md#predictive-test-selection))
  halves test cost while reporting over 99.9% of faulty changes, by accepting misses and catching them later. North's
  landing is the last gate before main for agent lanes, and a miss costs a red main and a misattribution, so
  prediction may order tests within a selection but never drop one.
- **Static analysis of test bodies.** This is STARTS-style static dependency selection
  ([references](../references.md#starts)), which in Rust means the crate closure narrowed by symbol use. It cannot see
  into the north binary that 41 `tests/it` modules spawn, so it falls back to the closure for exactly the tests that
  cost most. Rejected.

## Shadow plan

Per `harness/gates.toml`, a gate that can refuse a landing starts in shadow. Selecting more tests can refuse a lane,
so the new selection is `[gate.test-selection]` with `mode = "shadow"`:

- Every alone check and after-landing run computes both selections, and the current one gates. The difference, meaning
  the tests only the new selection picks with the rule that picked each, runs off the critical path in a background
  slot on a build host. Each lane appends one line to `$XDG_STATE_HOME/north/gates/test-selection.jsonl` with the lane,
  base and head, both sizes, the extra tests by rule, the extra wall time, and any extra test that failed. A failed
  extra test is a would-be refusal.
- The review reads every host's log after 24 h and at least 20 lanes with a non-empty difference. It counts each
  would-be refusal true (the test fails on the lane and passes on its base) or false, and reads the extra wall time at
  p50 and p90. Enforcing needs a `[gate.test-selection.review]` table with at least 20 decisions, at least 18 of them
  correct, and an extra p90 that keeps the alone check within the landing p90 target.
- The guard floor and its rename check run in the same shadow as rule 3. Blame's exact-test probes enforce at once,
  since they only add checks before a revert that already happens. Its walk-back revert of an earlier lane is a new
  revert, so it starts as `[gate.blame-walk-back]` in shadow, logging the lane it would revert to
  `$XDG_STATE_HOME/north/gates/blame-walk-back.jsonl`, under the same review rule.

## Fit to the five invariants

- **Alone before together (1).** The alone check runs the new selection, so a lane that breaks a guarded or reachable
  test is refused on its own and never sinks a batch.
- **No verdict is not a fail (2).** A missing or stale map proves nothing, so the test runs instead of counting as
  unaffected, within phase 1's one named gap until phase 2. Blame and the miss judge revert
  only a lane that is red on the exact failed tests where its parent passes.
- **Measure the work, not the machine (3).** Selection reads facts: the diff, Cargo's graph and the recorded reaches.
  The shadow review reads extra wall time only as a budget, never as a verdict.
- **Read the source of truth by revision (4).** The map names its commit, its regions are carried to BASE, and each
  host reports the map commit it holds. A host whose map is older selects more tests, not fewer.
- **Every gate runs before it can bite (5).** The full suite that judges misses runs after landing by nature. Its job
  is to correct selection: once a miss has taught the edge, rules 1 to 6 run before landing every test it could fail
  on. The after-landing selection moves to the same rules, so `land.after` runs nothing the alone check would not.

## Replay of 5f42fb5463

Against its parent, under the rules above:

- Rule 3: the lane changes north-base, north-providers and north-runtime. Package `north` depends on all three, and
  the map does not know
  `worker_sandbox::a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state`,
  so nothing proves it unaffected. It runs.
- Rule 1: the lane changes files under `crates/north-runtime/src/worker_sandbox/`, so the whole worker sandbox floor
  runs, including the `worker_sandbox::` it module. The rename check refuses the lane first: five files leave that
  path for `crates/north-base/src/platform/worker_sandbox/`, which no suite lists. The lane's author adds the path
  to the worker sandbox suite, and the floor then also guards the new home.
- The map, even once it knows the failing test, cannot prove it unaffected: the test starts north under `env_clear()`
  and `bwrap --clearenv`, so it is one of the spawning tests that only the crate closure can clear.
- The test fails on the lane (GitHub Actions run 42730398159 failed it at this commit), so the alone check refuses
  ds-1033 and names the test before the lane rides a batch. Main does not go red, and 95afbfbca lands on a green
  main.
- Had a miss still happened, the Cargo run's failure at 5f42fb5463 would have recorded a `selection-miss` naming
  ds-1033, the test and the 16 files, and told `:guards` edges from the test to each file.

`cargo nextest run -p north-land-checks -E 'test(/a_moved_module_selects_the_tests_that_reach_it/)'`, written by the
first lane below, replays this lane's diff against fixture facts that hold no fact for the failing test.

## Build

Each lane has a box on north#1039 with its Done when command:

- n1039-select: phase 1. Rules 4 to 6 and rule 3 for unknown and unprofiled tests, and the M..HEAD condition
  for known, profiled tests, in `it_modules` and `north-graph-change`; every test target of the closure built and
  the `built` filter removed; the Rust test check line firing whenever the selection is non-empty; the inclusion
  filter with its `cargo nextest list` step; the pre-landing `selected` record for every kind of selection; and
  `[gate.test-selection]` in shadow with its log.
- n1039-guards: `harness/guard-suites.toml`, its existence and rename checks, and rules 1 and 2 (the `:guards`
  reader) in the same selection.
- Landed by the first select and guards lane: rule 1 and its existence and rename checks (the `guard suite` lane gate,
  under the shared `landing_refusals` switch); rule 3 for unknown tests, which `north graph ask affected` now returns as
  `added` entries for every `#[test]` at the lane's head in a package the changed Rust reaches and the graph does not
  know; and rule 3 for the unprofiled spawning `tests/it` modules, since a graph answer now joins the module table's
  crate selection over the lane's compiled files instead of replacing it. The guard floor and the joined selection
  run at once rather than in shadow: they only add tests to the check that already gates. The rest of both boxes is
  n1039-select-rest.
- Landed by n1039-select-rest: rule 2, the `(test :guards FILE)` reader in the change graph's cone, which resolves the
  bare `test:NAME` node `north land missed` tells when no map holds the binary to the known tests of that name, else to
  a name term in every binary; every test target
  of the scoped packages built (`--tests`) and the `built` filter removed; the `selected` record of every alone check
  and after-landing run, for every kind of selection (graph answer, module table, every module, guard floor); and
  `[gate.test-selection]` in shadow, whose rows in `gates/test-selection.jsonl` carry the selection that runs and the
  M..HEAD tests (`north graph ask affected`'s `since`: known tests whose reached functions changed between the map's
  commit and the lane's base) that the new selection adds. The Rust test check line firing whenever the selection is
  non-empty and the inclusion filter with its `cargo nextest list` step landed as n1039-select-tail (a0cd450b3).
- n1039-blame: every prefix probe runs exactly the failed tests, a single named suspect is confirmed against its
  parent, a red base walks back to the first differentially red lane and reverts it, and `Cone::holds` follows the
  new rules.
- n1039-miss: `north land missed` extended to the pre-landing records and run after each Cargo run on main, the
  `selection-miss` incident and the `:guards` writer.
- n1039-map: the record on the build hosts with reads and the unprofiled marking, the ingest into every host's graph
  with lag reporting, the cadence, and the record's measured cost added to this note.
- n1039-proof: phase 2. The item-reference index recorded with the map, and rule 3's outside-function condition
  narrowed by the index, in shadow as `[gate.test-selection-proof]`, after n1039-map.
- Landed by n1039-proof: `north test coverage record` runs `rust-analyzer scip` (from the dev shell) after the
  instrumented run and stores the index in the map: each definition's line span widened over its attributes and doc
  comments, each `use` statement as an item that counts for every function of its file, each `impl` header line as
  an item standing for the trait and type it names, and the symbols every function region and item refers to. The
  ingest tells them as `item-in`, `defines` and `refers-to` facts. `north graph ask affected` computes the phase 2
  selection beside the one that gates: a changed line outside every function region selects the tests reaching the
  functions that refer to its innermost item, through items that refer to it; a line in no item selects the crate
  closure. Each answer appends both sizes and the extra and dropped tests to
  `$XDG_STATE_HOME/north/gates/test-selection-proof.jsonl`. A record without the index leaves every outside line
  at the crate closure. The review needs n1039-map's record running on the build hosts.
- n1039-enforce: the shadow review and the switch to enforce of phase 1, once n1039-select and n1039-guards have run
  24 h. Phase 2's review belongs to n1039-proof.
