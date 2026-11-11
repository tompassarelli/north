# Compile reuse: every compile input is built once across the fleet

North moves from Cargo to Buck2 with remote execution and one shared content-addressed store, so that no rustc
invocation, link or test runs twice for the same inputs on any host (north#1115, decided 2026-11-11). This file holds
the external-code review of each tool the migration adopts, the toolchain design, and the before and after
measurements.

## Toolchain (north#1115 box 1)

- Buck2 comes from the flake's nixpkgs input (`pkgs.buck2`, `unstable-2026-04-15`, upstream tag `2026-04-15`,
  daemon version `2026-04-14-7600cb80070a88b88be67aa5d20d6a93cffa0223`). nixpkgs fetches the upstream release binary
  with a fixed SHA-256 (`pkgs/by-name/bu/buck2/hashes.json`), and `flake.lock` pins that nixpkgs revision, so the
  binary is pinned by hash through the flake.
- The prelude is the copy bundled in that binary (`[external_cells] prelude = bundled`), so the prelude cannot drift
  from the binary that interprets it and needs no pin of its own.
- `toolchains//:rust` runs the flake's rust-overlay toolchain for nightly-2026-04-10 by absolute store path. The dev
  shell links `.buckconfig.flake` to `packages.x86_64-linux.buck2-toolchain`, a buckconfig of store paths (rustc,
  rustdoc, clippy-driver, cc, c++, ar, python3) that `.buckconfig` includes. A store path changes with every input of
  the tool it names, so each action's command line, and therefore its cache key, names the exact toolchain; a
  toolchain on `PATH` would let two different compilers share one key. The package's closure is the whole toolchain,
  so a host that serves remote execution realises it with one `nix build`.
- Modes are platforms, not buckconfig values, so dev and release outputs sit under different configuration hashes and
  never replace each other: a plain build is `toolchains//platforms:dev` (Cranelift, `-Copt-level=0`,
  `-Cdebuginfo=line-tables-only`), and `@mode/release` selects `toolchains//platforms:release` (LLVM,
  `-Copt-level=3`, `-Cstrip=debuginfo`), the same settings Cargo's dev and release profiles give today. The execution
  platform `toolchains//platforms:exec` carries the dev constraint, so proc macros and build scripts build once with
  Cranelift and serve both modes, as Cargo builds them unoptimised in release too.
- The linker is the `linker` attribute of `toolchains//:rust`, today `rust-lld`: rustc links through the store's gcc
  wrapper with its bundled self-contained LLD (`-Clinker-features=+lld -Clink-self-contained=+linker`), which is
  what Cargo does on this target. A built proc macro's `.comment` reads `Linker: LLD 22.1.2`.

## External-code review: Buck2 and its prelude

- License: MIT OR Apache-2.0 (`LICENSE-MIT` and `LICENSE-APACHE` at the root of facebook/buck2, where GitHub's
  detector reports only Apache-2.0; every prelude file carries the dual-licence header). Compatible with north's MIT OR Apache-2.0; nothing is copied into north, and the binary and
  prelude are used unmodified.
- Maintenance: Meta develops Buck2 in the open as its production build system; upstream tags a release on the 1st and 15th of
  each month (newest 2026-11-01, checked 2026-11-11), and nixpkgs tracks them with an update script; the pin here
  moves with the flake's nixpkgs input.
- Pin: the release binary's SHA-256 in nixpkgs, held by `flake.lock`; the prelude is inside that binary.
- Build time: nixpkgs downloads the binary from GitHub releases or substitutes it from cache.nixos.org.
- Run time: the daemon listens on a localhost TCP port and keeps its state under `~/.buck` and the checkout's
  `buck-out/`; `http_archive` targets download crates from static.crates.io and check their SHA-256 from
  Cargo.lock; the prelude's Rust rules run helper scripts with the flake's python3. The open-source build has no
  telemetry sink. Remote execution, when box 3 turns it on, reaches only the fleet's own service.
