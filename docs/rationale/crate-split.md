# Crate split: why a leaf edit rebuilt 141k lines

Measured 2026-10-31 on the laptop, heavy class (6 CPUs, 8 GiB), warm cache: appending a blank line to
`src/provenance.rs` cost 10.5 s for `cargo build` and 15.6 s for `cargo test --no-run`. The whole loop was
one rustc run over the single `north` crate. No build setting got near 1.5x, so the crate had to split.

## What decides the gain

Cargo rebuilds every package downstream of a changed one, and a package's lib and bin share one
dependency list, so a package that lists a crate rebuilds when that crate changes even if only its bin
uses it. A split pays only when the edited crate has few, small dependents. The module graph in
`structure.toml` is a layered DAG, but `ops`, `threads`, `fleet`, `ledger`, `guard`, `providers`,
`capacity` and `usage` reference each other, so they stay together.

## Crates

- `north-base`: `error`, `foundation`, `platform`, `shell`, `autoland_status` and `test_inputs`. Depends
  on nothing in the repo. `autoland_status` lives here because `guard` reads it; putting it in
  `north-landing` would make `guard` rebuild on every landing-gate edit.
- `north-landing`: `landing`, `dep_gate`, `language_gate`, `provenance`, `issue_box_lint`,
  `follows_release`, `land_abandon`. Depends on `north-base` only.
- `north-core`: everything else (`fleet`, `ledger`, `threads`, `capacity`, `health`, `providers`, ...).
  Depends on `north-base`; nothing in it uses `north-landing`, `north-tui`, `north-guard` or `north-ops`.
- `north-tui`: `tui`, the fleet terminal screen. Depends on `north-core`; nothing in `north-core` uses it,
  so a screen edit rebuilds `north-tui` and the root package while `north-core` stays cached.
- `north-guard`: `guard` and `hook`, the tool-call policy behind `north hook`. Depends on `north-core` and
  `north-base`; nothing in `north-core` uses it, so a guard edit rebuilds `north-guard` and the root package.
- `north-ops`: `ops`, the operational commands. Depends on `north-base` and `north-core`; nothing in
  `north-core` uses it, and `north-guard` names it only for its tests.
- `north-capacity-host`: `capacity/host`, `capacity/gate`, `capacity/watchdog` and the `north capacity`
  subcommand dispatch (`capacity/command.rs`). Its `capacity` module re-exports `north_core::capacity` and adds
  those modules, so `crate::capacity::...` and `super::` paths inside them are unchanged. Depends on
  `north-base` and `north-core`; `north-ops` (for `land_offload`) and the root package import `capacity` from it.
- `north-runtime`: `runtime`, `land_offload`, `build_offload`, `laptop_runner`, `agent_contain`,
  `laptop_ntfy`, `timers`, `model_watch`, `job_watch`, `outside_watch`, `usage_web_read` and
  `runtime_access`, moved out of `ops` (box 4-9). Depends on `north-ops` for `ops::pacing`, `ops::dispatch`
  and `ops::laptop`, which the plan puts in `north-dispatch`; nothing in `north-ops` uses it. Its tests
  reach `ops::laptop::Ctx::fixture` through `north-ops`'s `test-support` feature, a dev-dependency only.
- `north` (the root package): `src/main.rs`, `src/cli/` and the `verify-north` and `fake_claude`
  binaries, plus the integration tests. Depends on `north-core`, `north-capacity-host`, `north-guard`, `north-landing`,
  `north-ops`, `north-runtime` and `north-tui`.

A leaf edit in `north-landing`, `north-tui`, `north-guard`, `north-ops` or `north-runtime` rebuilds that crate and what sits
above it; `north-core` stays cached. An ops leaf edit took 15.5 s wall and 16.2 CPU-seconds for `cargo build
--bin north` before `ops` moved and 6.3 s and 7.0 CPU-seconds after (median of 3, cloud worker, 4 CPUs). A
`capacity/host/run.rs` edit leaves `north-core` Fresh and rebuilds `north-capacity-host`, `north-ops` and the
root: `cargo test --no-run` took 8.3 s wall (median of 3, cloud worker, 4 CPUs), against the 5.8 s the plan
predicts for gex45 below. An edit in `north-core` still rebuilds every crate above it.

## Moving code

Modules keep their names. `north-core` and `north-landing` re-export the `north-base` modules, and the root
`main.rs` imports the modules `cli` uses, so every `crate::platform::...` path is unchanged. Items that
were `pub(crate)` became `pub`, which turns off dead-code warnings for them; `unreachable_pub` is not
enabled.

## Moving a module

