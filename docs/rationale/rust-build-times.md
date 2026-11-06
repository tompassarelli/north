# Rust build times: a measured trigger for crate splits

Tom asked on 2026-11-04 that Rust compile times stay fast as Wisp grows. Splitting a crate costs a
`pub` surface, a manifest and moved code, and pays only where the edit-to-result loop is slow. So a
split is taken when the measured loop crosses a fixed cap, not before: every workspace's leaf-edit
`cargo test --no-run` must take at most 10 s. This file records the method, the cap and why it is
absolute, the first measurement of every Rust workspace agents edit, and the split each one gets.

## Method

`north code-health build-time --repo DIR [--workspace REL [--leaf REL --hub REL]] [--samples N] [--ledger FILE]`
measures one workspace per `[[build_time]]` entry in the repo's `structure.toml` (or the one named on the
command line):

1. A cold `cargo build --timings` in a fresh scratch target under `WORKSPACE/target/build-time-*`, with
   `RUSTC_WRAPPER` removed so sccache cannot serve cached crates. The critical path is read from the
   `UNIT_DATA` table of `cargo-timing.html`: from the last unit to finish, back through the unit whose
   unblock came last; units under 1 s are left out of the printed chain.
2. One unmeasured `cargo test --no-run` to fill the test profile.
3. N samples (default 3) of a one-line edit, `const _: u64 = K;` appended to the leaf module, each
   followed by a timed `cargo build` and a timed `cargo test --no-run`; the median is recorded. The file is
   restored, both commands run once unmeasured, and the same is done for the most-depended-on (hub)
   module.
