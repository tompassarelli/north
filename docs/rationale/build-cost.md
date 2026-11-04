# Build cost: where north's compiles burn cores, and what to cut first

Plan of 2026-11-04 (laptop time, +07), revised the same day after review. Tom asked for the smartest durable cut of
north's compile cost per lane, per landing and per runtime promotion, build caches included (2026-11-04). Nothing here
is built yet; each candidate is a box below.

## Measured baseline, 24 h to 5:45 AM 2026-11-04

Sources: leased runs in `~/.local/state/agents/machine-capacity-usage.jsonl` (laptop and gex45, `cpuSeconds` summed
by owner), watchdog samples in `~/.local/state/agents/watchdog/samples*.jsonl`, the `north-runtime-follow` journal,
kept failure logs in `~/.local/state/safe-push/land-check-*.log`, one Actions `Land check` run log (head 8275256ca,
base 566e9007d), lane `target/` directories, one timed lane build (below), and `git log aeabc8451..origin/main` (134
commits since the crate split on 2026-10-31).

| Consumer | Host | Runs | Core-min/day | Note |
| --- | --- | --- | --- | --- |
| Local land checks (`owner = land-check`), all repositories | laptop | 115 | 622 | records carry no repository; includes `.github/scripts/land-checks.sh drift`/`native` runs and lua failures |
| Runtime promotion (`owner = north-runtime`, `cargo build --release`) | laptop | 41 | 401 | 24 exit 0 (median 659 cpu-s, 204 s wall); 16 killed by supersede (73) |
| Runtime promotion | gex45 | 45 | 289 | 41 exit 0; mostly the same commits, built a second time |
| Lane `cargo` runs not named for another repository | laptop | 28 | 75 | |
| Lane `cargo test`/`cargo build` | gex45 | 35 | 20 | all repositories |

- The laptop averaged 4.3 busy cores (6,162 core-min in watchdog samples), 1,477 of them leased.
- 15 promotions in 24 h on the laptop, 3 of them pulled (one from gex45 at 4:33 AM, two from Nexus); 52 main commits.
- Failed land checks: 38 failed runs (359 core-min) fell in one hour, 4:55 to 5:43 AM on 4 Nov, all red on
  `bin/threads.test.sh`; b4c690bc5 fixed the cause, so this is not recurring cost.
- b4c690bc5 also widened the `.safe-push` machine-bound selection to `crates/`: 103 of 134 commits change Rust inputs
  and now each also runs `cargo build --bin north` plus the machine-bound tests. The 24 h window predates most of it,
  so the land-check row understates today's cost.
- `test-support` splits north-core: lane `defaults-honest` holds north-core `lib` built with features `[]` and
  `["test-support"]` under the same dev profile hash, two 233 MB rlibs and six north-core incremental sessions
  (0.16 to 1.17 GB each). On Actions, `cargo test --no-run` compiled north-core a second time after `cargo build`.
- Churn: 85 of 134 commits touch north-core, and only 4 of those touch no module in the
  `fleet`/`ops`/`capacity`/`threads`/`guard`/`providers`/`ledger`/`usage`/`health` cluster (88.8k of 125.9k lines).
- Tests: 36 files match `rg -l 'from_mode\(0o7|PermissionsExt|set_permissions' tests`. No code calls `set_var` or
  `remove_var`; `set_current_dir` appears only in `crates/north-core/src/lanes/gc/mod.rs:146`, which no test reaches.
  Two static mutexes: `FORK_GATE` (`tests/job_run.rs:21`) and `EXEC` (`tests/capacity_host/mod.rs:12`).

Timed lane build (whiterabbit, origin/main f9aed891e, fresh lane seeded by `north target-seed`, heavy lease of
6 CPUs, `cargo test --no-run --timings`, one run):

- 80.2 s cargo wall; 928 s including 14 min queued behind 3 heavy leases.
- The critical path is the north_core lib test harness, 77.4 s of the 80.2 s; the 83 integration binaries compile in
  22.8 s wall on spare cores and finish 6 s before it.
- The linker is already rustc's bundled rust-lld (LLD 22.1.2).
- `target/debug` is 7.7 GiB: `incremental/` 4.2 GiB, of which 2.2 GiB are north_core sessions copied from the seed
  lane that this lane never reads; test executables 1.9 GiB.

## Ranked candidates

Core-minutes are per day. Laptop minutes count first: gex45 sits mostly idle, the laptop runs Tom's clients.