`harness/split/move-module.sh MODULE CRATE` moves `crates/north-core/src/MODULE` into a new sibling crate
`crates/CRATE`. It `git mv`s the module to `crates/CRATE/src/MODULE`, so every relative path stays valid
and the files stay byte-identical; writes the crate's `Cargo.toml` with `north-core` and only the
workspace dependencies the module names (those named only in tests become dev-dependencies) and its
`lib.rs` with `pub mod MODULE;` and `pub use north_core::{...}` for each `crate::` root the module
reaches; adds the crate to the workspace and the root package; rewrites the root `main.rs` import and
the repo's path mentions, including those inside moved files other than fixtures; adds the crate's `lib.rs`
to the moved module's `paths` in `structure.toml`, never to `crate-roots`, whose `may_use` widening closed a
cycle; repoints a sibling crate that re-exported the module from `north-core`, as a dependency when its
library code uses it and a dev-dependency when only its tests do; and widens to `pub` only the items
`cargo check` reports as private. It also links `north-base` when the module names it, moves a fixture the
module embeds from elsewhere in `north-core/src` beside it, and turns a `#[cfg(test)]` helper the moved
tests reach into `#[doc(hidden)]`, always compiled: a `test-support` feature only tests turn on would give
`north-core` a second build per test run. It lists each `cfg!(test)` left in `north-core`, since that is
false under the new crate's tests; the caller in the new crate picks its own (`ops::defaults_measure`
passes `Sources::default()` to the `_with` calibration readers). To rebase a move, reset
to the new main and run it again rather than merging by hand.
`harness/split/move-module.sh MODULE CRATE` moves `crates/north-core/src/MODULE` into a new sibling crate
`crates/CRATE`. It `git mv`s the module to `crates/CRATE/src/MODULE`, so every relative path stays valid
and the files stay byte-identical; writes the crate's `Cargo.toml` with `north-core` and only the
workspace dependencies the module names (those named only in tests become dev-dependencies) and its
`lib.rs` with `pub mod MODULE;` and `pub use north_core::{...}` for each `crate::` root the module
reaches; adds the crate to the workspace and the root package; rewrites the root `main.rs` import and
the repo's path mentions; lets the `crate-roots` module of `structure.toml` use what the re-exports
point at; and widens to `pub` only the items `cargo check` reports as private. To rebase a move, reset
to the new main and run it again rather than merging by hand.
`north split move-module [--dry-run] [--from FROM] --crate CRATE [--allow-test-item NAME]... MODULE...`
moves each `MODULE` (a path such as `tui` or `ops/tempo`) out of `crates/FROM/src` (default
`north-core`) into the sibling crate `crates/CRATE/src/NAME`, all in one run. It refuses when any file
outside the root package still reaches a module being moved, since that edge must be cut first. It
`git mv`s each module, so the files stay byte-identical renames; reads `cargo metadata` before touching
anything, so registry crates are fetched and each crate's library name is known; writes the crate's
`Cargo.toml` with exactly the in-repo crates (`north-base` included) and workspace dependencies the moved
files name, test-only ones as dev-dependencies and `doctest = false` unless a moved doc comment holds a
code block; and writes `lib.rs` as `pub use FROM::{...}` for the source crate's modules the moved files
reach plus a `pub mod` list. It adds the crate to the workspace and the root package, rewrites the root
callers and the repo's path mentions except in `*_tests.rs` and `fixtures/`, which replay history, and
adds the new paths and the crate's `lib.rs` to the moved module's `structure.toml` entry; no `may_use`
list changes, and it refuses a result with a `may_use` cycle. It then widens to `pub` only what `cargo
check` names as private. A `#[cfg(test)]` item the compiler names stops the run, naming it, unless
`--allow-test-item NAME` lets that `#[cfg(test)] mod` become a `#[doc(hidden)] pub mod`, which the report
lists for review. It ends with `cargo test --workspace --no-run` and the new crate's tests and prints a
`Dependency: NAME — REASON` trailer for each dependency it added, which the lander's dependency gate
requires. A second run on a moved tree changes nothing. To rebase a move, reset to the new main and run
it again rather than merging by hand.

## north-guard

`guard` and `hook` reference each other, so they left together, and `north-guard` sits above `north-core`
because `guard` uses `threads`, `fleet`, `ledger`, `usage`, `capacity`, `lanes` and `router` while only a
few edges ran the other way. `guard::host::Host` already carried `guard`'s calls into `ops`; the move cut
the edges from `north-core` into `guard`:

- `providers/claude/runs` read transcripts through `guard::hook_events`. `hook_events` and `hook_rules`
  classify hook output and touch nothing else in `guard`, so they moved to `threads::runs`, the owner of the
  `hook_events` table; `guard` re-exports them.
- `ops::agents::goal::plan::gate_width` and `ops::pacing::placement::here_cpu_refusal` asked
  `guard::spawn_capacity` for the spawn gate's verdict. `ops::pacing::spawn_gate` holds the two functions,
  and `main` installs them from `guard::spawn_capacity` before any command runs.
- The `spawn_capacity` tests moved from `ops::pacing` back beside `spawn_capacity`, calling
  `ops::pacing::answer` through the re-export.

`move-module.sh` moves one module per run and writes the crate's `Cargo.toml` and `lib.rs`, so `hook` was
moved by hand first and added to `lib.rs` after the run, and `north-base` was added because `guard` names
`north_base::` directly. Since `ops` left `north-core`, `north-guard` lists `north-ops` as a dev-dependency
for those tests; nothing in `ops` points into `guard`.
`move-module.sh` moves one module per run and writes the crate's `Cargo.toml` and `lib.rs`, so `hook` was
moved by hand first and added to `lib.rs` after the run, and `north-base` was added because `guard` names
`north_base::` directly. When `ops` leaves `north-core`, `north-guard` depends on that crate; nothing in
`ops` points into `guard`.
The shell recipe this move used moved one module per run and rewrote the crate's `Cargo.toml` and `lib.rs`,
so `hook` was moved by hand first and added to `lib.rs` after the run, and `north-base` was added because
`guard` names `north_base::` directly; `north split move-module` moves both in one run and names
`north-base` itself. When `ops` leaves `north-core`, `north-guard` depends on that crate; nothing in
`ops` points into `guard`.

## Measurement

