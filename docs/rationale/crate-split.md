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
- `north-core`: everything else (`fleet`, `ledger`, `guard`, `threads`, `ops`, `providers`, ...).
  Depends on `north-base`; nothing in it uses `north-landing`.
- `north-tui`: `tui`, the fleet terminal screen. Depends on `north-core`; nothing in `north-core` uses it,
  so a screen edit rebuilds `north-tui` and the root package while `north-core` stays cached.
- `north` (the root package): `src/main.rs`, `src/cli/` and the `verify-north` and `fake_claude`
  binaries, plus the integration tests. Depends on `north-core`, `north-landing` and `north-tui`.

A leaf edit in `north-landing` rebuilds `north-landing` and the small root package; `north-core` stays
cached. An edit in `north-core` still rebuilds `north-core` and the root package, which is the next split's
target: `ops` (40k lines) leaves `north-core` only after `capacity`, `threads` and `health` stop pointing back
into it.

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
the repo's path mentions; lets the `crate-roots` module of `structure.toml` use what the re-exports
point at; and widens to `pub` only the items `cargo check` reports as private. To rebase a move, reset
to the new main and run it again rather than merging by hand.

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
cap; crates already over (north-core) may shrink or stay but not grow. The refusal names where the new
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