4. One TSV row per workspace is appended to the ledger (default `~/.local/state/north/build-times.tsv`):
   time, host, repo, workspace, SHA, rustc, samples, the five wall times, threshold, verdict, critical
   path, the host contention (the larger of the 1- and 5-minute load averages per hardware thread,
   read before the first build), and the effective `incremental` and `debuginfo` of the test profile,
   each with the setting that decided it (`CARGO_INCREMENTAL`, `CARGO_BUILD_INCREMENTAL` or a
   `build.incremental` in a `.cargo/config.toml` from the workspace up to `$CARGO_HOME`, then the
   `test` and `dev` profiles from env, config and `Cargo.toml`, then cargo's dev default), so a
   non-incremental host shows as such in the row and the printed report.
   A run with contention above the workspace's `contention_ceiling` takes the verdict `contended`, exits 0
   and ticks, edits and files nothing, because a loaded host times its neighbours, not the crate layout:
   202 s at load 25 on 20 threads against 43.8 s quiet.
5. The verdict compares the leaf-edit `cargo test --no-run` median with the workspace's limit: `over`
   when above it (exit 1), `near` from 75% of it, `within` below. The limit is the 10 s cap
   (`LEAF_TEST_NO_RUN_SECONDS_MAX` in `crates/north-core/src/build_time/threshold.rs`) unless
   `test_no_run_seconds` is lower. A larger `test_no_run_seconds` is ignored with a printed warning and
   10 s is used, so the number can only be lowered; a workspace without one gets 10 s.

`[[build_time]]` keys: `workspace` (relative to the repo), `leaf` and `hub` (relative to the workspace)
the optional `test_no_run_seconds` and the optional `contention_ceiling` (load per thread; north sets 0.5).

### Why the cap is absolute

The first rule set each threshold at the measured median times 2, at least 30 s. That made the gate
follow the code instead of the loop: north measured 43.8 s for a leaf-edit test build and got an 88 s
threshold, so the daily run called 43.8 s `within` while north's land checks and agent builds starved
gex45 of CPU. A threshold derived from the measurement can only ratify the size a crate has already
grown to. 10 s is a number an agent's edit-test loop can live with on a shared host, it holds for every
workspace whatever its size, and the code owns it, so raising it takes a reviewed change to north rather
than one line in a repo's `structure.toml`.

The leaf-edit `cargo test --no-run` is the gated number because it is the loop an agent runs most: change
one module, rebuild the tests. The hub edit is recorded beside it because it shows what a split would
buy: when hub and leaf times are close, everything sits in one crate and a split along module layers
moves most edits out of the rebuilt set.

## First measurement, 2026-11-04 6:40-6:54 AM (+07)

Host gex45 (Intel i5-13500, 20 threads, 62 GiB), each run under `north capacity host run --class heavy`
(6 CPUs, `CARGO_BUILD_JOBS=6`), the scope an offloaded agent build gets there. Measured in detached
worktrees at origin/main, never in the main checkouts. Three samples per edit, medians in seconds. The
cold rows were checked as truly cold: `cargo build -v` runs plain `rustc`, no sccache.

| Workspace | SHA | rustc | Lines | Cold build | Leaf build | Leaf test --no-run | Hub build | Hub test --no-run | Cap | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| north (4 crates) | a717bf640d | 1.96.1 | 154,042 | 36.2 | 25.4 | 43.8 | 22.8 | 42.2 | 10 | over (438%) |
| wisp relay (4 crates) | f48f0f876b | 1.97.0 | 13,846 | 1.5 | 0.9 | 1.3 | 1.3 | 2.2 | 10 | within (13%) |
| wisp window (1 crate) | f48f0f876b | 1.97.0 | 20,090 | 115.9 | 3.1 | 5.4 | 3.2 | 5.4 | 10 | within (54%) |
| makgora controller | 6fe06cfe1f | 1.97.0 | 5,238 | 99.8 | 0.9 | 1.1 | 0.9 | 1.1 | 10 | within (11%) |
| makgora tools/makgora-tools | 6fe06cfe1f | 1.96.1 | 1,936 | 1.5 | 0.5 | 0.5 | 0.5 | 0.5 | 10 | within (5%) |
| nixos-config tools/firn | 298bb3bdd3 | 1.96.1 | 564 | 1.7 | 0.3 | 0.3 | 0.3 | 0.3 | 10 | within (3%) |

The verdicts are against the 10 s cap. When this table was first written, under the times-2 rule, north
read `within (50%)` of an 88 s threshold.

Leaf and hub files (relative to the workspace): north `crates/north-guard/src/code_health/source.rs` and
`crates/north-base/src/foundation/mod.rs`; relay `relay-agent/src/wait.rs` and `relay-proto/src/wire.rs`
(relay-proto is used by the other three crates); window `wisp-window/src/mixer.rs` and
`wisp-window/src/settings/mod.rs` (49 `crate::settings` uses, the most); controller `src/profile.rs` and
`src/lib.rs`; makgora-tools `src/wait_state.rs` and `src/stop.rs` (7 uses, the most); firn
`src/host_idle.rs` and `src/main.rs`. Relay and the controller ran in wisp window's dev shell (rustc
1.97.0 and the cmake that `sdl3-sys` needs, which neither repo's own shell has); makgora-tools and firn
ran in north's shell, because their repos' shells have no cargo.

Critical paths of the cold builds (`cargo build --timings`):

- north 36.1 s: libsqlite3-sys build script (run) 6.2 > north-base 1.4 > north-core 20.8 > north bin 1.3
- wisp relay 1.5 s: relay-bench bin 0.4 (no unit over 1 s)
- wisp window 115.9 s: sdl3-sys build script 1.0 > sdl3-sys build script (run) 84.3 > wisp-window 3.1
- makgora controller 99.7 s: cc 1.1 > sdl3-sys build script (run) 94.8 > makgora-controller bin 0.8
- makgora-tools 1.5 s, firn 1.7 s: one bin each, no unit over 1 s

The raw rows are in gex45's `~/.cache/rbt/build-times.tsv`; rerun with
`north code-health build-time --repo DIR --workspace REL --leaf REL --hub REL` for a repo whose
`structure.toml` has no `[[build_time]]` yet.

## Verdicts and the split each workspace takes

North is over the 10 s cap, at 43.8 s; the other five are within it. North's `structure.toml` sets
`test_no_run_seconds = 10`; the other five get the cap without an entry, and their owners may add a
`[[build_time]]` with a lower number.

- north: the only workspace where the loop costs real time, 44 s per leaf test build. The leaf edit in
  north-core costs as much as the hub edit in north-base (25.4 s against 22.8 s), because north-core is
  20.8 s of the 36.1 s critical path and every edit there rebuilds it whole. The split is due now
  (north#259, section "Split north-core"): split north-core along its `structure.toml` layers into
  siblings that depend on a slimmer north-core rather than on each other: `ops` (41.8k lines) and `tui`
  (3.0k) as their own crates beside `guard` (17.3k), each reached only by the root package, so an edit in
  one rebuilds that crate and the root and the three compile in parallel. `ops` must first stop being
  pointed at by `capacity`, `threads`, `health` and `guard` (see `crate-split.md`); without that the split
  forms a chain and buys little.
- wisp window: 20.1k lines in one crate, yet a leaf test build takes 5.4 s; the cold build is 84 s of SDL
  compiled from C by `sdl3-sys`, which a crate split cannot touch. When it nears 7.5 s, 75% of the cap,
  split it along its module layers into a `wisp-window-model` crate (settings, model, texture) under
  parallel `render`/`draw` and `audio`/`mixer` crates, with the `wisp-window` bin on top.
- makgora controller: the same `sdl3-sys` cost (95 s cold, under 1.1 s warm); no split.
- wisp relay, makgora-tools, firn: under 2.2 s on every edit; no split.

## Daily measurement and the split box

gex45 runs `build-time.timer` daily at 4:00 AM (+07) (nixos-config `modules/build-time`, enabled in
south's gex45 host and registered as south `agents/routines/build-time.md`, placement `gex45`):
`north code-health build-time --repo ~/code/north/main --rev origin/main --file` under
`north capacity host run --class heavy`. `--rev` fetches origin and measures a fresh detached checkout
of origin/main that it removes afterwards, so the main checkout is never edited and every run is cold.
`--file` opens no issue of its own: it keeps one box per workspace in the "## Split north-core" section
of north#259 (creating the heading if absent), keyed by the workspace name in the box text. On `near` or
`over` it adds `- [ ] Leaf-edit cargo test --no-run in WORKSPACE is N s, over the 10 s cap: split the
crate holding LEAF (measured DATE on HOST at SHA)`, or rewrites that box's numbers when it is already
there; when a later run measures `within`, it ticks the box with the number. Filing starts at near
rather than over because a split takes days to land while the loop keeps slowing. `--file` refuses a
repo whose origin is not tompassarelli/north.

## The north_core unit-test harness, 2026-11-05

One timing on gex45 at origin/main cc77daa347, heavy class, `RUSTC_WRAPPER` unset, with gex45 at a load
of 20 on 20 threads: after a one-line edit in `code_health/source.rs`,
`cargo test --no-run -p north-core --lib --timings` took 100 s, all of it the single unit
`north_core "lib" (test)` (100.1 s, nothing else rebuilt). It is the whole critical path of every
north-core edit's test build, and the 43.8 s leaf median above is the same unit on a quiet host, so
contention roughly doubles it.

This entry first kept north-core as one crate, because the quiet-host median sat at 50% of the 88 s
times-2 threshold. Against the 10 s cap it is over, so the split is due: moving `ops` (41.8k lines) out
of north-core, after `capacity`, `threads`, `health` and `guard` stop depending on `ops`, is boxed in
north#259's "Split north-core" section, and the daily run keeps its measured box there.

## Toolchain levers, 2026-11-05

Tom asked on 2026-11-05 whether Rust builds can be made fast, so two toolchain levers
were measured against the stable baseline: the Cranelift codegen backend and the parallel front end
(`-Zthreads=8`). Host gex45, heavy class (6 CPUs, `CARGO_BUILD_JOBS=6`), one worktree at
origin/main fad7445bd7, `RUSTC_WRAPPER` unset, one `CARGO_TARGET_DIR` per configuration, each warmed by a
full `cargo build` and `cargo test --no-run` first. Each sample appends `const _: u64 = K;` to
`crates/north-guard/src/code_health/source.rs` and times `cargo build`, then `cargo test --no-run`, with
bash `time` (CPU is user + sys). Pass 1 (7:18-7:30 AM (+07)) ran stable, Cranelift and `-Zthreads=8` in
a fixed order as load fell from 18.7 to 10.3; pass 2 (7:39-8:00 AM) rotated stable, `-Zthreads=8`,
both levers and a plain-nightly control so that each took every slot once, at loads of 7.9 to 19.4. No
sample started or ended over the 20 threads. Medians over every sample, in seconds:

| Configuration | Samples | Leaf build | Build CPU | Leaf test --no-run | Test CPU | Test ratio | Build ratio |
|---|---|---|---|---|---|---|---|
| stable 1.96.1 (LLVM) | 7 | 37.3 | 63.9 | 60.9 | 182.6 | 1.00 | 1.00 |
| nightly-2026-04-10 (LLVM) | 4 | 42.6 | 73.7 | 56.7 | 171.5 | 1.07 | 0.88 |
| nightly, Cranelift | 3 | 50.3 | 60.9 | 69.3 | 149.2 | 0.88 | 0.74 |
| nightly, `-Zthreads=8` | 7 | 22.9 | 82.5 | 41.9 | 189.7 | 1.45 | 1.63 |
| nightly, Cranelift and `-Zthreads=8` | 4 | 22.7 | 54.7 | 37.5 | 142.1 | 1.62 | 1.64 |

The nightly is the last 1.96.0 nightly in north's pinned rust-overlay (rustc f5eca4fcf, 2026-04-09), the
commit 1.96 beta branched from, so lints and language match stable 1.96.1 and the plain-nightly row is
within noise of stable. The parallel front end is the lever: north-core's leaf rebuild is front-end
bound, so `-Zthreads=8` cuts wall time by about a third while CPU stays flat. Cranelift alone is slower
on wall time with 18% less CPU: it cuts codegen work but not the serial front end in front of it.
Together the two give the best wall time and the least CPU, 22% under stable, which matters on a host
whose builds queue for CPU. Load swung from 8 to 19 during the samples and single samples vary by up to
1.8x, so the ratios carry roughly 15% of noise; the ranking held in both passes.

Adopted the same day: `rust-toolchain.toml` pins `nightly-2026-04-10` with the
`rustc-codegen-cranelift-preview` component, and `.cargo/config.toml` sets `codegen-backend = "cranelift"`
and `rustflags = ["-Zthreads=8"]` for `[profile.dev]` only, which the test profile inherits. Release
builds, and so the runtime that `north runtime update` builds with `--release`, keep LLVM and no
`-Zthreads`. Checked before landing at the same SHA: clippy with `-D warnings` passes;
`cargo test --workspace --no-fail-fast` gives 2,987 passed and 12 failed against stable's 2,988 and 11. The
11 fail on stable too (repository-content and host-state tests that fail on that main). The twelfth,
`capacity_exclusive_native`, races a one-second exclusive lease against a bun helper start under load;
it passed 10 of 10 runs alone under each toolchain. `cargo fmt --check` reports the same 14,069 diffs
with stable and nightly rustfmt, and no check runs it. `RUSTFLAGS`, which the dev shell exports for a
foreign `CARGO_TARGET_DIR`, does not displace profile `rustflags`, so the levers hold in every lane.

### Incremental on the adopted toolchain

Measured on gex45 at origin/main 8a8f2296b with Cranelift and `-Zthreads=8`, 10:58-11:41 AM (+07) on
2026-11-05, in one worktree target under `north capacity host run --class heavy` (6 CPUs), `RUSTC_WRAPPER`
as the dev shell sets it (sccache). Each sample appends `const _: u64 = K;` to
`crates/north-core/src/agent_catalog.rs` after an unmeasured warm build in the same mode, then times a
workspace `cargo test --locked --offline --no-run` with bash `time`; the two modes alternate order each
round. "Off" is `CARGO_BUILD_INCREMENTAL=false`, "on" is the repository's `[build] incremental = true`.
Host load was 6.5 to 21 on 20 threads. Seconds; CPU is user plus system:

| Mode | Wall samples | Wall median | CPU samples | CPU median |
|---|---|---|---|---|
| off (`CARGO_BUILD_INCREMENTAL=false`) | 18.55, 18.58, 16.83 | 18.6 | 115.2, 107.3, 108.8 | 108.8 |
| on (`[build] incremental = true`) | 18.52, 17.07, 9.05 | 17.1 | 35.9, 34.7, 32.5 | 34.7 |

Incremental cuts the leaf edit's CPU 3.1x but its wall time only 1.09x, where box 8b measured 4.0x on
stable LLVM. The parallel front end and Cranelift already brought the off build from 87.4 s to 18.6 s on
6 CPUs, so what remains is serial. A `--timings` run of the on build (15.2 s wall, 32.9 CPU-s) shows the
chain: north-core rebuilds in about 5 s, then north-ops, north-runtime and north-tui each re-check
against its new metadata (to about 12 s), then the north binaries and test harnesses link (to 13 s).
Incremental reuses each crate's codegen, but every crate downstream of north-core still runs its front
end in turn. The wall-time lever left is that chain, which the north-core split shortens; the CPU cut
matters now on a host whose builds queue for CPU. Cranelift and incremental combine without error.

The repository's `.cargo/config.toml` sets `[build] incremental = true`, which outranks
`$CARGO_HOME/config.toml`; gex45's user config with `incremental = false` was moved to
`~/.cargo/config.toml.2026-11-05.bak`. Disk: the worktree's `target/debug/incremental` held 4.5 GiB after
the samples (north-core 0.5 GiB per metadata hash). Land checks are unchanged: `depbox run` mounts a
tmpfs over the home directory, so they never read the user config and already built incrementally
(rustc ran with `-C incremental=~/code/north/sandbox/target/debug/incremental`; the land-offload main
slot held 39 GiB of `incremental/`). The new cost falls on interactive worktree targets: 32 of them held
203 GiB; 23 of the 31 listed before this run already carried `incremental/` state that the off setting left unused, and the other 8
grow by about 4.5 GiB each when next built, at most 36 GiB. The root disk had 122 GiB (13%) free before
the change, so no lander override was added (it would forfeit the CPU cut where builds queue most), and
lane-gc found one retirable worktree. The disk budget in south's `orchestration.toml` covers the shared
cargo target, not worktree targets, so it does not move.
Lane-gc bounds worktree targets instead: since 2026-11-05 it deletes the git-ignored `target/` of every
worktree it keeps whose branch is landed or which has been idle for `--idle-hours`, unless a process holds
a file in it open (north#589).

## Codegen cost of north-core, 2026-11-05

Measured on gex45 at origin/main fad7445bd, rustc 1.96.1, in a detached scratch worktree under
`north capacity host run --class heavy` (6 CPUs) at a host load of 19 to 20 on 20 threads. LLVM lines come
from `cargo llvm-lines -p north-core --lib --profile test` (the lib test harness, the 77 s unit of
`build-cost.md`) and rustc queries from `-Zself-profile` plus measureme `summarize` on the stable 1.96.1
compiler with `RUSTC_BOOTSTRAP=1`, so no nightly was needed and no version could mismatch.

### What gex45 builds: incremental is off

gex45's `~/.cargo/config.toml` holds `[build] incremental = false` (written 2026-10-27, owned by no
repository). Every cargo build there inherits it, so each north-core edit rebuilds the crate from scratch
in 16 codegen units with no reuse: the leaf profile shows 17 `LLVM_module_codegen` items and 0 cache hits.
`north code-health build-time` does not override it, so the 2026-11-04 rows and the 100 s harness timing
above on gex45 measured non-incremental builds.

### Where the time goes

Self-profile of the lib test harness after a leaf edit (non-incremental, 214.5 s of self time summed
over threads, 111 s wall):

| Query (rustc component) | Self time | Share |
|---|---|---|
| `LLVM_module_codegen_emit_obj` (rustc_codegen_llvm, object emission) | 66.2 s | 30.9% |
| `LLVM_passes` (rustc_codegen_llvm, the opt-level 0 pass pipeline) | 30.4 s | 14.2% |
| `typeck_root` (rustc_hir_typeck) | 26.6 s | 12.4% |
| `codegen_module` (rustc_codegen_ssa, MIR to LLVM IR) | 23.7 s | 11.0% |
| `mir_borrowck` (rustc_borrowck) | 9.4 s | 4.4% |
| `finish_ongoing_codegen` | 4.0 s | 1.8% |
| `items_of_instance` (rustc_monomorphize, 184,705 instances) | 3.1 s | 1.4% |
| `symbol_name` (343,915 symbols) | 2.8 s | 1.3% |
| `expand_crate` (macro and derive expansion) | 2.4 s | 1.1% |

LLVM and codegen together are 59% of the work. `-Ztime-passes` on a from-scratch build of the same unit
(88.9 s wall) gives the serial front end: `type_check_crate` 16.8 s, `MIR_borrow_checking` 21.9 s,
`monomorphization_collector_graph_walk` 15.9 s, `codegen_to_LLVM_IR` 20.1 s, `LLVM_passes` 21.4 s;
`expand_crate`, which holds every serde derive, takes 2.0 s.

### The 25 costliest generic functions

The harness emits 5,770,112 LLVM lines in 176,394 function copies (the lib alone: 4,932,142 in 152,801).
The profile is flat: the costliest single function, `threads::calibration::summary::Run::read`, is 3,890
lines (0.07%). Copies were grouped by generic function with their type arguments erased. "Instantiated
from" names the north-core module whose types appear most in the copies' arguments; "std types" means
most copies are over std types such as `String`.

| # | Generic function (crate::module) | LLVM lines | Copies | Instantiated from | Cheapest cut |
|---|---|---|---|---|---|
| 1 | `alloc::vec` `Vec::from_iter` (`SpecFromIterNested`) | 167,923 | 1,993 | threads::calibration | none per site; incremental reuses it |
| 2 | `core::ptr` `drop_in_place` | 149,477 | 15,446 | std types | none: one copy per type with drop glue |
| 3 | `alloc::vec` `Vec::extend_desugared` | 94,426 | 1,166 | threads::calibration | none per site |
| 4 | `core::slice::iter` `Iter::fold` | 87,747 | 1,038 | threads::calibration | none per site |
| 5 | `core::slice::sort::stable::quicksort` `stable_partition` | 78,960 | 642 | std types | `dyn` comparator (below) |
| 6 | `alloc::vec` `Vec::extend_trusted` | 58,044 | 963 | std types | none per site |
| 7 | `core::slice::sort::shared::smallsort` `small_sort_general_with_scratch` | 52,746 | 323 | std types | `dyn` comparator |
| 8 | `alloc::collections::btree::node` `Handle::insert` | 49,399 | 302 | std types | none cheap: one per key and value type |
| 9 | `core::slice::sort::stable::drift` `sort` | 46,545 | 321 | std types | `dyn` comparator |
| 10 | `core::slice::sort::stable::quicksort` `quicksort` | 46,378 | 321 | std types | `dyn` comparator |
| 11 | `alloc::collections::btree::node` `NodeRef::ascend` | 43,146 | 493 | std types | none cheap |
| 12 | `core::iter::adapters::flatten` `FlattenCompat::size_hint` | 41,494 | 122 | std types | a loop for the costliest `flat_map` chains |
| 13 | `alloc::collections::btree::node` `NodeRef::search_tree` | 40,826 | 298 | std types | none cheap |
| 14 | `core::slice::sort::shared::smallsort` `bidirectional_merge` | 37,791 | 323 | std types | `dyn` comparator |
| 15 | `alloc::collections::btree::node` `Handle::next_leaf_edge` | 37,324 | 301 | std types | none cheap |
| 16 | `core::iter::adapters::map` `map_fold` closure | 36,099 | 1,319 | threads::calibration | none per site |
| 17 | `alloc::collections::btree::node` `Handle::insert_recursing` | 35,843 | 151 | std types | none cheap |
| 18 | `alloc::vec::into_iter` `IntoIter::try_fold` | 30,635 | 304 | fleet::store | none per site |
| 19 | `serde_json::de` `Deserializer::deserialize_struct` | 28,139 | 65 | fleet::store | read each type from one input form |
| 20 | `core::slice::sort::shared` `find_existing_run` | 27,300 | 325 | std types | `dyn` comparator |
| 21 | `alloc::collections::btree::node` `Handle::split_leaf_data` | 27,194 | 302 | std types | none cheap |
| 22 | `alloc::collections::btree::navigate` `LazyLeafRange::init_front` | 27,075 | 285 | std types | none cheap |
| 23 | `core::option` `Option::filter` | 27,071 | 595 | threads::calibration | none per site |
| 24 | `core::slice::iter` `Iter::try_fold` | 26,955 | 423 | std types | none per site |
| 25 | `core::result` `Result::branch` (`?`) | 25,141 | 1,242 | std types | none per site |

By family, over all grouped copies of the harness: Vec, iterator, Option and Result adapters 25.2%
(1.45M lines), north-core's own non-test functions 18.5%, `BTreeMap`/`BTreeSet` 14.2% (821k lines, about
150 map types), test functions 12.3%, slice sorting 8.7% (500k lines), other std 7.2%, serde 6.9%
(serde_json plus the derived visitors; north-core's own derive code is 257k lines, 4.5%), other
dependencies 4.5%, drop glue 2.6%.

Derive expansions are not the cost. 187 `Deserialize` and 135 `Serialize` derives expand in 2.0 s, and
their code is 4.5% of the lines. The costliest derived types are `router::ops::Request` (15,202 lines in
79 copies), `pace::Request` (10,106 in 79), `fleet::store::row::FleetRow` (8,585 in 60),
`fleet::model::SessionRecord` (8,439 in 32), `fleet::gates::GateRequest` (6,864 in 63) and
`capacity::admission::Facts` (6,457 in 22). Their copy counts come from each type being read through
several deserializers (`StrRead`, `SliceRead`, `Value`, and serde's `Content` buffer for tagged enums).
That is the one serde cut worth naming, and it is small.

Sorting is the one generic family with a code cut. Its 321 `driftsort_main` copies cover only 103
element types, because every `sort_by`/`sort_by_key` closure is its own type (275 call sites). A helper
that takes `&mut dyn FnMut(&T, &T) -> Ordering` would leave one sort per element type: about 68% of the
500k sort lines, roughly 340k lines or 5.9% of the harness (estimated, not applied).

### Cuts applied and measured

Leaf edit as in the method above (`const _: u64 = K;` appended to `crates/north-core/src/code_health/source.rs`,
a new K each sample), then a timed workspace `cargo test --locked --offline --no-run`, `RUSTC_WRAPPER`
unset. Each variant had its own target directory and was warmed once unmeasured. The three variants were
interleaved in each of the 3 rounds, so load drift hit them all alike. Every sample rebuilt north-core and
north. Medians, seconds; CPU is user plus system:

| Variant | Wall samples | Wall median | CPU median | Saving (wall) |
|---|---|---|---|---|
| gex45 as configured (incremental off) | 87.4, 88.4, 80.5 | 87.4 | 230.3 | baseline |
| `CARGO_BUILD_INCREMENTAL=true` | 21.2, 24.3, 21.7 | 21.7 | 36.3 | 65.7 s, 4.0x (CPU 6.3x) |
| incremental plus `CARGO_PROFILE_DEV_DEBUG=0` | 18.2, 26.6, 27.0 | 26.6 | 38.1 | none beyond incremental (inside the noise) |

Incremental is the cut, and it costs code nothing: `[profile.dev]` already asks for it, and only gex45's
user config turns it off. Its price is disk: each target directory gains 3.4 GiB of `incremental/`
(3.0 GiB without debuginfo) on a root disk that was 90% full, which is probably why it was turned off.
Dropping the remaining line tables saves nothing measurable once codegen is reused. The project already
pairs incremental with sccache, which passes incremental crates through (`build_cache_gate.rs`).

These rows ran on stable 1.96.1 with LLVM, before the Toolchain levers above were adopted. The
Toolchain levers rows ran on gex45 under the same `incremental = false`, because the repo's
`.cargo/config.toml` did not set `build.incremental` then and so the user config's value stood. The repo
config now sets `[build] incremental = true`, which outranks the user config; on the adopted toolchain it
cuts CPU 3.1x and wall time 1.09x (Incremental on the adopted toolchain, above).

## Strategy as data, 2026-11-06

Tom asked whether north's fast-moving strategy belongs in a second language, the way game engines split
engine from script (2026-11-06, paraphrased), and raised Rhai the same morning. The measure came first:
of the 1,053 non-merge commits on origin/main in the 7 days to 2026-11-06 6:20 AM, 82 touched a harness
table (`harness/defaults.toml`, `model-catalog.toml`, `loops.toml`, `model-watch-sources.toml`,
`nix/cards.toml`, `merge-sets.toml`, `policy-owners.toml`). One changed nothing else (dfa2fee27, one line
of `policy-owners.toml`); 6 changed one other file; the rest moved a table together with its Rust
consumer. By a key-set diff, 32 of the 82 changed only values of existing keys, and 31 of those 32 still
carried Rust changes. Another 10 commits changed only Rust literals, and all 10 were test expectation
strings, not strategy. So a strategy change is almost always value and logic together: an embedded
script layer would move the logic out of Rust, the type checker and the test tiers without removing a
build, and the evidence does not support adding Rhai.

What a table-only change paid: before this change every harness table was embedded with `include_str!`,
so `rust_build_input` counted it, and a one-line table edit ran the full Rust after-landing set (clippy,
nextest over every dependent crate) and a runtime rebuild. The land spans and logs keep about a day, so
only c1fe1d405 (a one-literal test fix, 5:42 AM) still has its record: 1,855 s of verification after the
push, of which host wait 1,346 s, compile 50 s, tests 122 s, lint 110 s and docs checks 160 s. The older
records, dfa2fee27's among them, had aged out.

The change: the tables and prompts in `north_base::build_inputs::RUN_TIME_DATA` are read at run time by
`north_base::platform::harness::table`, once per process, from `NORTH_HOME` when set, else the pinned runtime tree beside the
binary (`runtimes/SHA/tree`), else the source tree the binary was built from. They are no
longer Rust build inputs, so `north runtime update` reuses the live binary for a data-only commit and
installs that commit's tree beside it, and the landing selects no cargo check for it. The binary and its
tables still move together; a stale binary never reads a newer schema, because a lane that changes no
Rust input must pass `north land scan run-time-data BASE..HEAD` before the push: every TOML file still
parses, keeps every key the base had with the same type, and every array-of-tables entry keeps the keys
all earlier entries shared; a file may not be deleted. Adding keys and changing values pass. A lane that
also changes Rust is judged by its Rust checks, which read the lane's own tables.

Values are judged as well as keys (north#634). Every entry in `harness/defaults.toml` declares what its `value` may be in
an `allowed` inline table beside it: `min` and `max` for a number (or every number in a list), `one_of` or `prefix` for a
string (or every string in a list), `min_len`, `max_len` and `ascending` for a list. The scan checks the head value against the
`allowed` the base declared, refuses a lane that changes an `allowed` (only the Rust that reads the key may widen what it
accepts, and that lane is judged by Rust checks), and refuses a new key with no `allowed`. The other tables are judged by the
running north's own reader: the binary registers each reader's parse-and-validate function with
`north_base::run_time_data::install` at startup (`src/main.rs`: loops, expectations, private checks, nix cards, file skills), and
the scan runs the head text through it, so a value the reader would reject is refused before the push with no cargo. A Rust lane
is held to the same bounds by `every_default_on_main_declares_a_range_it_sits_inside`.

Measured after, 2026-11-06 6:56 AM: f2ae888b5 changed one value in `harness/defaults.toml`
(`laptop_idle_reopen_sec` 1800 to 1801, restored by the next commit). Its land log ran three checks
through `bin/head-north`, each served by the live runtime and none invoking cargo, and
`north land scan run-time-data` printed that the one changed file keeps every key and type; the lander
pushed 8 s after queueing. `north land profile` gives 11 s of verification after the push (8 s post-push
follow, 4 s CI), against c1fe1d405's 1,855 s. The after-landing run selected no Rust check, and no runtime
build log exists for f2ae888b5. The runtime it installed carries a binary byte-identical to the previous
runtime's (sha256 88a602b8...), and `north capacity host status` under strace opens
`runtimes/f2ae888b5.../tree/harness/defaults.toml`, the file holding 1801.
