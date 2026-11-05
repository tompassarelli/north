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
  `remove_var`; `set_current_dir` appears only in `crates/north-guard/src/lanes/gc/mod.rs:146`, which no test reaches.
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
| 1 | `north target-seed` skips `incremental/` (superseded 2026-11-05: Warm worktree builds) | 0 | 0 | ~2.2 GiB per lane (measured) | seed copy shorter | 1 box | none: sessions keyed to other lanes' hashes are never read | copy it again |
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
lane is closing the gex45-to-laptop pull in `crates/north-runtime/src/runtime/pull.rs`. With `settles_fleet = true`
on whiterabbit, the laptop deploys to a gex45 set to `builds_runtime = false`
(`crates/north-runtime/src/land_offload/destinations.rs:27`).

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

Measured 2026-11-04 on the laptop under `--class runtime` leases (4 CPUs charged, 2 for the last row) with a fresh
`CARGO_TARGET_DIR` per setting: build 1b699074a, then dfe087b46, which changes one `north-core` file. User+sys is the
lease's `cpuSeconds` in `machine-capacity-usage.jsonl`, since the build runs in a systemd scope outside `time`'s
children. The flake's shellHook sets `RUSTFLAGS=-Cmetadata=checkout:<toplevel>`, so a per-commit `runtime-<sha8>`
path changes RUSTFLAGS and every unit recompiles, with incremental on or off; the on-row was therefore run on a stable
path only, and `--remap-path-prefix` was not needed. The build-cache gate (`crates/north-landing/src/build_cache_gate.rs:19`)
matches only a plain `CARGO_INCREMENTAL` set to one, so `CARGO_PROFILE_RELEASE_INCREMENTAL=true` passes it.

| B2 result: setting | crates compiled (registry) | user+sys s | wall s |
| --- | --- | --- | --- |
| B2 result: incremental off, per-commit path | 150 (146) | 792 | 354 |
| B2 result: incremental off, stable path | 4 (0) | 500 | 195 |
| B2 result: incremental on, stable path | 4 (0) | 35 | 58 |

The on-row is 93% below the stable off-row, past B3's 30% bar, so B3 landed with one stable `worktrees/runtime-build`
lane and `CARGO_PROFILE_RELEASE_INCREMENTAL=true` on the build command.