| # | Candidate | Laptop | gex45 | Disk | Wall per landing/lane | Cost | Risk | Reverse |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `north target-seed` skips `incremental/` | 0 | 0 | ~2.2 GiB per lane (measured) | seed copy shorter | 1 box | none: sessions keyed to other lanes' hashes are never read | copy it again |
| 2 | Runtime release builds incremental | ~200 (est. 50% of 401, unmeasured) | 0 once gex45 stops building | +1-2 GB in `~/.cache/cargo-target/north` (est.) | promotion ~100 s sooner (est.) | 2 boxes | no reuse: each build is a fresh `runtime-<sha8>` worktree path | drop the env |
| 3 | The laptop is the only runtime builder | 0 | 289 freed headroom | 0 | +0-2 min on gex45 (settle deploy) | 1 config box | none | one config line |
| 4 | Drop north-core's `test-support` feature | ~60 (est., one north-core debug compile per lane build/test pair) | ~10 | 0.4-1.4 GB per lane (measured) | one north-core compile fewer per build/test pair | 1 box | ~60 lines of test doubles become unconditionally `pub` in the binary | re-add the feature |
| 5 | check-owners reuses a held runtime binary | unmeasured; low hit rate (77% of commits change Rust inputs, runtime lags main) | 0 | 0 | unmeasured | 1 measure + 1 box | none for check-owners | revert `.safe-push` lines |
| 6 | Merge the integration-test binaries into `tests/it` | ~30 (est.) | ~10 | ~1.5 GB per built lane (measured executables) | maybe none: they already build off the critical path, and one binary loses `land_test_runner.rs`'s per-executable parallelism | 2 boxes | `FORK_GATE`/`EXEC` serialize across former files | split back |
| 7 | Code-only lanes build on gex45 by default | ~75 (moves, not saves) | -75 | lane targets move to gex45 | none | 1 box after the offload fix | depends on the lane-tree offload fix in flight | placement config |
| 8 | Split north-core further along `structure.toml` | <5% of north-core rebuilds (4 of 85 commits leaf-only) | | | | days | | rejected for now |

The recurring fix for land checks rerunning after a red test is the landing train (`docs/rationale/landing-train.md`,
in design), not a candidate here.

Candidate 3 is decided: the laptop builds the runtime and gex45 never supplies a binary the laptop runs; a separate
lane is closing the gex45-to-laptop pull in `crates/north-core/src/ops/runtime/pull.rs`. With `settles_fleet = true`
on whiterabbit, the laptop deploys to a gex45 set to `builds_runtime = false`
(`crates/north-core/src/ops/land_offload/destinations.rs:27`).

Rejected, one line each:

- A faster linker: rust-lld is already in use.
- sccache: measured slower than `target-seed` on 31 Oct
  (`agent-machinery/skills/rust-development/references/compile-times.md`, both tables).
- Shared `CARGO_TARGET_DIR` for lanes: every lane waits on Cargo's build-directory lock.
- An Actions-built runtime: the runner is Ubuntu, so the binary links FHS glibc while promotion realises Nix store
  references; it needs Nix and a binary cache on Actions, a new dependency.
- Promoting the land check's debug `north` as the runtime: opt-level 0 north and bundled SQLite would run every hook.
- Turning off incremental in lanes: frees 3.7 to 5.4 GB per lane but slows every edit loop (compile-times.md keeps
  it on).

## Boxes

Independent boxes first; each runs alone in its own lane. Times are budgets.

### B1. `north target-seed` skips `incremental/` (candidate 1)

Category: performance. Spec: measured. Scope: single-module. Surface: rust. Verify: local-test. Files: the
`target-seed` command under `crates/north-core/src/`, `docs/features.md` (Lane target seed).

Done when:
- In a fresh lane after `north target-seed`, `test -e target/debug/incremental && fd -d1 . target/debug/incremental`
  prints nothing.
- `du -sb target` in that lane is at least 2 GiB below the seed lane's.
- `cargo test --locked --offline` in that lane passes.

### B2. A/B the runtime release build with incremental on (candidate 2, measurement)

Category: performance. Spec: measured. Scope: single-module. Surface: rust. Verify: local-test. Files:
`docs/rationale/build-cost.md` (results table).
Method: two detached worktrees at two consecutive main commits on different `runtime-<sha8>` paths; per setting, a
fresh `CARGO_TARGET_DIR` under a `runtime`-class lease, build the older commit, then time the newer one with
`/usr/bin/time -f '%U %S %e' nix develop -c cargo build --release --locked`, incremental set with
`CARGO_PROFILE_RELEASE_INCREMENTAL=true` (the build-cache gate at `crates/north-landing/src/build_cache_gate.rs:19` matches only plain `CARGO_INCREMENTAL` set to one; docs/features.md
names `CARGO_BUILD_INCREMENTAL=true` as the convention). If the on-row shows no reuse across paths, repeat it once
with a stable worktree path and once with `--remap-path-prefix` before dropping candidate 2.