- Why it beats what exists: Cargo keys its cache by checkout path and profile and shares nothing between hosts, and
  sccache skips incremental crates and was measured slower than lane seeding (build-cost.md, 31 Oct). Buck2 keys each
  action by its inputs and speaks the Remote Execution API, which is what one store for the whole fleet needs.

## Editor and lint (north#1115 box 10)

- rust-analyzer reads a `rust-project.json` that `rust-project` writes from the Buck2 graph. `rust-project` ships in
  the same nixpkgs `buck2` package (`bin/rust-project`, built from `integrations/rust-project` at the same tag), so it
  adds no dependency: same licence, maintainers and pin as the review below. It runs only `buck2` (the prelude's
  `rust/rust-analyzer/resolve_deps.bxl`, under its own isolation dir `.rust-analyzer`) and `rustc --print=sysroot`.
- Generating it: in the dev shell, `rust-project develop --prefer-rustup-managed-toolchain //crates/...` writes
  `rust-project.json` at the root. The file is git-ignored because it holds absolute store and `buck-out` paths.
  rust-analyzer picks it up ahead of `Cargo.toml` without any editor settings. An editor can instead load only the
  targets that own each opened file through the discovery protocol, with `rust-analyzer.workspace.discoverConfig` set
  to `{"command": ["rust-project", "develop-json", "--sysroot-mode=rustc", "{arg}"], "progressLabel": "buck2",
  "filesToWatch": ["BUCK"]}`. Check-on-save is `rust-analyzer.check.overrideCommand` set to
  `["rust-project", "check", "$saved_file"]`, which builds `[clippy.json]` for the saved file's targets. At this tag
  the generated test runnable names the program `buck`, so run-test lenses fail; run tests through `buck2 test`.
- std sources: rust-analyzer reads std from `$(rustc --print=sysroot)/lib/rustlib/src/rust/library`, so
  `rust-toolchain.toml` carries `rust-src`. Without it, `rust-project` stops at "finding std source code" with
  ENOENT. Adding the component leaves `rustc -vV` unchanged, and that output is the toolchain identity Cargo
  fingerprints. Checked on cpx62b: after the change, `cargo check -p north-strategy-types` into the same target
  directory re-checked nothing.
- What runs when the file is generated: `rust-project` builds the code the editor needs ("building generated code"):
  proc-macro dylibs and build-script `OUT_DIR`s. Check-on-save builds clippy. All of these are Buck2 actions, so on
  the laptop they run through box 6's remote execution and only their outputs download. The laptop generates its
  file only once box 6 is live there. A file copied from a build host does not work, because its paths point into
  that host's `buck-out`.
- Measured on cpx62b, 2026-11-11, with north-strategy-types (the one first-party target on main): generation took 6 s
  and covered 8 crates. `rust-analyzer analysis-stats` on the file reports 237 expressions, 0 with unknown types and
  0 failed bodies.

### Clippy

- `[clippy.json]` and `[clippy.txt]` are built with infallible diagnostics (prelude `rust/build.bzl`). A build of
  either exits 0 whatever clippy reports, so it cannot gate.
- The gate is `lint/clippy.bxl`. It takes `[clippy.txt]` for every first-party `rust_library`, `rust_binary` and
  `rust_test` the patterns name, and adds one `clippy_verdict` action per target that prints the text and fails
  when it is non-empty. Each verdict is an action keyed by its diagnostics, so an unchanged crate is a cache hit on
  every host.
- At the cut-over, `buck2 bxl //lint/clippy.bxl:clippy -- --targets //...` replaces
  `cargo clippy --all-targets --workspace --locked --offline -- -D warnings`. For one crate it is
  `--targets //crates/DIR/...`. The two match as follows:
  - `--all-targets` maps to every library, binary and test target.
  - `-D warnings` maps to any non-empty diagnostics.
  - `[workspace.lints]` (`unsafe_code = "forbid"` and seven clippy lints at warn, inherited by every crate) maps to
    `toolchains//:rust`'s `warn_lints` and `-Funsafe_code`. Those flags reach first-party crates only, because
    `cargo.rust_library` caps third-party lints with `--cap-lints=allow`.
  - There is no `clippy.toml` to carry.
