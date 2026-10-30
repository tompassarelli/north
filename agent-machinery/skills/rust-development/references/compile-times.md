# Rust compile times

Measure first, change one input, measure again. Every item below says where
it came from, what the Nix devShell must provide, what it is expected to buy and
when it does not help. Nothing here is a default for our repositories until
`harness/scripts/rust-build-timings.sh` shows it winning on Tom's machine.

All sources were accessed 2026-10-12. The canonical pages were blocked by this
container's egress policy, so each was read from its upstream source file on
GitHub (the second link); wording may differ slightly from the rendered page.
Claims marked **(unverified)** were not found in any source opened.

| Key | Source | Read from |
| --- | --- | --- |
| [perf] | [Rust Performance Book: Compile Times](https://nnethercote.github.io/perf-book/compile-times.html) and [Build Configuration: Minimizing Compile Times](https://nnethercote.github.io/perf-book/build-configuration.html#minimizing-compile-times) | [compile-times.md](https://github.com/nnethercote/perf-book/blob/master/src/compile-times.md), [build-configuration.md](https://github.com/nnethercote/perf-book/blob/master/src/build-configuration.md) |
| [matklad] | [matklad, Fast Rust Builds (2021)](https://matklad.github.io/2021/09/04/fast-rust-builds.html) | [2021-09-04-fast-rust-builds.dj](https://github.com/matklad/matklad.github.io/blob/master/content/posts/2021-09-04-fast-rust-builds.dj) |
| [corrode] | [corrode, Tips For Faster Rust Compile Times (updated 2026-03-14)](https://corrode.dev/blog/tips-for-faster-rust-compile-times/) | [index.md](https://github.com/corrode/website/blob/master/content/blog/tips-for-faster-rust-compile-times/index.md) |
| [bevy] | [Bevy setup: Enable Fast Compiles](https://bevy.org/learn/quick-start/getting-started/setup/) and Bevy's `config_fast_builds.toml` | [setup.md](https://github.com/bevyengine/bevy-website/blob/main/content/learn/quick-start/getting-started/setup.md), [config_fast_builds.toml](https://github.com/bevyengine/bevy/blob/main/.cargo/config_fast_builds.toml) |
| [rustc-perf] | [rustc-perf](https://github.com/rust-lang/rustc-perf) README and collector README | [README.md](https://github.com/rust-lang/rustc-perf/blob/master/README.md), [collector/README.md](https://github.com/rust-lang/rustc-perf/blob/master/collector/README.md) |
| [profiles] | [Cargo book: Profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) | [profiles.md](https://github.com/rust-lang/cargo/blob/master/doc/book/src/reference/profiles.md) |
| [cache] | [Cargo book: Build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html) | [build-cache.md](https://github.com/rust-lang/cargo/blob/master/doc/book/src/reference/build-cache.md) |
| [timings] | [Cargo book: Reporting build timings](https://doc.rust-lang.org/cargo/reference/timings.html) | [timings.md](https://github.com/rust-lang/cargo/blob/master/doc/book/src/reference/timings.md) |
| [config] | [Cargo book: Configuration](https://doc.rust-lang.org/cargo/reference/config.html) and [Unstable features](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html) | [config.md](https://github.com/rust-lang/cargo/blob/master/doc/book/src/reference/config.md), [unstable.md](https://github.com/rust-lang/cargo/blob/master/doc/book/src/reference/unstable.md) |
| [mold] | [mold README](https://github.com/rui314/mold) | [README.md](https://github.com/rui314/mold/blob/main/README.md) |
| [wild] | [wild README and USAGE](https://github.com/davidlattimore/wild) | [README.md](https://github.com/davidlattimore/wild/blob/main/README.md), [USAGE.md](https://github.com/davidlattimore/wild/blob/main/USAGE.md) |
| [sccache] | [sccache README and Rust notes](https://github.com/mozilla/sccache) | [README.md](https://github.com/mozilla/sccache/blob/main/README.md), [docs/Rust.md](https://github.com/mozilla/sccache/blob/main/docs/Rust.md) |
| [nextest] | [nextest: how it works, benchmarks](https://nexte.st/docs/design/how-it-works/) | [how-it-works.md](https://github.com/nextest-rs/nextest/blob/main/site/src/docs/design/how-it-works.md), [benchmarks/index.md](https://github.com/nextest-rs/nextest/blob/main/site/src/docs/benchmarks/index.md) |
| [lld-1.90] | [Rust blog: rust-lld on 1.90.0 stable](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable/) | [index.md](https://github.com/rust-lang/blog.rust-lang.org/blob/main/content/rust-lld-on-stable-1.90.0/index.md) |
| [parallel] | [Rust blog: parallel front-end in nightly](https://blog.rust-lang.org/2023/11/09/parallel-rustc.html) | [index.md](https://github.com/rust-lang/blog.rust-lang.org/blob/main/content/parallel-rustc/index.md) |

## Toolchains we actually have

- This host repository: `rust-toolchain.toml` pins 1.96.1 and `flake.nix`
  builds it with rust-overlay (rustup's distribution); devShell = toolchain
  plus bun.
- galileo: no `rust-toolchain.toml`; `flake.nix` takes `pkgs.rustc`/`pkgs.cargo`
  from a nixpkgs pin whose `rust` is 1.89, plus `pkgs.gcc`. Rust 1.90 is the
  first stable to link with `rust-lld` by default on x86_64-linux-gnu, and that
  `rust-lld` ships with rustup's toolchain [lld-1.90]. So galileo almost
  certainly links through gcc with GNU ld, and the host repository probably
  already uses `rust-lld` **(unverified for rust-overlay; check with
  `readelf -p .comment <binary>`)**.

## Where the config goes under Nix

- Cargo takes extra rustc flags from exactly one of `CARGO_ENCODED_RUSTFLAGS`,
  `RUSTFLAGS`, `target.<triple>.rustflags`/`target.<cfg>.rustflags`, or
  `build.rustflags`, first match wins [config]. A flake that exports
  `RUSTFLAGS` silently disables every `rustflags` in `.cargo/config.toml`.
- Prefer a committed repo `.cargo/config.toml` with
  `[target.x86_64-unknown-linux-gnu] linker = "clang"` and
  `rustflags = ["-C", "link-arg=-fuse-ld=mold"]`, and put `pkgs.clang` and
  `pkgs.mold` (or `pkgs.wild`) in the devShell. The config then needs the shell:
  outside `nix develop` the link fails, which matches our rule of running Cargo
  only through `nix develop`.
- Without `--target`, `RUSTFLAGS` also reach build scripts and proc macros
  [config] [cache]; linker flags there are harmless, `-Z` flags are not.
- Whether Nix's clang wrapper resolves `-fuse-ld=mold` through `ld.mold` on
  `PATH` **(unverified)**; mold's README gives the absolute-path form
  `-fuse-ld=/path/to/mold` for clang [mold], which a flake can template from
  `${pkgs.mold}/bin/mold`.

## Linker: mold, wild, lld

- What: link with a parallel linker instead of GNU ld. lld is the default on
  x86_64-linux-gnu since Rust 1.90; for ripgrep it cut incremental-rebuild link
  time 7x (40% end to end) and a from-scratch debug build by 20% [lld-1.90].
  mold claims 4.9x faster than lld and 1.9x faster than wild at the median in
  its own August 2026 benchmarks [mold]; perf-book says mold is often faster
  than lld and wild may be faster still but is less mature, and that a faster
  linker has no trade-off when it links your program correctly [perf]. Bevy
  says mold is up to 5x faster than lld with occasional stability issues [bevy].
- Config: mold `linker = "clang"`, `rustflags = ["-C", "link-arg=-fuse-ld=/path/to/mold"]`
  [mold]; wild `linker = "clang"`, `rustflags = ["-Clink-arg=--ld-path=wild"]`
  [wild]; back to the system linker with `-C linker-features=-lld` [lld-1.90].
- Under Nix: devShell adds `pkgs.clang` plus `pkgs.mold` or `pkgs.wild`; the
  flag goes in repo `.cargo/config.toml`, not `RUSTFLAGS` (see above).
  `pkgs.useWildLinker` in wild's README is a stdenv adapter for Nix package
  builds, not for `cargo` in a devShell [wild].
- Expected effect: largest on incremental rebuilds and on many test binaries,
  because each is a fresh link [lld-1.90] [corrode]. galileo's 1.89 toolchain
  probably still uses GNU ld, so it should gain more than a repo already on `rust-lld`.
- When it doesn't help: a build dominated by rustc front-end or codegen
  (check `--timings`); mold and wild are Linux-only [mold] [wild].

## Profiles for dev and test

The `test` profile inherits `dev` [profiles], so one `[profile.dev]` change
covers `cargo test`. `dev` defaults: `opt-level = 0`, `debug = true`,
`incremental = true`, `codegen-units = 256` [profiles].

- **`debug = "line-tables-only"` or `0`.** Dev builds can be 20-40% faster with
  debuginfo off; `line-tables-only` keeps file:line backtraces and "still gives
  most of the compile time benefits" [perf]. matklad also turns it off to shrink
  `target/` [matklad]. Under Nix: nothing in the shell; a `[profile.dev]` line in
  `Cargo.toml`. When it doesn't help: someone needs a debugger with variables;
  link-light crates with little codegen.
- **Optimize dependencies only.** `[profile.dev.package."*"] opt-level = N`
  applies to every non-workspace member, never to workspace crates [profiles];
  Bevy uses `opt-level = 3` there with `opt-level = 1` for the dev profile
  [bevy]. Generic code is instantiated in the calling crate and uses that
  crate's opt-level; at opt-level 2 or 3 a crate stops sharing monomorphized
  generics, so Cargo suggests trying 1 [profiles]. Under Nix: Cargo.toml only.
  Expected effect: faster tests that spend runtime in dependencies, at the cost
  of a slower cold build of those dependencies. When it doesn't help: our
  dependencies are few and test runtime is in our own crates (galileo).
- **`incremental`.** On by default in dev; only for workspace members and path
  dependencies; override with `CARGO_INCREMENTAL` [profiles]. matklad and
  corrode turn it off in CI: from-scratch builds pay its dependency tracking
  and extra I/O [matklad] [corrode]. sccache cannot cache incremental crates
  [sccache]. Keep it on for edit loops.
- **`codegen-units`.** 256 for incremental builds, 16 otherwise; more units
  mean more parallelism and possibly slower code [profiles]. Lowering it is a
  release-speed knob, not a compile-time one.
- **`split-debuginfo`.** Cargo's default is `unpacked` on macOS and
  platform-specific elsewhere; some values are nightly-only [profiles]. Any
  Linux speed-up from `unpacked` is **(unverified)**; measure it only if
  `debug` stays on.
- **Build overrides.** Build scripts, proc macros and their dependencies
  already build at `opt-level = 0`, `codegen-units = 256`, no debuginfo where
  possible [profiles]; raising them can build a shared crate twice [profiles].

## Reading `cargo build --timings`

- `cargo build --timings` writes `target/cargo-timings/cargo-timing.html`
  (stable; matklad's `-Z timings` is the older nightly form) [timings]
  [matklad].
- The unit graph shows each compiler invocation over time; lines show which
  units a finished unit unblocks, which traces the critical path, and this can
  vary between runs [timings]. Lavender is codegen; with pipelining a dependent
  starts once its dependency's metadata exists, so the front-end of each crate
  on a chain is what serializes [timings] [matklad]. Orange units are build
  scripts [timings].
- The concurrency graph's red "waiting" line means CPU-bound, blue "inactive"
  means dependency-bound: a long blue stretch with few active units is a crate
  graph problem, a long red stretch is a hardware or `-j` problem [timings].
- Cargo's own tips: look for slow dependencies and features to disable,
  duplicate versions, crates to split, and many crates bottlenecked on one
  [timings].
- Nightly `-Zsection-timings` adds rustc's section split to the report
  [config]; `cargo +nightly rustc -- -Z time-passes` shows the link share of one
  binary [corrode]; `cargo llvm-lines` ranks monomorphizations by LLVM IR lines
  [perf] [matklad]; `-Zmacro-stats` measures macro-generated code [perf].
- When rebuilds happen for no reason, `CARGO_LOG=cargo::core::compiler::fingerprint=info cargo build -vv`
  prints the `Dirty` cause, often an environment variable that differs
  between tools [corrode]. A devShell that sets different variables from
  rust-analyzer's environment is the usual Nix version of this.
- Under Nix: run inside `nix develop -c` so the report's toolchain is the one
  we ship; the HTML goes to `CARGO_TARGET_DIR`.
- rustc-perf benchmarks four scenarios: `Full`, `IncrFull`, `IncrUnchanged`
  and `IncrPatched` [rustc-perf]. Our script's clean build and touch-rebuild
  are `Full` and a weak `IncrPatched`: a blank line is a lower bound for a real
  edit, which rustc-perf models with a patch such as an added `println!`.

## cargo-nextest

- `cargo test` runs test binaries one after another, each running its own
  tests in parallel, so one slow test holds the rest; nextest builds with
  `cargo test --no-run`, lists every test, then runs each test as its own
  process in parallel [nextest]. Its benchmarks show 1.37x-3.38x over
  `cargo test` on nine projects, larger workspaces gaining more; doctests are
  not supported [nextest] [corrode].
- Under Nix: `pkgs.cargo-nextest` in the devShell; no Cargo.toml change.
- Expected effect: run phase only. When it doesn't help: the build of test
  binaries dominates (it reuses the same `--no-run` build); tests that rely on
  shared in-process state across tests in one binary.

## Splitting crates and modules

- **Shape the graph wide.** Crates form a DAG; a chain `A -> B -> C -> D`
  compiles serially, a diamond with a vocabulary crate, independent feature
  crates and one leaf compiles in parallel and rebuilds less. "The most
  important property of a crate is which crates it doesn't (transitively)
  depend on." Splitting into a chain buys little [matklad]. Modules divide
  reasoning, crates divide compilation (see `notes.md`).
- **Fewer final artifacts.** Each binary and each file in `tests/` or
  `examples/` is a separately linked executable; `n` binaries over `m`
  libraries cost `m * n` linking [matklad]. Combining integration tests into
  one `tests/it/main.rs` with `mod` files gave up to 50% in a cited case, at
  the cost of a `pub` surface for what tests reach [corrode].
- **Generics and `dyn`.** Generic code is compiled in the crate that
  instantiates it, per crate and even per codegen unit, so the same
  instantiation can be compiled several times [matklad]. Keep crate
  boundaries non-generic; if an API must be generic, make it a thin wrapper
  over a non-generic inner function (the `std::fs::read` pattern), prefer
  `&dyn Fn()` over `impl Fn()` for closures, and keep cold generic paths
  outlined [matklad] [perf]. Replacing hot `Option::map`/`Result::map_err` with
  `match` sometimes helps; effects are usually small [perf]. Nightly
  `-Zshare-generics=y` shares instantiations between crates [bevy].
- **Proc macros and `build.rs`.** A crate that uses a proc macro cannot start
  until the macro is fully compiled, so `syn`/`serde_derive` leave a hole in
  the CPU graph; derive features enabled anywhere reach everything
  downstream; keep serialization in leaf crates [matklad]. Gate derives
  behind features with `cfg_attr` [corrode]. Build scripts show as orange
  units [timings].
- **Feature unification.** One crate enabling `serde/derive` makes every
  other user of `serde` wait for `syn` [matklad]. In a workspace, a
  dependency used with different feature sets is rebuilt per set, which
  `cargo-hakari` collapses into one workspace-hack crate [corrode].
- Under Nix: nothing; this is code layout.
- When it doesn't help: splitting a crate that is not on the critical path, or
  adding binaries to get parallelism (more links).

## Shared caches: sccache versus a shared target directory

- **sccache** wraps rustc via `RUSTC_WRAPPER` or `build.rustc-wrapper`
  [sccache] [cache]. It cannot cache crates that invoke the linker (`bin`,
  `dylib`, `cdylib`, `proc-macro`) nor incrementally compiled crates, so in a
  dev profile it caches registry dependencies, not workspace members [sccache].
  Hits need equal absolute paths unless `SCCACHE_BASEDIRS` strips a base such
  as the worktrees root [sccache]. Proc macros that read files may be cached
  wrongly [sccache]. corrode found the local benefit "rather negligible" and
  best for many projects sharing dependency versions [corrode]. Parallel lanes
  share one server and keep lane-local `target/`, so it fits our rule.
  Under Nix: `pkgs.sccache` in the devShell and `RUSTC_WRAPPER` exported by the
  flake (this does not collide with `RUSTFLAGS`). Disk: the default local cache
  size **(unverified)**.
- **Shared `CARGO_TARGET_DIR`.** Cargo locks the whole build cache unless the
  nightly `-Zfine-grain-locking` is on [config], so lanes sharing one directory
  serialize their builds. Different `RUSTFLAGS`, features or toolchains
  rebuild rather than reuse **(unverified mechanism: fingerprint and
  metadata hash)**, and the final binaries in `target/debug/` are overwritten
  by whichever lane built last **(unverified)**. It breaks AGENTS's "keep
  target/ inside the lane". `build.build-dir` (stable since 1.91) moves
  intermediates elsewhere with templates such as `{workspace-path-hash}`
  [config] [cache]; that relocates lane-local state, it does not share it.
- Adopted 2026-10-31: a lane-local `target/` seeded by the host repository's `target-seed` command
  (reflink copy of the newest sibling checkout's `target/` built from the same
  `Cargo.lock`), not sccache; see "Measured: a new lane's first build".

## Nightly options, not defaults

- **Parallel front-end `-Zthreads=8`.** Up to 50% faster, varies widely, dev
  builds gain more than release, memory up to 35% higher, eight threads
  recommended, a few tiny crates get slower [parallel] [perf]. Bevy sets
  `threads=0` (one per core) [bevy]. Under Nix: a nightly toolchain in the
  flake (rust-overlay `rust-bin.nightly`) and the flag in repo config
  `rustflags`. Not for us while both repos pin stable.
- **Cranelift backend.** Faster codegen, slower code, dev only; enable with
  `[unstable] codegen-backend = true` and `[profile.dev] codegen-backend =
  "cranelift"`, and Bevy keeps dependencies on LLVM with
  `[profile.dev.package."*"] codegen-backend = "llvm"` [perf] [bevy]. Under Nix:
  nightly plus the `rustc-codegen-cranelift-preview` component through
  rust-overlay **(unverified that rust-overlay exposes it under that name)**.
  When it doesn't help: front-end-bound units such as lsp-types here (9.4 of
  9.9 s front-end); galileo-core's 19 s of codegen is the kind it targets.
- Others seen: `-Zshare-generics=y`, `-Zcache-proc-macros` (5-10% incremental,
  per Bevy), Cargo `-Zno-embed-metadata` (smaller `target/`) [bevy].

## Watched, not adoptable: rustd

- What: [rustd](https://github.com/DioxusLabs/rustd) (DioxusLabs, MIT/Apache-2.0, branch `rustd-spike`, read 2026-10-16 from `RUSTD.md`) keeps a workspace loaded in rust-analyzer's database and compiles changed functions from RA's MIR with Cranelift, linking them in front of rustc's rlibs. Its launch chart claims a saved edit becomes a linked debug binary in about 200 ms, 4.3x-11.6x faster than incremental `cargo build` on the regex workspace (8 cores).
- Status: a prototype. Its own document says latency is "only comparable to cargo once a crate is at 100%" of functions compiled; there is no drop glue or unwinding yet (RA's MIR "has no cleanup edges"), async is planned as "the largest single item", unit tests inside `src/` are not relinked, there is no debug info, and no cargo subcommand exists (it scrapes `cargo rustc -- --print link-args`). The toolchain must be the exact rustc that rust-analyzer's `ra-ap-rustc_abi` was cut from; a mismatch miscompiled `?`. RA can take 0.5-8 GB per workspace.
- Fit here: none today. This host repository has 89 `async fn` on tokio and relies on `Drop`, so rustd cannot build it, and our toolchain pin would have to follow rustd's.
- Adopt when all hold: async, drop glue and unwinding supported; a cargo-integrated entry point; unit tests in `src/` relinked; its scoreboard shows 0 mismatches on a crate shaped like ours. Then measure it with `rust-build-timings.sh --label rustd` like any other lever.

## galileo

Read-only notes from galileo's eleven manifests (nine under `crates/`, two
under `tools/`; the brief's "10 crates" undercounts) and one cold build plus
one test build in this container: rustup 1.97 with `rust-lld`, 4 cores,
`--timings`. Tom's machine runs nixpkgs 1.89 with GNU ld, more cores; treat
the numbers as shape, not size.

```
galileo-core (0 deps, 10.4k src lines)   galileo-nix (0 deps)
        |  \__________________________________/  |
  galileo-syntax (core, nix; 12.5k)              |
   |       |        |        |          |        |
 review  bindgen   lsp     import(+rnix/rowan, nix)
   |    (dev:runtime)  (+lsp-server, lsp-types -> serde_derive, url -> icu/*-derive)
 runtime(core, review, syntax)                 nix-roundtrip(import, nix)
   \_______________ galileo-cli (bin; depends on all 8) ______________/
manifest tool in tools/ (no deps)
```

| Container measurement | Value |
| --- | --- |
| Cold `cargo build --workspace` | 26.6-32.5 s wall |
| galileo-core | 26.1 s unit; front-end 6.8 s, codegen 19.3 s |
| galileo-syntax | 16.7 s unit; front-end 5.6 s, starts at core's metadata |
| lsp-types | 9.9 s unit, 9.4 s of it front-end (serde derives) |
| Touch `galileo-core/src/ir.rs`, rebuild | 2.6 s |
| `cargo test --workspace --no-run` after the build | 43.5-55.8 s wall, 79 test executables |
| Test units by crate (unit-seconds) | import 25 bins 104 s, cli 21 bins 38 s, core 14 bins 31 s, syntax 12 bins 26 s |

The test run (170 s) failed without `FIRN_ROOT`/`NORTH_ROOT` fixtures and is
not a baseline.

Likely critical paths:

1. Cold build: `syn -> serde_derive -> serde -> url -> lsp-types ->
   galileo-lsp -> galileo-cli` link was the tail here (lsp-types finished at
   26.3 s, galileo-lsp at 29.9 s, the binary at 32.5 s), with
   `galileo-core -> galileo-syntax -> galileo-review -> galileo-runtime` beside
   it and core's codegen still running until 28.4 s.
2. Test build: linking 79 executables, a quarter of them galileo-import's;
   the test build takes longer than the cold build itself.
3. Edit loop on core or syntax: every other crate and every test binary
   relinks.

Heavy items visible from source: no proc macros or build scripts in galileo's
own crates (their 233 `#[derive]` attributes use only built-in traits such as
`Clone`, `Debug` and `PartialEq`);
all proc macros (`serde_derive`, `serde_repr`, `displaydoc`, `yoke-derive`,
`zerofrom-derive`, `zerovec-derive`) arrive through galileo-lsp's
`lsp-types`/`url` and galileo-import's dev-dependency `serde_json`.
Cross-crate generics are already thin: `galileo_runtime::Session::run<E>`
delegates to non-generic `run_with_budget`, and core's `incremental::blocking`
takes `&dyn Fn`.

Split candidates, each a hypothesis for `--timings` on Tom's machine:

- **Integration tests per crate into one binary** (`tests/it/main.rs`),
  starting with galileo-import (25 files) and galileo-cli (17 plus four
  `[[test]]`). Confirm: `cargo test --no-run` wall and linked-executable count
  before and after.
- **galileo-runtime depends on galileo-review only for `identity`/`Identity`.**
  Moving that type below review would take review off runtime's path.
  Confirm: runtime's start time in the unit graph.
- **galileo-lsp keeps the serde/syn/url/icu chain in galileo-cli's path.**
  Building the CLI without the LSP (a feature or separate binary) would
  remove it; a separate binary adds a link. Confirm: whether lsp-types ends
  last before `galileo` links, on Tom's core count.
- **galileo-syntax serves dependents that need little of it.** galileo-import
  uses mostly `record`, `expr` and `types`; galileo-review and galileo-runtime
  mostly `lower`. A parse/AST crate under a lowering/check crate would let
  import start earlier. Confirm: syntax's front-end share and import's start
  time; grep counted `galileo_syntax::<path>` uses only, not brace imports.
- **galileo-core codegen (19 s) is the largest single unit.** Separating the
  `ir` data types from `engine`/`check`/`why` would let dependents start on a
  smaller metadata. Confirm with `-Zsection-timings` and `cargo llvm-lines -p
  galileo-core` before any split.
- **Linker first.** With GNU ld on 1.89, mold or wild should help items 2 and
  3 above. Run the script with `--label gnu-ld` and `--label mold`.

## Measuring

`harness/scripts/rust-build-timings.sh [--label NAME] REPO FILE` runs, inside
`nix develop REPO -c`, a clean build in a fresh `CARGO_TARGET_DIR`, a rebuild
after appending a blank line to `FILE` (restored on exit), `cargo test
--no-run`, then `cargo test --no-fail-fast`, and prints `label`, step and
seconds as TSV. `--dry-run` prints the plan. Compare runs on one idle machine,
change one input per label, and repeat each label at least three times before
believing a difference.

## Measured on Tom's machine (12 Oct 2026, galileo)

`rust-build-timings.sh` on galileo, one run per cell, nixpkgs 1.89 with GNU
ld. The unpinned rows ran 11:30-11:50Z at load 6-11 from other agents; the
pinned rows ran back to back on cores 16-23 while other jobs held cores 0-15.

| Config | Pinning | Clean build | Incremental (core edit) | `test --no-run` | `test` |
| --- | --- | --- | --- | --- | --- |
| baseline | none | 40.4 s | 5.6 s | 56.1 s | 255.2 s |
| mold | none | 33.0 s | 1.8 s | 46.4 s | 250.4 s |
| `debug = "line-tables-only"` | none | 28.6 s | 3.3 s | 36.5 s | not run |
| baseline | 16-23 | 41.7 s | 6.7 s | 69.5 s | not run |
| mold | 16-23 | 45.0 s | 6.3 s | 66.1 s | not run |

Adopted: `debug = "line-tables-only"` in galileo's `[profile.dev]` (clean
build 40.4 to 28.6 s, `test --no-run` 56.1 to 36.5 s). Not adopted: mold,
whose unpinned win did not repeat pinned and whose runs never confirmed mold
was the linker in use. Open for the next pass, each with three runs per label
on an idle machine: mold with `-Wl,--version` proof, `split-debuginfo`, the
line-tables-only plus mold combination and `cargo build --timings`.

## Measured: a new lane's first build (31 Oct 2026, whiterabbit)

Source: `cargo build -p wisp-window` in `window/` of fresh wisp worktrees and
`cargo build` in fresh host-repository worktrees, one run per row, by hand on the laptop
(btrfs) with other agents' builds loading it (load average 16-40 in `uptime`
during the runs), so each row is indicative only. Times are Cargo's own
`Finished` unless marked wall.

| Setup | First lane | Next lane |
| --- | --- | --- |
| wisp, no cache | 101 s | |
| wisp, sccache, `-Cmetadata=checkout:` key kept | | 98 s wall: no gain |
| wisp, sccache, key removed | 110 s wall, 176 misses | 84 s wall, 157 hits, 19 misses |
| wisp, the host repository's `target-seed` command | | 13.5 s (copy 0.08 s); Cargo recompiled the five `-sys`/input crates and `wisp-window` after the shell gained libraries |
| wisp window/ shell, `target-seed`, `-j4` | 5 m 02 s, 182 units: its seed came from an unrelated lane and reused nothing | 4.9 s, 1 unit: `wisp-window` |
| host repository, no seed | 118 s, 147 units | |
| host repository, `target-seed` | | 71 s, 1 unit: the host crate itself, the only workspace crate |

- With the per-checkout key kept, the second worktree gained nothing; with the
  key removed it hit 157 of 176. The key goes into every crate's rustc
  arguments, so it probably blocked reuse **(inferred from the two rows, not
  isolated)**.
- sccache refuses 51 compilations of this graph by `crate-type`
  (build scripts, proc-macros and binaries, per [sccache]), so even with 157 hits
  the next lane still spent 84 s **(cause not isolated)**. A seeded `target/` carries those units too.
- the host repository's `target-seed` command runs from the devShell `shellHook`: when the lane has no
  `target/` and `CARGO_TARGET_DIR` is unset, it copies (`cp -a --reflink=auto`;
  a full copy on ext4) the newest idle sibling checkout's `target/`, a sibling
  being the same container's `main` or `worktrees/*` with a byte-identical
  `Cargo.lock` and a `CACHEDIR.TAG`, then deletes the fingerprints of every
  `Cargo.lock` package without a registry or git source, in each profile and
  `--target` directory, so none reads as fresh from another checkout's sources.
  The copy is private to the lane, so parallel lanes never wait on one Cargo
  lock. An equal `Cargo.lock` does not guarantee reuse: the unrelated seed above
  was copied and rebuilt in full, most likely because that lane was built under the old per-checkout key **(not isolated)**.
- The `-Cmetadata=checkout:` RUSTFLAGS key now applies only when
  `CARGO_TARGET_DIR` points at a shared target; with a lane-local target it
  probably blocked cross-checkout reuse, sccache's included **(inferred, see above)**.
- Disk: a seed copies the sibling's whole `target/` (837 MB for a wisp
  `window/` debug build), so a lane costs that on ext4 and shares the blocks
  with the sibling on btrfs until they diverge **(sharing not measured)**;
  the reclaim and `land-clean` sweeps still trim targets, so no separate cache
  budget exists.

## Measured: sccache against target-seed across wisp lanes (31 Oct 2026, the 20-thread build host)

Source: `cargo build -p wisp-window --locked` in `window/` of wisp worktrees on
the 20-thread build host, one run per row, each inside a heavy capacity lease sized by
admission (2 CPUs for the seed row; the sccache rows' size was not recorded,
and the first row ran without a lease) while other work loaded the host;
sccache 0.17.0 as `RUSTC_WRAPPER` and as CMake's C and C++ launcher, one
server per host, its statistics zeroed before each row. Rows are indicative only.

| Setup | Wall | Cache |
| --- | --- | --- |
| no wrapper, fresh `CARGO_TARGET_DIR`, no lease | 106.9 s | |
| sccache, first lane, own `target/` | 304.4 s | 0 of 196 Rust, 11 of 659 C/C++ hits |
| sccache, second lane, own `target/` | 361.9 s | 168 of 196 Rust, 11 of 659 C/C++ hits |
| `target-seed` from `main/window/target` | 9.9 s | 1 unit rebuilt: `wisp-window` |

- A fresh `CARGO_TARGET_DIR` per run gave 0 Rust hits on a warm cache, because
  sccache probably hashes `CARGO_TARGET_DIR` with the other `CARGO_*` variables
  **(not isolated)**; lanes with their own `target/` and no `CARGO_TARGET_DIR` hit.
- The second lane missed 648 of 659 C/C++ compiles from SDL3 (built from
  source by `sdl3-sys` through CMake) and the other C-building crates **(split
  not counted)**, most likely because their objects and include paths sit
  under the lane's `OUT_DIR` **(not isolated)**; with
  them, 168 Rust hits still left the second lane slower than the first.
- Adopted: the wisp `window/` and smashcraft-client shells now run
  the host repository's `target-seed` command as its own shell does; before this the two shells never
  seeded, so every lane there compiled from scratch. sccache is not adopted.