Done when:
- `rg -n 'B2 result' docs/rationale/build-cost.md` on origin/main prints a table row per setting with user+sys and wall
  seconds of the second build.

### B3. Turn on incremental for runtime builds (candidate 2, if B2's on-row is at least 30% below off)

Category: performance. Spec: measured. Scope: single-module. Surface: rust. Verify: local-test. Files:
`crates/north-core/src/ops/runtime/update.rs` (and the stable path or remap B2 chose), `docs/features.md`.

Done when:
- `rg -n 'INCREMENTAL' crates/north-core/src/ops/runtime/update.rs` prints the line on the build command.
- `cargo test --locked --offline --test runtime_update` passes.
- After landing, the median `cpuSeconds` of the next 5 exit-0 `north-runtime` runs in the laptop's
  `~/.local/state/agents/machine-capacity-usage.jsonl`, skipping any run that follows a `north reclaim` sweep of
  `~/.cache/cargo-target/north` (a cold target), is at most 460 (70% of the 659 s baseline).

### B4. Drop the `test-support` feature (candidate 4)

Category: performance. Spec: measured. Scope: cross-module. Surface: rust. Verify: local-test. Files:
`Cargo.toml` (enable at :58), `crates/north-core/Cargo.toml` (feature at :33), `crates/north-core/src/fleet/claims.rs`
(gates on `Held` at :72, :76), `crates/north-core/src/fleet/watch.rs` (`TurnWatch::idling_by` at :201),
`crates/north-core/src/fleet/registry/mod.rs` (`mod testing` at :54); no edit expected in the only consumers,
`src/cli/fleet/{claims,list,router_tests,silent_tests,detached_tests,placement_tests}.rs`. `Fleet::at`, `Held` and
`Unrecorded` become unconditionally `pub`.

Done when:
- `! rg -q 'test-support' Cargo.toml crates/` succeeds.
- In the lane, after `cargo test --locked --offline --no-run`, `! cargo build --locked --offline --bin north 2>&1 |
  rg -q 'Compiling north-core'` succeeds.
- `cargo test --locked --offline` and `cargo clippy --all-targets --locked --offline -- -D warnings` pass.

### B5. Measure what the local land checks spend (candidate 5, measurement)

Category: performance. Spec: measured. Scope: single-module. Surface: harness. Verify: local-test. Files:
`docs/rationale/build-cost.md` (results table). Method: for 3 north commits landed on 2026-11-04, a lane at the
commit's parent with a built `target/`, then checked out at the commit; run each local `land.check` command under
`/usr/bin/time -f '%U %S %e'` with `SAFE_PUSH_LAND_BASE`/`SAFE_PUSH_LAND_HEAD` set, under a moderate lease.

Done when:
- `rg -n 'B5 result' docs/rationale/build-cost.md` on origin/main prints a table with user+sys and wall seconds per
  local check per commit, splitting check-owners' `cargo run` compile from its check and machine-bound's build from
  its tests.

B5 result (2026-11-04, lane seeded by `north target-seed`, built at 12da0de8a, then stepped through the three
commits in order under one moderate lease; user+sys core-seconds / wall seconds; dev-shell was selected for none):

| Commit | Changes | check-owners compile | check-owners check | machine-bound build | machine-bound tests |
| --- | --- | --- | --- | --- | --- |
| 2fdb333f1 | `harness/machine-bound-tests` | 0.5 / 0.6 | 1.3 / 4.2 | 0.1 / 0.9 | 0.1 / 0.2 (none selected) |
| 32da14e68 | `land_offload/ship.rs` | 29.2 / 38.2 (north-core, north) | 3.9 / 7.9 | 0.1 / 0.7 | 1.2 / 1.0 |
| 1b699074a | `land_offload/ship.rs` | 14.8 / 28.8 (north-core, north) | 1.5 / 7.8 | 0.1 / 2.7 | 1.4 / 3.3 |

- machine-bound's `./bin/land-shell --dev -- cargo build --bin north` is a no-op after check-owners' `cargo run`:
  both build the same debug `north` in the same `target/`, so check-owners pays the landing's compile.