See north#402 for the numbers; the method is a blank line appended to `provenance.rs`, `cargo build --bin
north` inside one `nix develop` shell, median of 3, taken against a worktree at the pre-split commit under
the same machine load.

## Crate-size gate

So no crate grows back into one giant rustc run, `structure.toml` `[crate_lines]` caps each workspace
crate's non-test Rust lines: `default` (25000) for most crates, `hub` (10000) for a crate that `hub_dependents`
(3) or more workspace crates list under `[dependencies]`, and `[crate_lines.overrides]` for a named crate.
Non-test lines are the lines of `src/**` and `build.rs` that are neither blank, comment-only nor inside a
`#[cfg(test)]` item, in files not named `tests.rs` or `*_tests.rs` and not under a `tests/` directory.
`north code-health` prints each crate's count against its cap. `north code-health crate-lines BASE HEAD`
counts both revisions from git and refuses a crate over its cap whose count grew, and a new crate over its
cap; crates already over (north-core) may shrink or stay but not grow. `north-ops` left `north-core` already over
the default cap, so its override is the count it moved with (37362) and it ratchets the same way. The refusal names where the new
code goes: the smallest crate under its cap that the grown crate already depends on, or a new
`crates/<name>/` named after the module that grew most. It runs as the `crate-lines` after-landing gate
beside `code-health`, through the `north` beside `safe-push`, wherever HEAD's `structure.toml` has
`[crate_lines]`.

## CPU profile of the land check

Measured 2026-11-05 on gex45 (20 cores, shared with other leases) at fad7445bd, inside `./bin/land-shell --dev`
under one heavy lease (6 CPUs, `RUST_TEST_THREADS=6`, `CARGO_BUILD_JOBS=6`), with a fresh target directory and
the dev profile's incremental compilation. CPU-seconds are user plus sys from GNU time 1.10, children included.
Wall time on a loaded host inflates; CPU-seconds move far less, so they are the figure to compare.

Until 2b5675065 (2026-11-05) the land check ran `harness/scripts/cargo-tests.sh`: `cargo test --no-run`, then
every test binary side by side through `north land-tests`. Since then the land check runs clippy and the docs
checks, and the full suite runs once per push to main in `cargo.yml`. The suite's cost below is what a landing
spends either way: on the lander before land-first, on the CI runner after it, and on every agent that runs
`cargo test` locally.

### One landing, in CPU-seconds

- Suite after a one-line edit in `north-core`: 31.3 to compile (rustc and link of the workspace's targets) plus
  409.2 to run the eight test binaries, 440.5 in all. Doc tests add 3.4 (CI shard 1 only).
- Suite after a one-line edit in `north-landing`: 13.0 to compile plus the same 409.2 to run, since every binary
  still runs.
- Clippy (`--all-targets -D warnings`) after a one-line edit in `north-core`, today's land check: 49.6 (46.3 + 3.4), 28.7 s wall; the first clippy over a target that held only the test build cost 155.2. The docs checks beside it were not measured.
- Cold build of everything `cargo test --no-run` needs: 481.8 (346.4 the workspace's own targets, 118.2 the
  231 dependency units, the rest cargo and build scripts), 146 s wall.

### Build cost per target

CPU-seconds of each workspace rustc invocation (compile and link), from a `RUSTC_WRAPPER` that ran each rustc under
GNU time.

| Target | Cold | After a `north-core` edit | After a `north-landing` edit |
| --- | ---: | ---: | ---: |
| `north-core` unit tests (`--test` of the lib) | 140.6 | 13.8 | - |
| `north-core` lib | 107.0 | 8.6 | - |
| `north-landing` lib | 24.2 | - | 1.4 |
| `north-landing` unit tests | 23.7 | - | 2.1 |
| `it` integration tests (root and `north-landing`, 2 binaries) | 18.2 | 2.6 | 3.3 |
| root package unit tests | 8.4 | 2.3 | 2.1 |
| `north` bin | 7.2 | 2.3 | 2.1 |
| `north-base` unit tests | 7.0 | - | - |
| `north-base` lib | 5.3 | - | - |
| `verify-north` bin and its tests | 2.4 | 0.5 | 0.6 |
| `lander_model` integration test | 1.6 | - | 0.3 |
| `fake_claude` example | 0.8 | 0.2 | 0.2 |

There are three integration test binaries: the root `tests/it` (110 modules; it links `north-core`,
`north-landing` and `north-base` and drives the built `north` through `CARGO_BIN_EXE_north`) and
`crates/north-landing/tests/{it,lander_model}` (they link `north-landing` and `north-base` only). Only the
root `it` links the whole of `north-core`, so it rebuilds on any `north-core` edit. The `north-core` unit-test
target is the costliest compile in both the cold and the incremental case: it compiles the 141k-line crate a
second time with its tests.

### Run cost per test binary

| Binary | Tests | CPU-s (user + sys) | Wall s | Sum of per-test wall s |
| --- | ---: | ---: | ---: | ---: |
| root `it` | 512 | 287.4 (157.0 + 130.4) | 119.1 | 644.4 |
| `north-core` unit tests | 1922 | 75.5 (65.0 + 10.6) | 33.5 | 151.1 |
| `north-landing` unit tests | 362 | 27.3 (17.1 + 10.1) | 5.1 | 23.8 |
| `north-base` unit tests | 57 | 6.0 | 4.5 | 8.0 |
| `lander_model` | 4 | 6.0 | 2.7 | 3.9 |
| `north-landing` `it` | 17 | 4.4 | 3.3 | 9.3 |
| root package unit tests | 123 | 2.6 | 9.4 | 53.1 |
| `verify-north` | 0 | 0.0 | 0.0 | 0 |
| Total | 2997 | 409.2 | | |

The root `it` binary is 70% of the suite's CPU, and 45% of its CPU is sys time: its tests fork and exec
`north`, `git` and bash stubs. Twelve tests failed in this sandbox (8 in root `it`, 4 in `north-core`: they
read the installed skills, live defaults, the gaming hold or the checkout's untracked files); each failed
in under 5 s, so the totals hold to within a few CPU-seconds.

### The 25 costliest tests

Each test run alone (`--exact NAME --test-threads=1`) under GNU time, for the 80 tests with the longest
per-test wall in the binary runs (libtest `-Z unstable-options --report-time` under `RUSTC_BOOTSTRAP=1`).
The 25 below sum to 128.3 CPU-s, the top 20 to 115.9, the 80 to 166.8, against 409.2 for the suite: the rest
is spread thin over 2900 tests, so per-crate selection of the root `it` binary is the larger lever, and these
levers buy at most a quarter of the suite.

| # | Test (binary) | CPU-s (user + sys) | Lever |
| ---: | --- | ---: | --- |
| 1 | `safe_push_tag::safe_push_tag_scans_an_outgoing_tag_against_an_origin_with_thousands_of_refs` (root `it`) | 20.4 (7.3 + 13.2) | Smaller setup: write the 6000 refs as one `packed-refs` file and the 2000 blobs through one `git fast-import`, instead of 2000 file writes and `git add`; measure the setup against the scan first |
| 2 | `land_serve::serve_persistent_transient_isolates` (root `it`) | 9.8 | Shared fixture: build the origin, base commit and stub bin once per binary and copy it, instead of `git init`, commit and push per test |
| 3 | `land_serve::serve_persistent_deferred_isolates` (root `it`) | 9.3 | Same fixture as 2; both run `persistent_isolates`, so one setup serves both |
| 4 | `threads::calibration::tests::the_eta_prediction_ratio_row_gives_the_scale_covering_the_target_share_of_runs` (`north-core`) | 9.1 (all user) | The drifting-ledger walk-forward property it calls first is pure CPU; recalibrate incrementally per step instead of over the whole ledger at each step |
| 5 | `ops::dispatch::order_tests::holds_and_claims_obey_the_rules_over_every_generated_order_of_events` (`north-core`) | 8.0 (all user) | Same 300 seeds of 40 events on a cheaper step: format the trail only when a seed fails, not with `format!` on every event |
| 6 | `capacity::host::estimate_replay_tests::the_recorded_week_of_runs_is_covered_by_its_estimates_without_over_reserving` (`north-core`) | 6.6 (all user) | Same recorded week on a cheaper replay: keep the earlier runs' estimate state incrementally instead of re-estimating from every earlier run |
| 7 | `lane_gc_journeys::an_orphan_landing_ref_is_pinged_held_through_grace_and_archived_to_the_mirror_before_deletion` (root `it`) | 6.2 | Shared rig template (origin, mirror, checkouts) built once per binary and copied |
| 8 | `lane_gc_journeys::each_lane_class_retires_behind_a_verified_archive_tag_and_owned_work_stays_reported` (root `it`) | 6.0 | Same rig template as 7 |
| 9 | `desktop_action_guard::guard::desktop_action_guard` (root `it`) | 5.6 | One fixture and fake niri for all cases instead of one per case; wall 10.2 s against 5.6 CPU-s also points to fixed sleeps that a readiness wait would replace |
| 10 | `capacity_leases::lease_past_its_timeout_has_its_scope_killed_and_is_released` (root `it`) | 5.6 | CPU near wall (6.2 s) means a busy poll; wait on the lease state change instead of spinning |
| 11 | `safe_push_tag::safe_push_tag_publishes_clean_fixtures_through_either_entry` (root `it`) | 3.7 | Shares the safe-push tag template with 1; build it once per binary |
| 12 | `land_serve::precheck::precheck_checks_each_lane_alone_as_it_arrives` (root `it`) | 3.3 | Same fixture as 2 |
| 13 | `worker_ledger_record::recorded_context_telemetry_counts_compactions_hook_fires_and_ignored_reminders_once_per_request` (root `it`) | 3.0 | Build `context_rig` and its backfill once per binary (the asserts only read it) |
| 14 | `dev_shell_target::the_dev_shell_keys_a_shared_target_to_its_checkout_and_leaves_the_checkouts_own_target_unkeyed` (root `it`) | 2.9 | End-to-end over `nix develop`: run it only when `flake.nix`, `bin/land-shell` or the dev-shell code changes |
| 15 | `laptop_holds::a_seat_idle_on_power_reopens_the_laptop_after_the_window_closes_it_and_tom_active_or_on_battery_keeps_it_shut` (root `it`) | 2.8 | Drive the hold decision as a `north-core` unit test instead of one `north` process per tick; keep one end-to-end tick |
| 16 | `land_serve::promote::promote_replays_each_recorded_round_then_shadows_the_queue_before_it_swaps_the_lander_link` (root `it`) | 2.8 | Same fixture as 2 |
| 17 | `land_serve::culprit_search_names_the_culprit_in_one_round` (root `it`) | 2.8 | Same fixture as 2 |
| 18 | `fleet_launch::a_launch_naming_files_two_queued_lanes_changed_is_queued_behind_the_earlier_lane_unless_overlap_ok_says_why` (root `it`) | 2.7 | Shared fleet-launch fixture built once per binary |
| 19 | `land_serve::prechecks_spread_over_hosts` (root `it`) | 2.7 | Same fixture as 2 |
| 20 | `land_serve::serve_mixed` (root `it`) | 2.6 | Same fixture as 2 |
| 21 | `land_serve::serve_defer` (root `it`) | 2.6 | Same fixture as 2 |
| 22 | `land_serve::speculate_depth_three_red_middle` (root `it`) | 2.5 | Same fixture as 2 |
| 23 | `worker_ledger_record::every_provider_is_recorded_with_its_brief_stages_landing_and_roles_and_the_summary_groups_them` (root `it`) | 2.4 | Same as 13: one recorded rig per binary |
| 24 | `worker_ledger_record::a_recording_run_recalibrates_a_stale_ledger_once_and_records_priors_once_a_week_where_codex_rollouts_exist` (root `it`) | 2.4 | Same as 13 |
| 25 | `provenance::test_steering_records_refuse_missing_grounding_and_rank_revisit_evidence` (root `it`) | 2.4 | Its cases are argument checks; cover them as `north-landing` unit tests on the parser and keep one end-to-end case, so per-crate selection skips the rest |

All 76 `land_serve` tests in the root `it` binary spend 199 s of per-test wall together, the largest family.
The 9 land checks recorded before land-first (2026-11-04 11:35 PM to 2026-11-05 6:09 AM) gave the per-binary walls
on the lander: root `it` 38.6 s, `north-core` 21.8 s, `north-landing` 9.7 s, `north-base` 4.2 s, `lander_model`
4.0 s (means); those logs carry no per-test or CPU times, which is why this run exists.

### Sources

- Build figures: `RUSTC_WRAPPER` logs (GNU time per rustc) and GNU time over `cargo test --locked --offline
  --no-run` for the cold build, a no-op rebuild (0.2 CPU-s) and a blank line appended to
  `crates/north-core/src/lib.rs` and to `crates/north-landing/src/lib.rs`.
- Binary figures: GNU time over each test binary run from its package directory, as `cargo.yml` runs them.
- Per-test figures: GNU time over each of the top 80 tests run alone, single-threaded.
- Clippy: GNU time over `cargo clippy --all-targets --locked --offline -- -D warnings`, warm, then after the same
  `north-core` edit.
- Lander walls: `land check: BINARY passed after N s` lines in `~/.local/state/safe-push/land-check-*.log`.

## Next cuts

Planned 2026-11-05 at ec20479a5 (north#259 box 4). That base already has box 1 (no module outside `ops` names
`crate::ops`, 47210d2f0), box 3a (`north-tui`, 895bb0e6f), the crate-size gate (d7535393f) and the 10 s
leaf-edit gate (5faa493cf). This version supersedes the first draft (8c7aadcec, withdrawn), which a review
broke in 12 places. The goal is CPU-seconds per landed lane.

### Method, rerunnable

`harness/split/cut-plan.sh PLAN [DAYS]` computes every number below from the checkout and `git log`. PLAN is
one of `harness/split/plans/`:

- `now.txt`: today's crates.
- `c12.txt`: this plan.
- `c13.txt`, `c15.txt` and `c19.txt`: the alternatives priced below.

Each plan line is a crate, its declared dependencies and its module prefixes. The script prints:

- each crate's lines, its non-test code lines (as the crate-size gate counts them: no blank, comment or
  post-`#[cfg(test)]` lines), its cap, 7-day commits, transitive dependents, and the CPU cost of
  one edit;
- every `crate::` reference that breaks the plan's DAG, with file:line, marked test or prod by file name or
  an enclosing inline `#[cfg(test)] mod`;
- the per-commit rebuilt set over the window.

It does not resolve `super::` chains, `pub use` re-exports or `mod.rs` dispatch, and it counts string
literals. C5 below was found by hand for that reason, and each crate move's `cargo check` is the final word.

The cost model is the incremental figures in "CPU profile of the land check" above (gex45, dev profile,
incremental on):

- After a one-line north-core edit: lib 8.6 CPU-s and unit tests 13.8 CPU-s, of which about 2.3 is the link,
  over 161k lines. So a rebuilt crate costs 0.053 (lib) plus 0.071 (unit-test compile) CPU-s per 1k lines.
- Each rebuilt test binary also links 2.3 CPU-s times the share of the workspace it links.
- The root targets add 7.4 CPU-s per edit.

The model gives 31.7 CPU-s for today's north-core edit against 31.3 measured. Wall time, worked by hand from the same line counts, is the critical path of libs to the root
plus the slowest dependent's test harness, times 1.57. That factor is box 8c's
loaded gex45 leaf-edit wall (21.7 s, incremental on) over the measured 13.8 s critical path. It is a
one-point calibration, so the first crate move re-measures it.

Box 4-9 measured the north-runtime leaf edit (a blank line appended to `runtime/probe.rs`, `cargo test
--no-run`, warm, median of 3) on a cloud worker, 4 CPUs at load 3, with the Cranelift dev profile of
cd8f3c1: 3.6 s wall and 9.7 CPU-s, from 7.4 s and 18.4 CPU-s for the same edit in `north-ops` at 8bef37c. The
model's critical path for that edit is 5.2 / 1.57 = 3.3 s, so the cloud factor is 1.08. The 1.57 factor is
gex45's under load and LLVM codegen; it stands until a gex45 run of the same edit replaces it.

Box 4-12 measured the north-dispatch leaf edit the same way (a blank line appended to `dispatch/brief.rs`,
4 CPUs at load 3): 8.3 s wall and 25.9 CPU-s, from 8.8 s and 24.4 CPU-s for the same edit in `north-ops` at
515094e. North-core, north-capacity-host, north-guard and north-tui stay Fresh; north-dispatch, north-ops,
north-runtime and the root rebuild. The cloud-factor prediction is 7.9 / 1.57 × 1.08 = 5.4 s, so this hub edit
misses the model by 2.9 s: the edit still rebuilds north-ops (20.1k non-test lines) and its test harness above it.

### Candidate 8 answered

`build-cost.md` rejected a further split because only 4 of 85 north-core commits touched no module of the
fleet/ops/capacity/threads/guard/providers/ledger/usage/health cluster. That count assumes the cluster stays
one crate. `cut-plan.sh plans/now.txt` agrees on today's layout: only 85 of the window's 578 Rust commits
rebuild less than north-core. The cluster is not a knot, though. The six cuts below are small: test fixtures,
one impl, one adapter's policy, and host primitives. Over the same 578 commits, `plans/c12.txt` gives:

- 234 commits rebuild exactly one crate besides the root; today, 85 do.
- Rebuilt-set sizes: 270 commits rebuild at most 40k lines, 17 rebuild 40k to 65k, 89 rebuild 65k to 110k,
  131 rebuild 110k to 160k (`fleet`, `threads`, `capacity`, `providers` and `dispatch` edits), and 71 rebuild
  more than 160k (`north-base`, `pace`, `Cargo.lock`).
- Rebuilt lines fall from 86.6M to 48.5M (44%), and modelled CPU from 17,002 to 12,592 CPU-s (26%).

The CPU saving is smaller than the line saving, for two reasons. Every edit still pays the root's 7.4 CPU-s.
And a hub edit (`fleet` 31.6, `threads` 30.9, `pace` 34.9 CPU-s against 31.7 today) saves nothing, because it
rebuilds nearly everything and links more test binaries. The saving comes from leaf edits. Every move takes
its `*_tests.rs`, `tests.rs` and inline `#[cfg(test)]` modules with it, so the residual north-core unit-test
harness covers 14.2k lines, from 161k.

### How many crates

Per-crate costs, priced:

- Each extra crate whose edit is rebuilt adds one test-binary link to every edit beneath it. The model counts
  this as 2.3 CPU-s times the linked share.
- Each new lib adds one rustdoc pass to `cargo test`. Only `ops/land_offload/ship.rs:122-128` has doctests,
  so every new crate except `north-runtime` sets `doctest = false`.
- Each move costs one lane, about 9 `Dependency:` trailers (895bb0e6f has 9) and a `lib.rs` re-export layer.

The same 578 commits under each grouping (`plans/*.txt`, or a scratch copy for the last two):

- 19 crates (`c19.txt`): 44.1M lines rebuilt, 12,704 CPU-s, 214 single-crate commits.
- 15 crates (`c15.txt`, with lanes and code_health in guard, health in flow, timers in runtime): 47.0M,
  12,772 CPU-s, 232.
- 13 crates (`c13.txt`, also flow and health in ops, ledger in providers): 48.0M, 12,665 CPU-s, 234.
- 12 crates (`c12.txt`, also `ops/pacing` in north-dispatch): 48.5M, 12,592 CPU-s, 234. This is the plan.
- 11 crates (also runtime in ops): 51.6M, 12,764 CPU-s. It leaves ops at 24.1k of its 25k cap.
- 12 crates with capacity-host merged back into capacity: 51.8M, 13,367 CPU-s. The host split earns its keep.

Twelve crates use the least CPU because they link the fewest test binaries without merging two hot leaves,
and they need 7 fewer move lanes than 19. The review's 11-crate grouping (pace, threads, fleet, ledger and
providers as one shared crate) is ruled out by the crate-size gate. That crate would hold about 24k non-test
lines with 3 or more dependents, against the 10k hub cap.

### Crate DAG (12 crates from north-core)

```
north (root: src/cli, bins, tests/it) ── every crate
  leaves: north-ops, north-runtime, north-guard, north-core (residual), north-tui, north-landing
  north-runtime ── north-dispatch, north-capacity-host, north-providers
  north-ops ── north-dispatch, north-threads
  north-guard ── north-dispatch, north-providers
  north-tui ── north-providers
  north-dispatch ── north-capacity, north-fleet, north-threads, north-providers
  north-capacity-host ── north-capacity, north-threads, north-fleet
  north-providers ── north-fleet, north-threads
  north-core ── north-fleet, north-threads
  north-fleet ── north-pace;  north-threads ── north-pace
  north-pace, north-capacity, north-landing ── north-base
```

### Crates

Lines are total and non-test code at ec20479a5. Commits are 7-day commits touching the crate, out of 578.
CPU is the modelled CPU-s of one edit, against 31.7 today. Saved is commits × (31.7 − CPU). It ranks the
moves but is not additive, because a commit touching two crates counts in both; the whole plan saves 4,410
of 17,002 CPU-s over the window (26%). Wall is the predicted leaf-edit `cargo test --no-run`, against the
10 s gate.

| Rank | Crate | Modules | Lines (non-test) | Cap | Commits | Dependents | CPU | Saved | Wall |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | north-ops | ops minus the groups below, ops/flow, ops/agents, ops/detectors, ops/tool_mine, ops/tempo, health | 30.2k (17.8k) | 25k | 117 | none | 12.3 | 2,270 | 6.6 s |
| 2 | north-runtime | ops/runtime, land_offload, build_offload, laptop_runner, agent_contain, laptop_ntfy, timers, model_watch, job_watch, outside_watch, usage_web_read, runtime_access | 12.8k (6.3k) | 25k | 102 | none | 10.1 | 2,203 | 5.2 s (cloud 3.6 s, 9.7 CPU-s) |
| 3 | north-guard | guard, hook, hook_tests, hook_rewrite_tests, lanes, code_health | 29.6k (20.1k) | 25k | 82 | none | 12.3 | 1,591 | 6.6 s |
| 4 | north-capacity-host | capacity/host, capacity/gate, capacity/watchdog | 7.4k (4.5k) | 25k | 74 | runtime | 11.7 | 1,480 | 5.8 s |
| 5 | north-core (residual) | grounding, catalog, agent_config_check, agent_policy_contract, agent_catalog, build_time, proxy, contract_probe | 14.2k (9.1k) | 25k | 43 | none | 9.8 | 942 | 5.3 s |
| 6 | north-dispatch | ops/dispatch, tune, check, dividend, pacing, laptop, resident_cap | 15.8k (7.4k) | 10k | 85 | runtime, ops, guard | 22.7 | 765 | 7.9 s (cloud 8.3 s, 25.9 CPU-s) |
| 7 | north-capacity | capacity minus host, gate, watchdog | 6.2k (3.4k) | 10k | 68 | dispatch, capacity-host, runtime, guard, ops | 25.2 | 442 | 8.4 s |
| 8 | north-providers | providers, codex, ledger | 7.1k (3.9k) | 10k | 50 | dispatch, runtime, guard, ops, tui | 25.2 | 325 | 8.5 s |
| 9 | north-threads | threads, work_items | 10.8k (5.9k) | 10k | 62 | everything above it except capacity | 30.9 | 50 | 9.4 s (cloud 12.3 s; `-p` 1.5 s) |
| 10 | north-fleet | fleet, claude_stream | 15.9k (7.8k) | 10k | 91 | everything above it except capacity | 31.6 | 9 | 9.8 s |
| 11 | north-pace | pace, router, usage | 11.1k (6.0k) | 10k | 30 | everything but capacity and landing | 34.9 | −96 | 10.7 s |
| - | north-tui (landed) | tui | 3.4k (1.6k) | 25k | 10 | none | 8.5 | 232 | 5 s |

Box 4-14 measured `north-threads` after its move, on a 4-CPU cloud worker at load 3.6 (Cranelift dev profile,
median of 3, a blank line appended to `threads/withheld.rs`): `cargo test --workspace --no-run` 12.3 s, against
12.7 s for the same edit on the parent main; `cargo test -p north-threads --no-run` 1.5 s, against 5.8 s for
`-p north-core` on the parent. The workspace edit still rebuilds every crate above threads, as the model's 30.9 CPU-s
per edit against 31.7 says, and the crate's own test loop is 3.9 times faster.

The leaf moves (ranks 1 to 5) can land first, each depending on north-core the way `north-tui` does. The hubs
(ranks 6 to 11) save little themselves. They leave north-core so the residual becomes a leaf, so north-core's
ratcheted size shrinks, and so the hubs sit low in the DAG.

`pace` (10.7 s) is the one predicted miss of the 10 s gate, with `fleet` (9.8 s) close behind. Box 4-13
measured it after the move, on a 4-CPU cloud worker (Cranelift dev profile, median of 3, a blank line appended
to `pace/limits.rs`): `cargo test --workspace --no-run` 14.6 s, against 14.5 s for the same edit on the parent
main; `cargo test -p north-pace --no-run` 2.7 s, against 9.9 s for `-p north-core` on the parent. So the
hub edit costs the workspace what it did, as the model's −96 says, and the crate's own test loop is 3.7 times
faster. Box 4-15 measured `fleet` after its move the same way, a blank line appended to `fleet/lane_probe.rs`:
`cargo test --workspace --no-run` 11.8 s, against 12.3 s on the parent main; `cargo test -p north-fleet
--no-run` 1.7 s, against 5.8 s for `-p north-core` on the parent. `north code-health` counts north-fleet at
8,942 non-test lines at the move. Their edits
rebuild every lib chain up to `ops`. The gate times a single leaf file (`structure.toml` names
`code_health/source.rs`, which lands in north-guard at 6.6 s), so they stay slow without failing the gate.
`fleet` sits at 7.8k of its 10k hub cap while taking 91 commits a week. If it nears the cap, the relief is
`fleet/registry`: 1.8k non-test lines and 52 of fleet's commits, reached from outside fleet by 2 `guard`,
4 `ops`, 1 `usage` and 8 `north-tui` references.

### Edges to cut

`cut-plan.sh plans/c12.txt` prints every break below. Paths are under `crates/north-core/src/`.

- C1, `pace` → `fleet`: `router/pricing.rs:56-57`, the one `impl From<fleet::turns::Tokens> for TokenUse`,
  moves to `fleet/turns.rs`. The orphan rule allows it because `Tokens` is local there.
- C2, `providers` → `guard`: `providers/claude/runs/mod.rs:261-262` call `guard::hook_events`. That file and
  the `hook_rules.rs` it uses (`guard/hook_events.rs:5`, 95 lines, self-contained) move to `threads/runs/`,
  beside the `hook_events` table (`threads/runs/hooks.rs:6`). `guard/hook_stats.rs:4` and
  `guard/hook_stats_tests.rs:3-4` import them from there.
- C3, `fleet` → `providers` and `ledger`, in production since 895bb0e6f. `fleet/registry/fixtures.rs:14-15` is
  now a `#[doc(hidden)] pub` module compiled into fleet's lib. It builds a real `Ledger` and `Claude`, and
  `north-tui`'s tests use it (`crates/north-tui/src/tui/fleet/desk.rs:235`). It moves out of fleet into
  north-providers as a `fixtures` module that tui's tests reach as a dev-dependency. Fleet's own tests that
  need the pair (`fleet/tick_tests.rs:18,33`, `fleet/registry/tests.rs:10,25`,
  `fleet/registry/senders.rs:96-97`) switch to fleet-local fakes of `Runtimes` and `Books`, or move to
  north-providers.
- C4, `ops/pacing` and `ops/agents` → `guard`: cut by box 5 (26eaddb97), not by moving the policy. `ops/pacing/placement/hosts.rs:156`
  and `ops/agents/goal/plan.rs:20` call `ops::pacing::spawn_gate`, which `main` installs from
  `guard::spawn_capacity::{launch_refusal, gate_summary, Live}`, so `ops` names no `crate::guard`. The policy and
  its tests stay beside the hook adapter in north-guard, which already sits above `ops`. Moving the policy into
  `ops/pacing` would drag the `World` trait, `usage`, `logs` and part of `briefs` along, all of which `decide` shares.
- C5, `capacity` → its host code, 11 production references:
  - `capacity/admission.rs:398` (`host::policy::aggregate_cpus`), `capacity/deadline.rs:100`
    (`host::runtime_root`) and `capacity/mod.rs:164` (`host::host_name`): these three primitives move into
    the low crate.
  - The `north capacity` subcommand dispatch, `capacity/mod.rs:76-109` (host, gate and watchdog), moves to
    north-capacity-host's entry, which the root CLI calls.
  - The `pub use host::land_check_cpus` re-export, `capacity/mod.rs:17`, is dropped.
    `ops/land_offload/mod.rs:160` then names the host crate, which north-runtime depends on.
- C6, `pace` ↔ `fleet`, from the usage shell (41a0162d9). `usage/shell/fleet_records.rs:9` reads
  `fleet::records`, and `usage/mod.rs:116` calls `fleet::registry::root_here()`, while `fleet/handover.rs:12,15,75`
  uses `usage::decide`. `FleetRecords` moves into fleet and implements usage's `Registry` trait, which the
  orphan rule allows. `usage/mod.rs:116`'s entry takes the registry from its caller, the root CLI. Then
  `pace` names no `fleet` item.

No other production edge breaks `c12.txt`. The `code_health` hits on `north-core` are string literals in
tests (`code_health/tests.rs:94,111`, `code_health/habits_tests.rs:43`). Box 1 already moved the test-only
back-edges into `ops`.

### Paths that break on a move

`north split move-module` keeps a top-level module's path inside `src/`. So the 48 `include_str!` paths with three or more
`../` and the same-crate `CARGO_MANIFEST_DIR` joins (`src/ops/check/fixtures` and the like) stay valid. Four
kinds of path cross a crate and break, and they surface only in main's `cargo.yml`, since `cargo test`
left the land gate at 2b5675065:

- `fleet/handoff.rs:52` reads `CARGO_MANIFEST_DIR/src/guard/worker_handoff.rs`, another crate's source.
- The shared `crates/north-core/src/fixtures/` serves four future crates:
  - `claude-stream/`: `claude_stream.rs:243-245` (fleet), `contract_probe.rs:310` (core) and
    `src/cli/fleet/router_tests.rs:236`.
  - `orchestration-hosts.toml`: `ops/runtime/pull/tests.rs:7`,
    `ops/land_offload/destinations/tests.rs:38,48`, `ops/laptop_runner/ssh_destination_tests.rs:11` and
    `src/cli/worker_ledger/pull/tests.rs:7`.
  - `staffing-overlap-2026-11-05.tsv`: `fleet/overlap_replay_tests.rs:7`.
- The doctest at `ops/land_offload/ship.rs:122-128` names `north_core::ops::...`.
- Root `tests/it` modules name moved paths: `fleet_wait_rows`, `fleet_spool`, `fleet_row_leased` and `job_run`
  (`north_core::fleet::{store, sender, records}`), `worker_ledger_record.rs:533` (`ops::defaults_measure`),
  and 62 more through `north_core::foundation::shim`.

Each move therefore runs `cargo test --no-run --workspace` and the moved crate's tests, not only
`cargo check`.

### Root integration tests (`tests/it`)

One binary of 110 modules and 21.5k lines. 85 modules drive the `north` binary through `CARGO_BIN_EXE_north`,
which Cargo defines only for the bin's own package. 27 `capacity_*` modules run
`harness/skills/machine-capacity/scripts/machine-capacity.mjs` through `bun` (`tests/it/capacity_host/mod.rs:115`).
63 files name a crate's library, mostly `north_core::foundation::shim`. So the binary stays in the root.

Its compile cost is small: 2.6 CPU-s after a north-core edit, with incremental on. Its run cost is large:
287.4 of the suite's 409.2 CPU-s. The lever is therefore per-crate selection of which `tests/it` modules
run, through a module-to-crate table, not moving files. `land_repositories` and `stub_exec_policy` (library
tests on `north_landing` alone) move to `crates/north-landing/tests/it`.

The table is `tests/it/crates.txt` (box 4-20), one line per module, merged as a set. It names planned crates,
so `crates/north-landing/src/land/slots/it_modules.rs` resolves a changed file through `structure.toml`'s
`[crate_dag]` globs, not through `cargo metadata`, and the selection holds before and after each move. A
module runs when one of its crates, or a crate whose `may_use` reaches one, changed. Any change outside
`crates/` except `docs/` runs every module. The lander exports the result as
`SAFE_PUSH_LAND_IT` (`--test it -- MODULE::` filters) to any check that names it. The selection follows the
planned DAG, so a back-edge still listed under "Edges to cut" can let a test outside the selection fail; the full
suite in main CI catches it.

### Lander selection

`crates/north-landing/src/land/slots/affected.rs` (lander box B, f9142b21b) already maps changed files to
packages from `cargo metadata` and adds the reverse-dependency closure for the clippy check. A path that
`north_base::build_inputs::rust_build_input` calls a Rust input but no member owns selects the whole
workspace. For example, `harness/defaults.toml`, which seven crates-to-be embed with `include_str!`. That is
safe but coarse. One gap selects nothing: three `include_str!` targets are missing from
`build_inputs.rs`'s `EMBEDDED` list. They are `docs/feedback-loop.md`, `harness/lib/fixtures/goal-eta-2026-10-12.json` and
`harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json`. The fix maps an
embedded file to the packages whose rustc dep-info (`target/debug/deps/*.d`) lists it, falling back to the
whole workspace without dep-info. A test then requires every `include_str!`/`include_bytes!` target to
be a Rust input.

### Conflict hotspots as registries

The lander's refused conflicts over the 24 h before 1:41 PM on 2026-11-05 were:

- `docs/features.md` 31, regenerated from `docs/features/` since 5295dff75 at 6:00 AM, so most of those
  predate the fix;
- `crates/north-landing/src/land/mod.rs` 28;
- `tests/it/land_serve.rs` 18;
- `tests/it/land_enqueue.rs` 14;
- `crates/north-landing/src/land/serve.rs` 14;
- `structure.toml` 7;
- `.safe-push` 7.

Each shared list becomes one line per entry, merged as a set by `harness/merge-sets.toml`, or one file per
entry:

- `land/mod.rs:34-47`'s `USAGE` and match become a one-line-per-subcommand table.
- `tests/it/land_serve.rs` (2,042 lines) and `land_enqueue.rs` become directories with one file per topic.
- `ops/mod.rs`, `capacity/mod.rs`, `fleet/mod.rs` and `guard/mod.rs` (25, 31, 19 and 15 commits) become pure
  `pub mod` lists merged as sets. Each new crate's `lib.rs` starts that way.
- `src/cli/verbs.rs` becomes a one-line-per-verb table.
- `structure.toml`'s `paths` arrays join the `toml-array` set beside `may_use`.

## Before the split, fad7445bd

Measured 2026-11-05 on gex45 under `north capacity host run --class heavy`, in a detached worktree at
fad7445bd, with `RUSTC_WRAPPER` unset and rustc/cargo 1.96.1 on PATH. Lines counts the 229,818 tracked `.rs`
files of the workspace. The host was contended (1-minute load about 19 at the start and 18 at the end on 20
threads), and the cold build ran once, so treat the timings as a single contended sample.

| Workspace | SHA | rustc | Lines | Cold build | Leaf build | Leaf test --no-run | Hub build | Hub test --no-run | Threshold | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| north | fad7445bd | 1.96.1 | 229,818 | 96.4 s | 66.9 s | 88.0 s | 66.9 s | 93.9 s | 88 s | near |

The cold build used 232.3 CPU-seconds (212.3 s user plus 20.0 s sys) over 87.4 s wall, measured in a
separate cold run with the same toolchain and an empty target directory.