- Measured on cpx62b, 2026-11-11: the clean tree passes (`clippy: 1 targets clean`, exit 0). A pedantic
  `cloned_instead_of_copied`, which only the workspace list enables, fails it with exit 3 and the rendered warning.

### rustfmt

No landing check runs rustfmt. `cargo fmt --all --check` reports 21,492 diffs on main (2026-11-11), and there is no
`rustfmt.toml` (rust-build-times.md). The cut-over therefore has no fmt check to move, and none is added. Editors
format with rustfmt at each crate's edition from `rust-project.json` (2024 for every crate). The plain run equal to
`cargo fmt --all --check` is `rustfmt --edition 2024 --check` over each crate root.

## Worker sandbox

A Buck2 daemon outlives the command that starts it and keeps that command's Landlock domain. The worker sandbox
grants Execute entry by entry at a lane's root, around `target/` (crates/north-runtime/src/worker_sandbox/executable.rs),
so a `buck-out/` created after the daemon's domain was built is never executable for that daemon, and `~/.buck` is
not among the trees a worker command may remove from, so a second daemon start fails to clean its directory. Until
the sandbox treats them as build output and cache, a worker creates `buck-out/` before its first `buck2` command.

## Tests (north#1115 box 5)

### What nextest runs today

`cargo nextest list --workspace --locked --message-format json` on cpx62b at 366fe3971 (2026-11-11):

- 1702 test cases in 72 test binaries; 2 are `#[ignore]`d, so `cargo nextest run --workspace` runs 1700:
  - 66 library binaries (a crate's unit tests);
  - 4 binaries of the root package's own targets (`north` 89 tests, `verify-north` 1, `fake_claude` and
    `push_trace_git` 0), plus `north-land-sandbox`'s binary with 0 tests;
  - 8 integration-test binaries: the root package's `it` (156), `north-lander::it` (43),
    `north-land-sandbox::{enforce,record}` (6, 13), three of `north-ops` (1 each) and
    `north-providers::opencode_lane` (2).
- No `.config/nextest.toml`: the default profile, no retries, no timeouts, ignored tests skipped.
- Nextest runs each test in its own process (`BINARY --exact NAME --nocapture`), with the package directory as the
  working directory. At run time it sets `CARGO_MANIFEST_DIR`, `CARGO_PKG_*`, and, for integration tests only,
  `CARGO_BIN_EXE_<bin>`. It also sets the `[env]` of `.cargo/config.toml`, here `NORTH_DECISION_LOG`.