- One-commit steps from a warm target put the compile at 0.25 to 0.5 core-min, under B6's gate. Real landings sit
  higher, because a lane's target is built at its fork point and the landing head is rebased past it: the laptop's
  usage ledger, 6:24 to 7:32 AM, holds 96 check-owners runs at 10,250 cpu-s (mean 107, 1.8 core-min; 39 over 60 s)
  and 83 machine-bound runs at 3,369 cpu-s. B6 goes ahead on the ledger number.
- B6 saves the compile only where machine-bound builds nothing: lanes outside its path filter (11 of 75 main
  commits today) and lanes whose diff selects no machine-bound test, as 2fdb333f1 did. Elsewhere the same compile
  moves into machine-bound's build; B7 owns that share.

### B6. check-owners runs a held runtime binary unless the checker changed (candidate 5, if B5 puts the compile at 1 core-min or more)

Category: performance. Spec: measured. Scope: single-module. Surface: harness. Verify: local-test. Files:
`harness/scripts/check-owners.sh` (new, glue under 20 lines), `.safe-push`. Rule: take the sha `current` names under
`~/.local/state/north/runtimes/`; if `git diff --quiet SHA "$SAFE_PUSH_LAND_HEAD" -- crates/north-core/src/catalog
crates/north-core/src/agent_catalog.rs crates/north-base Cargo.lock` holds, run that binary, else `cargo run`.

Done when:
- In a lane whose diff touches only `docs/`, the script with base and head set exits 0 in under 10 s and prints no
  `Compiling`.
- In a lane touching `crates/north-core/src/catalog/registration_checks.rs`, its output shows `Finished` or
  `Compiling north`.
- `rg -c 'check-owners.sh' .safe-push` prints 2 (the check and its `local-only` line).

### B7. Machine-bound checks reuse a held runtime (unscheduled)

Unscheduled until B5 shows the machine-bound compile share at 1 core-min or more per landing: the tests use the debug
build, so swapping in a release runtime would hide overflow and `debug_assert` failures.

### B8. gex45 stops building the runtime (candidate 3, after the pull closure lands)

Category: performance. Spec: measured. Scope: single-module. Surface: config. Verify: local-test. Files:
`~/code/south/main/agents/orchestration.toml` (`builds_runtime = false` on gex45). Orders after the lane closing the
gex45-to-laptop pull in `crates/north-core/src/ops/runtime/pull.rs` lands.

Done when:
- Over 24 h, `ssh gex45 "rg '\"owner\":\"north-runtime\"' ~/.local/state/agents/machine-capacity-usage.jsonl" | jq
  'select(.cpuSeconds > 60)'` prints no record newer than the landing.
- `north runtime settle` prints `live and healthy` for gex45 after each landing in that window.

### B9. Land the `etxtbsy-shim` lane (candidate 6, prerequisite)

The Sonnet lane `etxtbsy-shim` already holds routing test script writes through `north_base::foundation::shim`; it is
blocked on the capacity wrong-tree bug. B9 is landing that lane once the bug is fixed.

Done when:
- `rg -l 'from_mode\(0o7|PermissionsExt|set_permissions' tests | xargs rg -L 'write_script|wait_until_executable'` on
  origin/main prints nothing.

### B10. Merge the integration tests into `tests/it` (candidate 6, after B9)

Category: performance. Spec: measured. Scope: cross-module. Surface: rust. Verify: local-test. Files: `tests/*.rs`
moved to `tests/it/*.rs`, `tests/it/main.rs`, `tests/support/`, `harness/scripts/cargo-tests.sh` and
`land_test_runner.rs` if they name binaries.

Done when:
- `! rg -q 'set_current_dir' tests src crates --glob '!crates/north-core/src/lanes/gc/mod.rs'` succeeds.
- `cargo test --locked --offline --no-run --message-format=json | jq -r 'select(.profile.test) | .executable' | rg -c
  .` prints the same command's origin/main count minus the merged files plus 1, the root-crate unit-test executable
  included in both.
- `cargo test --locked --offline` passes 3 runs in a row with the same test count as origin/main, and its wall time
  is not above origin/main's median of 3.

### B11. Code-only lanes build on gex45 (candidate 7, after the lane-tree offload fix lands)

Category: performance. Spec: measured. Scope: single-module. Surface: config. Verify: local-test. Files: placement
config named by the offload fix.

Done when:
- Over 24 h, laptop `heavy`-class `cargo test`/`cargo build` runs from lane owners in the usage log fall below 20
  core-min, and gex45's rise by about the same.

Order: B1, B2, B4 and B5 run in parallel; B3 follows B2, B6 follows B5, B8 follows the pull closure, B10 follows B9,
B11 follows the offload fix.