Incremental release state can go stale (north#575, 2026-11-06). On whiterabbit on 2026-11-05, runtime builds failed
to link four times with `rust-lld: undefined hidden symbol ... .llvm.N`. The unresolved symbols were internal to one
workspace rlib (north_core, north_guard or north_landing). Each streak started with the first build after a superseded
build was stopped mid-compile (exit 143). That happened at 11:51 AM, 1:32 PM, 4:21 PM and 9:17 PM. The broken state
then outlived every later sha. The hourly reclaim trims and the onsets did not line up, and the runtime-update lock and
cargo's build lock let only one cargo use the target at a time. Each build starts in a freshly added
`worktrees/runtime-build`, so cargo reruns rustc for every workspace crate each time. That leaves rustc's
`release/incremental` sessions as the only state that carries over from build to build, and that is where the stale
objects came from. Removing the workspace's release artifacts at 9:42 PM made the next build link, and no runtime build since
has failed that way. A build that fails with that signature now removes `release/incremental` and builds once more, so
incremental stays on. Each failed build keeps its compiler output in
`~/.local/state/north/runtime-build-logs/<sha>.log`, and its failure line names that file.

### B3. Turn on incremental for runtime builds (candidate 2, if B2's on-row is at least 30% below off)

Category: performance. Spec: measured. Scope: single-module. Surface: rust. Verify: local-test. Files:
`crates/north-runtime/src/runtime/update.rs` (and the stable path or remap B2 chose), `docs/features.md`.

Done when:
- `rg -n 'INCREMENTAL' crates/north-runtime/src/runtime/update.rs` prints the line on the build command.
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
- `rg -c 'check-owners.sh' .safe-push` prints 2 (the check and its `machine-bound` line).

### B7. Machine-bound checks reuse a held runtime (unscheduled)

Unscheduled until B5 shows the machine-bound compile share at 1 core-min or more per landing: the tests use the debug
build, so swapping in a release runtime would hide overflow and `debug_assert` failures.

### B8. gex45 stops building the runtime (candidate 3, after the pull closure lands)

Category: performance. Spec: measured. Scope: single-module. Surface: config. Verify: local-test. Files:
`~/code/south/main/agents/orchestration.toml` (`builds_runtime = false` on gex45). Orders after the lane closing the
gex45-to-laptop pull in `crates/north-runtime/src/runtime/pull.rs` lands.

Done when:
- Over 24 h, `ssh gex45 "rg '\"owner\":\"north-runtime\"' ~/.local/state/agents/machine-capacity-usage.jsonl" | jq
  'select(.cpuSeconds > 60)'` prints no record newer than the landing.
- `north runtime settle` prints `live and healthy` for gex45 after each landing in that window.

### B9. Land the `etxtbsy-shim` lane (candidate 6, prerequisite)

The Sonnet lane `etxtbsy-shim` already holds routing test script writes through `north_base::foundation::shim`; it is
blocked on the capacity wrong-tree bug. B9 is landing that lane once the bug is fixed.

Done when:
- `rg -l 'from_mode\(0o7|PermissionsExt|set_permissions' tests | xargs rg -L 'write_script|write_executable'` on
  origin/main prints nothing.

### B10. Merge the integration tests into `tests/it` (candidate 6, after B9)

Category: performance. Spec: measured. Scope: cross-module. Surface: rust. Verify: local-test. Files: `tests/*.rs`
moved to `tests/it/*.rs`, `tests/it/main.rs`, `tests/it/support/`, `harness/scripts/cargo-tests.sh` and
`land_test_runner.rs` if they name binaries.

Done when:
- `! rg -q 'set_current_dir' tests src crates --glob '!crates/north-guard/src/lanes/gc/mod.rs'` succeeds.
- `cargo test --locked --offline --no-run --message-format=json | jq -r 'select(.profile.test) | .executable' | rg -c
  .` prints the same command's origin/main count minus the merged files plus 1, the root-crate unit-test executable
  included in both.
- `cargo test --locked --offline` passes 3 runs in a row with the same test count as origin/main, and its wall time
  is not above origin/main's median of 3.

Result, 2026-11-04 (lane `merge-test-bins`, whiterabbit, seeded target, dev-shell cargo under lane-worktree
admission, one run each). Every `tests/*.rs` file and its helper directories moved to `tests/it/`, declared by
`tests/it/main.rs`; Cargo still autodiscovers, so a new `tests/*.rs` file builds and runs on its own, and the
`registry` test in the merged binary fails for any `tests/**/*.rs` file `tests/it/main.rs` does not reach, naming the
`mod` line to add. Measured after a warm `cargo test --no-run`, then appending a blank line to
`crates/north-core/src/lib.rs` and timing `cargo test --locked --offline --no-run`:

| | origin/main 1f12bb98d | merged |
| --- | --- | --- |
| Integration test executables | 82 | 1 |
| All test executables (incl. 3 lib and 2 bin unit-test harnesses) | 87 | 6 |
| Relink wall after the north-core edit | 40.4 s | 13.7 s |
| Integration executable bytes | 1,055 MB | 88 MB |
| All test executable bytes | 1,412 MB | 445 MB |
| Tests listed by the integration executables | 274 | 275 (274 plus the registry test) |

`target/debug` read 9.9 GiB before and 11 GiB after with the stale 82 executables still present; deleting them
leaves the difference to the executable bytes above. Planting `tests/it/planted.rs`, `tests/planted_top.rs` and
`tests/it/runtime_update/orphan.rs` made the registry test fail with one line per file, and removing them made it
pass.

Rebased onto origin/main 09691aecf, 2026-11-04 (gex45 offload): the 86 `tests/*.rs` files there list 331 tests
summed over their separate binaries; `cargo test --test it -- --list` lists the same 331 names under their file
modules plus the registry test, 332, and `cargo test --test it` passes all 332 with none ignored.

### B11. Code-only lanes build on gex45 (candidate 7, after the lane-tree offload fix lands)

Category: performance. Spec: measured. Scope: single-module. Surface: config. Verify: local-test. Files: placement
config named by the offload fix.

Done when:
- Over 24 h, laptop `heavy`-class `cargo test`/`cargo build` runs from lane owners in the usage log fall below 20
  core-min, and gex45's rise by about the same.

### Shared registry artifacts for land checks (north#360)

Cause: the dev shell set `RUSTFLAGS=-Cmetadata=checkout:<toplevel>` whenever `CARGO_TARGET_DIR` was set (54143fd25,
north#161, against another checkout's build into a shared target reading as fresh), and `north target-seed` skipped
any checkout with it set. The land checks set `CARGO_TARGET_DIR=target`, the lane's own target, so each fresh lane
compiled all 146 registry crates and its land checks, keyed to its path, never matched its own unkeyed builds. Now the
key applies only to a target outside the checkout, and target-seed seeds a checkout's own target.

Shared-registry result (2026-11-04, laptop under 25 to 30 load, cargo `Finished` seconds, queue time excluded):

| Run in a fresh worktree, `CARGO_TARGET_DIR=target` | Compiling lines | Registry crates | cargo s |
| --- | --- | --- | --- |
| before, `cargo build --bin north` | 150 | 146 | 47 |
| before, check-owners `cargo run` | 150 | 146 | 333 |
| after, `cargo build --bin north`, seeded | 4 | 0 | 81, 85 |
| after, check-owners `cargo run`, seeded | 4 | 0 | 87 |

- Wall time is bound by north-core (about 77 s on the critical path, as measured above), so removing the registry
  crates cuts core-seconds more than wall time; the before rows ran under different load, so the walls are not a
  like-for-like comparison.
- The seed takes the newest idle sibling target; until every lane rebases past this change, that target may hold
  only path-keyed registry artifacts from an old land check, and a seed from it still compiles all 150.

### Warm worktree builds (north#360, 2026-11-05)

Cause: fleet workers build with `depbox run -- nix develop -c cargo ...` (AGENTS.md). depbox hides `~/.local` and the
sibling checkouts, so the shellHook's `north target-seed` found no `north` and every new lane compiled all 158 crates;
and B1's seed dropped `incremental/`, so even a seeded lane recompiled north-core from scratch. Copying a sibling's
`incremental/` did not help on its own: rustc keeps a session only when the build runs at the same checkout path.
Now depbox runs `north target-seed` before its sandbox and lease, the seed keeps `incremental/`, and depbox mounts the
checkout a second time at `~/code/<project>/sandbox` and builds there, so every lane of a project compiles at one
path. B1 is superseded.

Result: whiterabbit (24 threads), a fresh north worktree with one blank line appended to
`crates/north-guard/src/lanes/target_seed.rs`, `date +%s.%N` deltas around the whole command, one run per row:

| Run in the fresh worktree | Seed | Load | Crates compiled | Wall s |
| --- | --- | --- | --- | --- |
| before, `depbox run -- nix develop -c cargo test -p north-core --no-run` | none (hook finds no `north`) | 17 | 158 | 102.0 |
| before, `nix develop -c cargo test -p north-core --no-run` | sibling, no `incremental/` | 8 to 13 | 11 | 109.8 |
| sibling's `incremental/` kept, other path | sibling built at its own path | 17 to 23 | 3 | 112.0 |
| sibling's `incremental/` kept, same path | sibling built at this path | 20 to 25 | 2 | 14.4 |
| in-place rebuild, no seed | the lane's own target | 20 to 25 | 2 | 23.0 |
| after, `depbox run -- nix develop -c cargo test -p north-core --no-run` | lander lane built at its real path | 15 | 3 | 72.6 |
| after, same command | sibling built in the sandbox | 11 | 2 | 16.4 |

- The 72.6 s row is the transition: a seed built before this change is still at its real path; land checks run
  through `land-shell` and so depbox, so lander lanes become sandbox-path seeds after their next check.
- The seed copy is not the cost: `cp -a --reflink=auto` of a 2.2 GiB lane target took 3.1 s on gex45's ext4 at
  load 26 and 0.1 s on the laptop's btrfs, so the seed stays synchronous; a background copy would race the first
  build for `target/`.
- sccache is already the `RUSTC_WRAPPER` on both hosts (firn module `sccache`) and depbox passes it; the cold row
  still took 102 s because north-core is the critical path and sccache skips incremental crates.
- Disk: a seeded lane now carries the sibling's `incremental/` (1.6 GiB for north on the laptop), shared by reflink on
  btrfs and copied on ext4; the reclaim sweep still removes idle lane targets after 4 h.
- A per-worktree `CARGO_TARGET_DIR` (option b of the brief) was not built: depbox already pins each checkout's own
  `target/`, and a target outside the checkout gets the `-Cmetadata=checkout:` key that blocks reuse.

Order: B1, B2, B4 and B5 run in parallel; B3 follows B2, B6 follows B5, B8 follows the pull closure, B10 follows B9,
B11 follows the offload fix.

### Shared compiler cache across hosts (north#259, 2026-11-05)

Cause: the laptop, gex45 and ccx33 each kept their own sccache disk cache, so a crate compiled on one host was
compiled again on the others. Now the firn module `sccache` adds a shared level behind the local disk: nexus serves
it (`sccache.server`), and the other hosts read and write it (`sccache.remote`). Only the laptop uses it for now: the cross-host
measurement below made gex45 and ccx33 slower, and they rejoin once the store serves from Finland.

Shape, and why each piece is there:

- Store: `rclone serve webdav` on nexus at 10.77.0.1:8379, on disk under `/var/lib/sccache-cache`, capped at 20 GiB.
  An hourly sweep deletes the entries read longest ago. nexus is the wg-nexus hub, and the hub is the one address
  every client reaches: it forwards nothing between clients (`net.ipv4.ip_forward = 0`), so the laptop cannot reach
  gex45 or ccx33 over wg-nexus. The nftables rule accepts port 8379 only from 10.77.0.2, .4 and .5. WireGuard ties each
  source address to one peer key, held in sops, so no new credential exists, and the phone (.3) is shut out.
- Client relay: a local nginx on 127.0.0.1:16380 proxies to the store with a 1 s connect timeout and a 3 s read
  timeout, and answers every upstream error as 404. That 404 is what makes an outage harmless. sccache's storage
  layer (opendal) retries a "temporary" error for tens of seconds per read, but treats a 404 as a final miss.
- Sandbox bridge: depbox runs builds with no network. The firn module bridges `/run/sccache-remote/http.sock` to the
  relay, and when the rustc wrapper finds no relay on 127.0.0.1:16380 it starts a loopback-to-socket bridge inside
  the sandbox. depbox needs no change for this.
- The wrapper adds the WebDAV level (`SCCACHE_MULTILEVEL_CHAIN=disk,webdav`) only when it is about to start an sccache
  server and the relay accepts a connection. A level that cannot be built stops sccache's server from starting, and
  the build then fails.
- The wrapper also roots `SCCACHE_BASEDIRS` at the checkout named by rustc's `--out-dir` (formerly only the CMake
  launcher rooted it, by working directory), with one sccache server per checkout. Without that, a lane's in-checkout
  `target/` paths enter the key, and no two worktrees share an entry. depbox builds already share one path
  (`~/code/<project>/sandbox`).

Outage measurements: ccx33, a 5-crate project (serde_json, itoa, memchr), a fresh target and an empty disk level,
one run each, all exit 0. The outages were simulated in a user network namespace.

| Backend, state | Wall s | Rust hits |
| --- | --- | --- |
| WebDAV via relay, populate | 3.85 | 0 of 5 |
| WebDAV via relay, disk level wiped | 1.84 | 5 of 5 |
| WebDAV via relay, packets to the store dropped by nftables mid-session | 14.8 | 0 |
| WebDAV via relay, server started while dropped | 5.6 | 0 |
| WebDAV via relay, store stopped | 3.6 | 0 |
| Redis direct, endpoint unreachable at server start | 20.5, exit 101 | |
| Redis via socat relay, Redis frozen (SIGSTOP) | 182.5 | 0 |
| Redis via socat relay with TCP_USER_TIMEOUT 3 s, packets dropped | 182.7 | 0 |

- Redis was the first choice, rejected on the last three rows: opendal retries the unreachable level past the
  client's 10 s server-start limit, and sccache 0.15 has no setting for either. `redis+unix://` endpoints are refused
  as `invalid format`.
- bazel-remote (an LRU HTTP cache) refuses sccache's paths with 400 `resource name must be a SHA256 hash`, and nginx's
  stock DAV module answers sccache's PROPFIND with 405, so writes fail. rclone accepts both.

What the cache can and cannot save, from `cargo build --locked --offline -p north` on ccx33 at north
11f5152f7, under a 3-CPU heavy lease, with the target at one fixed path so keys match:

| Build | Wall s | Rust hits |
| --- | --- | --- |
| incremental (default), empty cache | 72.6 | 0 of 142 |
| incremental, every cacheable crate cached | 53.9 | 142 of 142 |
| `CARGO_INCREMENTAL=0`, empty cache | 66.1 | 0 of 145 |
| `CARGO_INCREMENTAL=0`, all cached | 12.6 | 145 of 145 |
| incremental, one-line north-core edit after a build | 13.6, 12.3 | |
| `CARGO_INCREMENTAL=0`, the same edit | 44.6, 42.9 | |

- sccache passes incremental crates through, and north's four workspace crates are incremental by default. A full
  cache hit therefore cuts a fresh lane's build only to 74% of cold. The rest is north-core, where `target-seed`
  (warm worktree builds, above) is the lever on one host.
- Turning incremental off makes north-core cacheable (12.6 s, 19% of cold) but adds about 30 s to every north-core
  edit build. Lanes keep incremental for that reason.
- Land checks stay incremental too. Lander lanes are the newest seeds for new worker lanes, and a seed without
  north-core's incremental session sends the worker's first build back to about 72 s.
- A first build with a different `CARGO_TARGET_DIR` hit 0 of 142 on the same host and cache, because target paths
  outside the basedir enter the key.

Cross-host result (2026-11-05, north 86618706c, `cargo build --locked --offline -p north` in a fresh worktree with a
pre-created `target/` so target-seed copies nothing, an empty local disk level, one run each):

| Host, run | Wall s | Rust hits | Average cache read hit | Average compile |
| --- | --- | --- | --- | --- |
| laptop, shared level, first build of the commit | 68.6 | 0 of 142 | | |
| ccx33, plain sccache, cold | 76.2 | 0 of 142 | | 0.387 s |
| ccx33, shared level after the laptop's build | 103.7 | 115 of 142 (81%) | 0.608 s | 0.487 s |

- The keys match across hosts and worktree names; the 27 misses were not traced.
- From Finland, a hit costs more than the compile it saves. nexus is in Singapore, 178 ms per round trip from ccx33
  (62 ms for a whole GET from the laptop), and a WebDAV read takes several round trips. gex45 and ccx33 are off the
  shared level until the store serves from Finland (north#259, box shared-cache-fi). The laptop keeps it.

### The head-north checks share one build per candidate (north#634, 2026-11-06)

Each `bin/head-north` land check runs the host's live runtime when the candidate leaves its `--code` paths as that
runtime has them, and otherwise ran the lane's `north`, built in its own offload slot. The checks run in parallel in
separate gex45 slots with separate `target/` directories, so a candidate that changed any checker built north once
per check: in the 7 days to 2026-11-06 the six checks took 62 to 97 s each at p90 against 3 to 17 s at p50, and a
candidate touching every checker's code took 78 to 164 s per check on gex45.

- The runner adds one `head-north --code UNION --build`, the union of the `--code` lists of the head-north checks
  the candidate selects (`land/slots/shared_build.rs`). In the after-landing run it starts beside every other check
  and the head-north checks start once it passes, still beside the rest; before a push with clippy listed it runs
  beside clippy. All of it uses one runner host, and when the live runtime covers the union the build is a no-op.
- `bin/head-north` keeps the build per host under `~/.cache/north/head-north/TREE/north`, keyed by the clean
  checkout's tree and built under a lock, so a second slot or a second check on the same tree runs it; a dirty tree
  builds in place as before. It keeps the 4 most recently used trees and sets `NORTH_HOME` to the checkout, which is
  where the in-tree `target/debug/north` resolved its harness.
- The `citation-check.sh` and `check-owners.sh` wrappers are inlined into `.safe-push`, so every head-north check
  names its `--code` list where the runner reads it.

Measured on gex45 (2026-11-06) with a candidate on origin/main b9fd1e346 that touches every checker's code, warm
slots: before, each check built north in its own slot and took citation-check 78 s, check-owners 97 s, code-health
layers 112 s, experiments 124 s, features-index 135 s and rationale-index 164 s; after, the shared build took one
slot and the same six checks ran from it in 3.6, 2.3, 0.1, 0.1, 1.7 and 0.1 s (check-owners, layers,
features-index, rationale-index, citation-check, experiments).