- What the code reads from Cargo:
  - 58 compile-time `env!("CARGO_MANIFEST_DIR")` sites in 43 files, some outside test code;
  - 102 `env!("CARGO_BIN_EXE_*")` sites (99 of them `north`), all in integration tests (`tests/it` and the sandbox
    crate's `tests/`);
  - 72 `include_str!`/`include_bytes!` sites, 17 of them reaching outside their crate through `../`;
  - harness tables read at run time through `harness_root` (`crates/north-base/src/platform/harness.rs`), which
    finds `harness/defaults.toml` above the running executable, then from `CARGO_MANIFEST_DIR`, then falls back to
    `~/code/north/main`.
- The landing's test line runs `--lib --bins --test it` across the workspace or the selected packages. The other
  nextest lines select by package and `-E` filter.

### How `buck2 test` runs a test (Buck2 at 940470de5, 2026-11-10)

- Open-source Buck2 runs tests through its built-in runner (`app/buck2_test_runner`) unless
  `[test] v2_test_executor` names an external one. Meta's external runner, Tpx, is not open source.
- The built-in runner executes each test target's command once, with a 600 s timeout by default. It reports one
  verdict per target: pass on exit 0, fail otherwise. It splits no target into its cases.
- Test execution caching (`supports_test_execution_caching`, on by default for `rust_test`) looks the test command
  up in the remote action cache. A local test run is never uploaded ("We never upload local test executions",
  `app/buck2_test/src/orchestrator.rs`). A test result is reused only when the run happened on remote execution,
  and the remote-execution service caches only a zero exit, so only a pass is ever reused.
- The test command's key is the remote-execution action digest: the command line, the environment, and the
  merkle tree of every declared input.
- The event log (`buck2 log show`) records each test target twice. `TestEnd` carries the command report, whose
  `RemoteCommand` has the `action_digest` and `cache_hit`. `TestResult` carries the verdict, the duration and
  `details`, the command's stdout and stderr as text.

### Decisions

**One Buck2 test target per nextest binary, run by a case runner.** Each target's command is
`north-test-cases --binary-id ID -- BINARY` (crate `crates/north-test-cases`, std only). The runner lists the
binary's cases with libtest's `--list --format terse` and its ignored cases with `--ignored`. It runs each
non-ignored case in its own process, `BINARY --exact NAME --nocapture`, as nextest does. When they have ended it
prints, sorted by name, one nextest-format line per case, `PASS [   0.012s] north-base platform::tests::x`
(`FAIL`, or `SIG*` for a signal), with the output of each case that did not pass. A `Summary` line follows. It exits
0 when every case passed and 100 when any failed.
- Same verdicts: per-test processes keep nextest's semantics, where a test that sets an environment variable or a
  static sees a fresh process. Running a binary's cases in one libtest process would change verdicts.
- Same count: the lines carry nextest's binary id and test name, so the per-test verdict sets of the two runs
  compare line for line.
- Rejected for now: an external `v2_test_executor` that lists each target and executes each case as its own
  action, as Tpx does. It would give per-case cache entries and spread one large binary's cases across hosts. It
  speaks Buck2's unversioned internal test protocol (`app/buck2_test_proto/test.proto`, gRPC), which would have to
  be followed at every Buck2 update. A per-case cache entry adds reuse only when a binary is unchanged and some of
  its cases failed, because the binary's digest is in every case's key. The lever, if the `it` binary (156 cases)
  becomes the long pole: shard it into K targets with `--shard I/K` in the runner, which stays inside the
  built-in runner.

**Target shape** (emitted by box 4's generator; the rule is `north_rust_test` in `build/rust_test.bzl`):

- Names follow the shared layout. A library's tests are `//crates/<dir>:<crate>-unittest`. A binary's unit tests are
  `//<pkg>:<bin>-bin-unittest`, because `north-land-sandbox` has a library and a binary of the same name. Each
  integration test file is `//<pkg>:<crate>-test-<file>`. Every nextest binary gets one, including those with 0 cases.
- `north_rust_test(name, binary_id, crate, crate_root, srcs, deps, env, run_env, resources, ...)` declares
  `rust_binary(<name>-bin, rustc_flags = ["--test"])`, the binary `cargo test` builds. It also declares the test
  target `<name>`, whose command is the case runner. It is a `rust_binary`, not a `rust_test`, because `buck2 test //...`
  would otherwise run every binary a second time without the case runner.
- `env` is the compile-time environment, and the test sees it too. `CARGO_MANIFEST_DIR` is the package's path from
  the repository root (`crates/north-base`, `.` for the root package), set by `manifest_dir_flags()` through rustc's
  `-Zunstable-options --env-set`, and refused in `env`. The prelude turns an `env` value of `CARGO_MANIFEST_DIR` into
  the absolute path of the crate's source symlink tree under `buck-out` (`process_env` in `rust/build.bzl`): the
  binary would embed one checkout's or one remote worker's path, and every `join("../..")` back to the repository
  root would land inside `buck-out`. No site uses it in `include!`; all read files at run time from the project
  root. Every first-party `rust_library` and `rust_binary` takes the same flags, since library code such as
  `harness_root` reads it too.
- `run_env` is the test's run-time-only environment. Integration tests get
  `CARGO_BIN_EXE_<bin> = $(location //<pkg>:<bin>)` there, for each binary of their package, so the binary is an
  input of the test run but not of the test's compile, and the `it` compile does not wait for the `north` link.
  The path is relative to the project root, and a relative program path breaks every test that spawns it after
  `current_dir` or symlinks it. The sites therefore read the path through `support::bin_exe!` (`tests/it/support`),
  which takes Cargo's compile-time value when there is one and otherwise makes the run-time value absolute. The
  macro keeps the literal `CARGO_BIN_EXE_north` at each site, because the landing's test selection finds the
  modules that spawn north by that string (`crates/north-land-checks/src/land/slots/it_modules.rs`).
- Environment: a test run locally inherits only `PATH`, `USER`, `LOGNAME`, `HOME`, `TMPDIR` and `XDG_RUNTIME_DIR`
  from the Buck2 daemon (`EnvironmentInheritance::test_allowlist`), and one run remotely gets only what its target
  declares. Tests spawn git, bash and nix, so the rule declares `PATH` from `[north] test_path`. The flake
  writes that value into the toolchain buckconfig (`.buckconfig.flake`): the dev shell's packages, git and the
  stdenv tools by store path, which `flake.lock` fixes, so they are part of the test's key and the same on every host
  with the same lock; then `/run/wrappers/bin` and `/run/current-system/sw/bin` last, as the dev shell has them, for
  host tools such as systemctl that no test should reach on a remote worker.
- The working directory is the project root (`run_from_project_root`), which remote execution requires. Paths
  relative to the root, including `CARGO_MANIFEST_DIR` and `$(location)`, resolve there. A test that assumed its
  package directory as the working directory shows up as a failure in the first comparison run, not as a silent
  difference.
- Declared run-time inputs: each package's `north_files(name = "files", srcs = glob(["**"]))`, and the root
  package's files outside `crates/`, `target/`, `buck-out/` and `worktrees/`. `north_files` hands on the source
  artifacts unchanged, so a test finds each file at its repository path, as `harness_root` and the fixture reads
  expect; `filegroup` would copy them under `buck-out`. A test that reads another crate's files names that crate's
  `:files` in `resources`. On remote execution an undeclared tree read fails, because the input root holds only
  declared inputs, so a missing declaration is a red run on the first try, never a stale pass.

**Verdict reuse reads Buck2's action results.** A cached test target is a reused verdict: its `TestEnd` reports
`cache_hit`, and its `TestResult` details repeat the case lines of the run that passed. The landing's reuse reader
takes, per target, the action digest and `cache_hit` from `TestEnd`, and the per-case lines from `TestResult`.
The two-phase verdict store and the recorded path sets of [verdict-reuse.md](verdict-reuse.md) stop being the key:
Buck2 keys on declared inputs, and remote execution enforces them.

**What declared inputs do not fence, and the fix.** A read outside the input root, such as `$HOME`,
`~/code/north/main` through `harness_root`'s fallback, or `/etc`, is not in the key. On a worker that runs as
`tom`, such a read succeeds and could carry a stale pass. The remote-execution workers (box 3) therefore run as a
dedicated user that cannot read `/home/tom`, so that read fails. The ptrace recorder of
[verdict-reuse.md](verdict-reuse.md) stays the shadow audit for the remaining host reads (`/etc`,
`/run/current-system`): an observed path outside the declared inputs and the Nix store is a disagreement.

### How box 5 is checked

- `buck2 test //...` exits 0 on a build host.
- The `PASS` lines in the `TestResult` details of `buck2 log show`, taken as (binary id, test) pairs, are the same set
  as the `PASS` lines of `cargo nextest run --workspace` on the same commit. Both counts are printed.

Sources: [Buck2 test command](../references.md#buck2-test), [Buck2 test execution](../references.md#buck2-test-execution),
[nextest environment variables](../references.md#nextest-env).
