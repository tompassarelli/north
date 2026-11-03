# Rust build times: a measured trigger for crate splits

Tom asked on 2026-11-04 that Rust compile times stay fast as Wisp grows. Splitting a crate costs a
`pub` surface, a manifest and moved code, and pays only where the edit-to-result loop is slow. So a
split is taken when the measured loop crosses a threshold, not before. This file records the method, the
first measurement of every Rust workspace agents edit, and the split each one gets when it nears its
threshold.

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
   time, host, repo, workspace, SHA, rustc, samples, the five wall times, threshold, verdict and critical
   path.
5. The verdict compares the leaf-edit `cargo test --no-run` median with `test_no_run_seconds`: `over` when
   above it (exit 1), `near` from 75% of it, `within` below, `unset` without a threshold (exit 0, and the
   report prints the suggested value). A new threshold starts at the measured median times 2, at least
   30 s.

`[[build_time]]` keys: `workspace` (relative to the repo), `leaf` and `hub` (relative to the workspace)
and `test_no_run_seconds`.

The leaf-edit `cargo test --no-run` is the gated number because it is the loop an agent runs most: change
one module, rebuild the tests. The hub edit is recorded beside it because it shows what a split would
buy: when hub and leaf times are close, everything sits in one crate and a split along module layers
moves most edits out of the rebuilt set.

## First measurement, 2026-11-04 6:40-6:54 AM (+07)

Host gex45 (Intel i5-13500, 20 threads, 62 GiB), each run under `north capacity host run --class heavy`
(6 CPUs, `CARGO_BUILD_JOBS=6`), the scope an offloaded agent build gets there. Measured in detached
worktrees at origin/main, never in the main checkouts. Three samples per edit, medians in seconds. The
cold rows were checked as truly cold: `cargo build -v` runs plain `rustc`, no sccache.

| Workspace | SHA | rustc | Lines | Cold build | Leaf build | Leaf test --no-run | Hub build | Hub test --no-run | Threshold | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| north (4 crates) | a717bf640d | 1.96.1 | 154,042 | 36.2 | 25.4 | 43.8 | 22.8 | 42.2 | 88 | within (50%) |
| wisp relay (4 crates) | f48f0f876b | 1.97.0 | 13,846 | 1.5 | 0.9 | 1.3 | 1.3 | 2.2 | 30 suggested | within (4%) |
| wisp window (1 crate) | f48f0f876b | 1.97.0 | 20,090 | 115.9 | 3.1 | 5.4 | 3.2 | 5.4 | 30 suggested | within (18%) |
| makgora controller | 6fe06cfe1f | 1.97.0 | 5,238 | 99.8 | 0.9 | 1.1 | 0.9 | 1.1 | 30 suggested | within (4%) |
| makgora tools/makgora-tools | 6fe06cfe1f | 1.96.1 | 1,936 | 1.5 | 0.5 | 0.5 | 0.5 | 0.5 | 30 suggested | within (2%) |
| nixos-config tools/firn | 298bb3bdd3 | 1.96.1 | 564 | 1.7 | 0.3 | 0.3 | 0.3 | 0.3 | 30 suggested | within (1%) |

Leaf and hub files (relative to the workspace): north `crates/north-core/src/code_health/source.rs` and
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

No workspace is over its threshold or within 25% of it, so no split is due. The thresholds are the
measured leaf `cargo test --no-run` median times 2, at least 30 s: north 88 s (in north's
`structure.toml`); 30 s for the other five, to be added to wisp's and makgora's `structure.toml` and to
nixos-config, which has none yet, by their owners.

- north: the only workspace where the loop costs real time, 44 s per leaf test build. The leaf edit in
  north-core costs as much as the hub edit in north-base (25.4 s against 22.8 s), because north-core is
  20.8 s of the 36.1 s critical path and every edit there rebuilds it whole. When the leaf test build
  reaches 66 s, split north-core along its `structure.toml` layers into siblings that depend on a slimmer
  north-core rather than on each other: `ops` (41.8k lines) and `tui` (3.0k) as their own crates beside
  `guard` (17.3k), each reached only by the root package, so an edit in one rebuilds that crate and the
  root and the three compile in parallel. `ops` must first stop being pointed at by `capacity`, `threads`,
  `health` and `guard` (see `crate-split.md`); without that the split forms a chain and buys little.
- wisp window: 20.1k lines in one crate, yet a leaf test build takes 5.4 s; the cold build is 84 s of SDL
  compiled from C by `sdl3-sys`, which a crate split cannot touch. When it nears 30 s, split along its
  module layers into a `wisp-window-model` crate (settings, model, texture) under parallel `render`/`draw`
  and `audio`/`mixer` crates, with the `wisp-window` bin on top.
- makgora controller: the same `sdl3-sys` cost (95 s cold, under 1.1 s warm); no split.
- wisp relay, makgora-tools, firn: under 2.2 s on every edit; no split.
