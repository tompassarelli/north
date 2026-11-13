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
- The linker is the `linker` attribute of `toolchains//:rust`, today `rust-lld`: rustc links through the store's clang
  wrapper with its bundled self-contained LLD (`-Clinker-features=+lld -Clink-self-contained=+linker`), which is
  what Cargo does on this target. A built proc macro's `.comment` reads `Linker: LLD 22.1.2`.
- `toolchains//:cxx` is the flake's clang wrapper (clang 21), not the stdenv gcc: the prelude runs a build script
  with `CC` and `CXX` set to a shim that passes `--ld-path` and `--target`, which only clang accepts, so with gcc
  every build script that compiles C through the cc crate (ring, libsqlite3-sys) fails.

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
  proc-macro dylibs and build-script `OUT_DIR`s. Check-on-save builds clippy. All of these are Buck2 actions. On a
  build host they run on the workers and only their outputs download. On the laptop they run on the laptop: it is
  no client of the store (Lanes, decision 9), its buckconfig names no engine, and the execution platform then runs
  every action locally. The editor's check is the one Buck2 work the laptop keeps. A file copied from a build host
  does not work, because its paths point into that host's `buck-out`.
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
north#1116 a worker created `buck-out/` before its first `buck2` command; the sandbox now prepares a Buck2 lane's
`buck-out/` before it builds a command's domain (crates/north-runtime/src/worker_sandbox/buck.rs), and
`north lane remove`, `reap` and `gc` count `buck-out/` and `.buckconfig.flake` as disposable, as they do `target/`.

Reads fail the same way: the sandbox fixes what a domain may read when it builds the domain
(`crates/north-runtime/src/worker_sandbox/readable.rs`), so a directory created later is unreadable inside it. On gex45
on 2026-11-11, a worker's first `buck2 build` on the host created `~/.buck` inside a domain built before `~/.buck`
existed. The daemon could not read its own `buckd.pid` and shut itself down at its 60 s self-check. The client gave
up after 803 attempts over 89.8 s. The next start in that lane stopped at
`remove_dir_all(~/.buck/buckd/<lane>/v2/prev): Permission denied`.
north#1116 also prepares the lane's own daemon directory under `~/.buck/buckd/` before it builds a command's domain
(crates/north-runtime/src/worker_sandbox/buck.rs). Buck2 places `~/.buck` from HOME alone (`home_buck_dir` in Buck2's
app/buck2_common/src/invocation_roots.rs at 7600cb80070a) and has no setting to move it, and pointing HOME into the
lane for `buck2` would hand that HOME to every program the daemon and `buck2 run` start.

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
- `buck2 test` runs every test on the host that invoked it unless it is given
  `--unstable-allow-compatible-tests-on-re`, whatever the execution platform says. With the flag, a test that runs
  from the project root with project-relative paths, as every north test target does, runs on the remote-execution
  workers. The seam's test run passes the flag (`crates/north-build-system/src/buck2.rs`).
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
- A case writes its output to an unlinked temporary file, not a pipe. Two north-providers cases leave a process
  behind that holds their output (nextest marks them `LEAK` after 100 ms and counts them passed); with a pipe the
  runner waited for that process, and a 100 ms wait as nextest's would name a case by how busy the host is. A
  case's verdict is its own exit, so the runner prints `PASS` for them and nextest's `LEAK` compares as a pass.
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
- Home: a remote worker sets no `HOME`, and a host's own would let a case read what no target declares. The case
  runner makes one empty directory per run and gives it to every case as `HOME`, as the dependency box gives
  nextest's cases an empty home.
- The working directory is the project root (`run_from_project_root`), which remote execution requires. Paths
  relative to the root, including `CARGO_MANIFEST_DIR` and `$(location)`, resolve there. A test that assumed its
  package directory as the working directory shows up as a failure in the first comparison run, not as a silent
  difference.
- Declared run-time inputs: each package's `north_files(name = "files", srcs = glob(["**", "**/.*", "**/.*/**"]))`,
  and the root package's files outside `crates/`, `target/`, `buck-out/`, `worktrees/`, `.git` and
  `.buckconfig.flake`. Buck2's `**` skips a name that starts with a dot, so the two dot patterns carry `.safe-push`
  and `.cargo/config.toml`, which tests read. `north_files` hands on the source artifacts unchanged, so a test finds
  each file at its repository path, as `harness_root` and the fixture reads expect; `filegroup` would copy them under
  `buck-out`. A package whose tests read files outside it says so in its manifest,
  `[package.metadata.north.buck] test-resources = [LABEL, ...]`, and the generator writes those labels into each of
  its test targets' `resources`. `//:tree` is every package's `:files` as one target, for tests that scan the
  whole repository (the root package, north-code-health, north-land-checks and north-runtime); such a test's key
  changes with every file, as its verdict can. On remote execution an undeclared tree read fails, because the input
  root holds only declared inputs, so a missing declaration is a red run on the first try, never a stale pass.
  Inputs are read-only on a worker, so a fixture that edits a copy of one copies its content, not its mode.

**Verdict reuse reads Buck2's action results.** A cached test target is a reused verdict: its `TestEnd` reports
`cache_hit`, and its `TestResult` details repeat the case lines of the run that passed. The landing's reuse reader
takes, per target, the action digest and `cache_hit` from `TestEnd`, and the per-case lines from `TestResult`.
The two-phase verdict store and the recorded path sets of [verdict-reuse.md](verdict-reuse.md) stop being the key:
Buck2 keys on declared inputs, and remote execution enforces them.
- As built: `north land reuse observe`, which already wraps the landing's test line, reads a `buck2 test` line back
  from `buck2 log show`, run behind the same shell words as the test
  (`crates/north-build-system/src/buck2/test_record.rs`,
  `crates/north-land-checks/src/land/slots/unit_reuse/runs.rs`). It prints one `north-reuse` unit per test binary,
  carrying the action digest when the result was a cache hit, so `north land reuse report` counts Buck2's reuse with
  the lines it already harvests. Its summary line gives the cases the binaries printed and how many failed.
- Measured on gex45, 2026-11-12, four targets run twice with the flag above: the first run executed each on a worker
  (`RemoteCommand`, `cache_hit` false), the second answered all four from the action cache (`cache_hit` true, the
  same digests, the first run's durations). A test run locally carries its digest in `LocalCommand` and is never
  reused.

**What declared inputs do not fence, and the fix.** A read outside the input root, such as `$HOME`,
`~/code/north/main` through `harness_root`'s fallback, or `/etc`, is not in the key. On a worker that runs as
`tom`, such a read succeeds and could carry a stale pass. The remote-execution workers (box 3) therefore run as a
dedicated user that cannot read `/home/tom`, so that read fails. The ptrace recorder of
[verdict-reuse.md](verdict-reuse.md) stays the shadow audit for the remaining host reads (`/etc`,
`/run/current-system`): an observed path outside the declared inputs and the Nix store is a disagreement.

### How box 5 is checked

- `buck2 test //...` exits 0 on a build host.
- The `PASS` lines in the `TestResult` details of `buck2 log show`, taken as (binary id, test) pairs, are the same set
  as the `PASS` and `LEAK` lines of `cargo nextest run --workspace` on the same commit. Both counts are printed.

### Measured, 2026-11-12

Both runs outside any worker sandbox, through `north capacity offload run`. An earlier comparison ran inside a worker
sandbox, where nextest itself fails 12 cases, so its differences said nothing about Buck2.

- Count: equal. On cpx62b at one tree (origin/main aaac03c8b5 with this box's changes), nextest ran 1808 cases and
  skipped 2, and `buck2 test //...` ran 1808 cases in 252 targets.
- Nextest, as the landing runs it (`bin/land-shell --dev`, so inside the dependency box): 1808 passed, 2 of them
  leaky, in 72 s of test time.
- `buck2 test //...`, tests on the host: 1792 passed and 16 failed, in 278 s. The tests ran as the invoking user,
  outside the dependency box, with that user's home and the network:
  - 12 `north::it worker_sandbox` cases and one north-lanes case assert a confinement the dependency box supplies;
  - one north-runtime case read sweep cooldowns from the real state directory;
  - one north-safe-push case asked the host's installed north for a private-text verdict and waited 244 s for it,
    which the dependency box's masked home and closed network never allow;
  - one north-guard case failed because `third-party/BUCK` built serde_json without `float_roundtrip`, which the
    manifest has asked for since north#1276 (regenerated in this change).
- `buck2 test --unstable-allow-compatible-tests-on-re //...` on gex45, tests on the workers: 1759 passed and 51
  failed of 1810 (the tree had two more cases), in 135 s. By test binary: `north::it` 27, north-land-checks 9,
  north-base 4, north-lanes 3, north-guard 3, and one each in north-code-health, north-fleet, north-lander,
  north-runtime and the north binary. Seen causes:
  - a read of a file no target declares, such as another crate's sources, `.safe-push` or `.cargo/config.toml`, which
    the workers refuse as designed;
  - a worker where the dependency box cannot start (`bwrap: Can't mount proc on /proc: Operation not permitted`),
    so every case that runs the dependency box or a worker sandbox fails; four north-base ssh cases that each
    wait 120 s on their fixture; and `Function not implemented` when north-lanes opens a directory.
- One verdict differed only under load: north-git-locks' four worktrees fetching at once failed once on gex45 with
  `cannot lock ref ... File exists`, a sibling holding the ref's lock past git's 100 ms wait. The fetch retry now
  takes that for the race it is.
- So box 5's count holds and its verdicts do not yet. The tests belong on the workers, where an undeclared read
  fails and a pass is cached; a run on the host is neither fenced nor reused.

### The 51 by cause, 2026-11-12

At origin/main c013d6124c on gex45, tests on the workers: 1813 cases, 1762 passed and 51 failed, in 126 s. Each
cause is named by what its cases ask of the machine that runs them.

- Other crates' files, 9 cases. A check over the whole tree read sources only its own package and the root declared.
  Fixed by a declared input, `//:tree`.
- `HOME` unset, 6 cases. Fixed by the case runner's empty home; one of the 6 then starts a sandbox and joins that
  cause.
- Dot names, 4 cases reading `.safe-push` or `.cargo/config.toml`, which `**` skips. Fixed by the dot patterns of
  `:files`; one of the 4 then edited its read-only copy and joins the next cause.
- Read-only inputs, 2 cases. A fixture copied an input with `fs::copy`, which keeps the mode, and then edited the
  copy. The fixtures copy the content.
- A nested sandbox, 20 cases: 13 `north::it worker_sandbox`, 2 `capacity_agent_compile`, and one each of
  `agent_build_sandbox`, `carry_matrix`, `depbox_broker`, `land_checks` and `landing_targets`. Each starts the
  dependency box or a worker sandbox, which fails on a worker with `Can't mount proc on /proc: Operation not
  permitted`. Open: a worker that lets them.
- Asks git about the checkout, 5 cases: one in north-code-health and one in north-land-checks list the tree with
  `git ls-files`, three in north-land-checks run `git diff HEAD HEAD`. A worker's input root has no `.git`. Fixed by
  north-base's `rust_files`: a root that is not a checkout lists its `.rs` files, and the three selections take their
  change list as an argument (`selection_of_files_with`), so a commit with itself asks git for nothing.
- The ssh fixture, 4 north-base cases. The fixture starts its own sshd and logs in to it as the user running the
  test, which takes a user with a login; each case waits 120 s for a connection that never comes. Open.
- `openat2`, 3 north-lanes cases: on a worker the call answers `Function not implemented`. Open.

Two cases are counted under two causes, the second hidden behind the first. With the first four fixed, at the same
commit on cpx62b: 1813 cases, 1781 passed and 32 failed, 247 of 252 targets passing, in 126 s, of which 120 s is
the ssh cases' wait. None of the 32 is an undeclared read. They are the last four causes, and each needs either a
worker that can run it or a named class of test that runs on the host inside the dependency box.

Sources: [Buck2 test command](../references.md#buck2-test), [Buck2 test execution](../references.md#buck2-test-execution),
[nextest environment variables](../references.md#nextest-env).

## Remote execution and store

Box 3 of north#1115 (2026-11-11). Every Buck2 action on every build host is keyed by its input digest in one shared
store, so an action one host ran is fetched by the next host instead of run again, and a host with spare cores runs
another host's actions.

### Service chosen: Buildbarn

Candidates are the services Buck2 documents as tested for remote execution
([buck2-remote-execution](../references.md#buck2-remote-execution)) plus the two other open Remote Execution API
servers. Each license was read from the repository's own LICENSE file on 2026-11-11.

- Buildbarn ([buildbarn](../references.md#buildbarn)): Apache-2.0 (bb-storage, bb-remote-execution). Store, action
  cache, scheduler, worker and runner, all in Go, with no database. Chosen.
- NativeLink: FSL-1.1-Apache-2.0, a source-available license that turns into Apache-2.0 only two years after each
  release; not open source today, so ruled out.
- BuildBuddy: MIT outside `enterprise/`, and remote execution lives under `enterprise/` with its own license; ruled out.
- EngFlow: proprietary service; ruled out.
- Buildfarm: Apache-2.0, but Java and a Redis it needs for its queue and action cache, two more services to run and
  watch; Buildbarn does the same with none.
- bazel-remote: Apache-2.0, a cache only, with no execution; it cannot give the laptop or a busy host somewhere to run.

### External-code review

- License: Apache-2.0, compatible with north's MIT or Apache-2.0 and with firn. Nothing is copied into either
  repository; the firn module writes its own configuration, modelled on bb-deployments' example (Apache-2.0).
- Maintenance: bb-storage and bb-remote-execution both published releases in the week of 2026-11-02, and
  bb-deployments, which pins a tested pair, moved on 2026-11-06.
- Pin: the release binaries bb-deployments pins as a pair, bb-storage `ca3d8b4` (release `20261102T091443Z-ca3d8b4`)
  and bb-remote-execution `c9ba98c` (release `20261102T094740Z-c9ba98c`), each fetched by its SHA-256 in
  firn's `modules/buildbarn`, the same hashes the releases publish in their `sha256` asset. They are static Go
  binaries built by the projects' own release workflow; nixpkgs carries no Buildbarn package.
- What it reaches at build time: GitHub release downloads, once per pin, checked by hash.
- What it reaches at run time: only the store address its configuration names; no component calls home. Where
  the store listens and what a remote action may reach are host security posture, kept in the private host
  configuration.

### Shape

- One store: cpx62 runs the store and action cache (bb_storage on loopback), a frontend (bb_storage) that clients and
  workers reach over the mesh, and the scheduler. cpx62 is the host that already builds and serves every host's
  Nix closure ([propagation.md](propagation.md)), and it has the free disk. The store is a 64 GiB block file that
  drops its oldest blocks when full; the action cache checks that an entry's outputs are still in the store before it
  answers, so a hit never points at an evicted blob.
- Workers on gex45, cpx62 and cpx62b each fetch inputs into a local cache, hardlink them into the action's directory
  and hand the action to the runner. gex45 runs fewer actions at once because it also runs game clients and runners.
- Mesh: the store traffic travels over a private network between the build hosts that forwards nothing; its
  membership is part of the private host configuration.
- Clients: every build host gets `/etc/buckconfig.d/remote-execution`, which names the store and scheduler for every
  Buck2 project on that host, so north's own `.buckconfig` names no host address. Buck2 merges that directory below
  the repository's own files ([buckconfig](../references.md#buckconfig)).

### What the Buck2 configuration must keep

- Same digest everywhere: an action hits on a second host only if its command, environment, inputs and platform
  properties are byte-identical. The toolchain is named by absolute `/nix/store` paths from north's flake, never
  looked up on `PATH` or under a checkout, and the execution platform's `remote_execution_properties` equal the
  workers' properties (`OSFamily = linux`). Buck2 sends those properties in the Command, not the Action, which
  Buildbarn's action key extractor reads, so the scheduler routes every action by a static key equal to the workers'
  properties: before that, a loopback run on cpx62 failed with `No workers exist ... platform {}`; after it, the
  same run executed remotely and `buck2 clean` then a rebuild reported `cached: 1`.
- The toolchain must be in each worker's `/nix/store`: an action names it, it is not uploaded. Every build host
  realises north's dev shell, which holds it.
- Buck2 skips symlinks in `/etc/buckconfig.d`, and NixOS writes `environment.etc` entries as symlinks into the
  store, so the client file is written as a regular file (`mode = "0444"`); as a symlink every action ran locally.
- The remote execution addresses are read once, when the daemon starts, from the buckconfig files; a
  `--config-file` on a later command does not reach them and the client fails with `No address`.
- The prelude's `system_cxx_toolchain` hard-codes `link_binaries_locally`, so every rustc link, build scripts
  included, ran locally on each host and was never cached. `toolchains//:cxx` wraps it and turns that one field off
  (`toolchains/remote_link.bzl`). Measured on `//crates/north-strategy-types` at one tree: cpx62 ran 48 actions
  (40 cached, 8 remote, 0 local), then cpx62b answered all 48 from the cache, and
  `buck2 log what-ran --skip-cache-hits | rg -c rustc` printed nothing.

## Lanes (north#1115 box 6)

This section covers how a lane builds once the shared store serves every host: no seeded target directory, no
warmed slot, and a laptop that sends its builds whole to a build host. It was written for box 6 before box 6 was built;
what has been built since is marked in the decisions and in the list that ends the section. Every number was
measured on gex45 on 2026-11-11 at 2bd22a2744 unless another source is named. The runs were `buck2 build //...` in
fresh worktrees of origin/main, inside the dev shell and under a 6-core heavy capacity lease.
The store was on cpx62, with Buildbarn workers on gex45 (8 slots), cpx62 and cpx62b.

### What a lane costs today

- Every lane, land-offload slot and lander lane has its own Cargo target. On gex45 (`du -sb`), 7 lane targets held
  1.7 to 10.4 GiB each (56 GiB in all), and 4 land-offload slots held 16.0 to 17.8 GiB each (66 GiB).
- Earlier readings (north#1036): on 2026-11-10, gex45's lanes held about 128 GiB of targets and its six land slots
  199 GiB. On 2026-11-11, cpx62b's slots held 236 to 251 GiB. The `land_offload` disk budgets in south's
  `agents/orchestration.toml` (160 GiB on gex45, 180 GiB on cpx62 and cpx62b) exist to hold those slots.
- gex45's root is ext4, so the seed's `cp --reflink=auto` made full copies. On 2026-11-11, three seeds at once wrote
  about 100 GiB in 15 minutes (9bebd095da). Since then a lane on ext4 is not seeded and builds cold.
- A seed pays only at the same path with `incremental/`. A north-core rebuild on whiterabbit took 14.4 s with one and
  109.8 s without ([build-cost.md](build-cost.md), Warm worktree builds).

### Measured on Buck2

- Cold build, with nothing in the store for these inputs: 47.0 to 73.5 s in four runs. Each ran 2,344 to 2,416
  actions remotely and 0 locally, and analysed 1,105 targets. 394 of the actions are proc macros and build scripts
  on the execution platform.
- A second fresh lane at another path, built after the first: 9.1 s wall (6.1 s of build). Of 2,425 actions, 2,417
  came from the action cache, 8 ran remotely and 0 locally. The 8 are the failing actions named below. The store
  caches no failure, so they run again in every lane.
- No-op rebuild in a running daemon: 1.6 to 2.1 s, including the retried failures.
- Materialised with the default deferred materialiser: 572 MiB in 9,720 files, after downloading 321 to 330 MiB from
  the store, leaving a 0.59 GiB `buck-out/`. These are the requested targets' own outputs: 247 `.rmeta` files
  (133 MiB), unpacked crate sources and argument files. No `.rlib` and no other intermediate output was
  materialised. With `--materializations none` (`-M none`): 0 B materialised, 11 MiB downloaded and a 21 MiB
  `buck-out/`.
- Daemon: a new daemon accepted its client 0.4 s after start. The first daemon on the host took 8.7 s to create its
  materialiser state. Resident memory after `//...` was 488 to 909 MiB across five daemons.
- The isolation dir is part of every action key. Outputs are written under `buck-out/<isolation dir>/`, and output
  paths are part of the command. `prelude//rust/tools:rustc_cfg` had digest `abbac298…` under `--isolation-dir m1`
  and `9547b481…` under `m2`. Four cold runs in four isolation dirs (`m1`, `m2`, `m3` and `v2`) got 0 to 13 cache
  hits each, of about 2,400 actions.
- The lane's path is in no key: the second lane above sat at another path and hit 2,417 of 2,425.
- The build-host half of box 6's check already prints nothing at this commit:
  `buck2 log what-ran --skip-cache-hits --skip-remote-executions | rg -c rustc` in the second lane. The platform's
  limited hybrid ran 0 actions locally in all five runs.
- A full remote `//...` failed at this commit: `root//third-party:libsqlite3-sys-0.39` failed on the workers with
  `could not find native static library sqlite3`, while box 2's `--local-only` build of it passed, so north-lander,
  north-runtime and every target above them did not build remotely. Box 3 fixed it in the fixups
  ([Third-party crates](#third-party-crates-reindeer), native libraries).

### Decisions

1. **One daemon per lane.** Buck2 binds a daemon to one project root and isolation dir, with its state under
   `~/.buck/buckd/<project root>/<isolation dir>`. A lane is its own worktree, so it gets its own daemon whatever north
   does. One daemon per host would need every lane to share one checkout, which ends lane isolation.
   - Cost: 0.5 to 0.9 GiB resident for each lane that has built `//...`. gex45 had 16 north worktrees touched in the
     last two hours, so 8 to 15 GiB of its 62 GiB.
   - Saved: each lane's 1.7 to 10.4 GiB target and the copy that seeded it.
2. **Every checkout builds under the isolation dir of the shared store's generation, else the default `v2`.** A second
   isolation dir shares nothing with `v2` (see Measured on Buck2) and rebuilds everything once for its name: 47 to 74 s
   cold and about 2,400 more entries in the 64 GiB store. Buck2's local state must not outlive a change of the store's
   generation, which the system configuration writes on a client host to `/etc/buildbarn/store-generation` and north
   only reads. Where it holds N, every Buck2 line north forms runs under `north-gN`, so its daemon and `buck-out` are
   that generation's alone, and a tree's first line under N ends the daemon of every other `north-gM` and removes its
   output directory, printing each. A line that decides a land check (the lint gate, the listed crates' tests and a
   build a host key chooses) needs the file: missing, unreadable or malformed sends it to Cargo with the reason. A tool
   that decides nothing (`north buck compare-cases`) runs under `v2` without the file and refuses a malformed one. The
   `--out` copies are no generation's, so a build removes its out dir before it runs and every reader reads it only
   after the build exits 0. `north build-time` measures with `--no-remote-cache` in the same dir, not in a scratch dir
   (box 7a's inventory planned a scratch dir).
   - The one exception is rust-project's `.rust-analyzer` dir (box 10), which keeps an editor's builds from waiting
     on the lane's build. The path is in no key, so its proc-macro and build-script outputs (394 of 2,425 actions)
     are built once per input across the fleet under that name, not once per checkout.
3. **A lane starts with an empty `buck-out/`, and nothing is seeded.** On a host set to `build_system = "buck2"`,
   the one key the build seam and the lane tools read, `north lane seed` creates only an empty `buck-out/`, as its
   own subvolume on btrfs (90df989383). A Buck2 build reads no `target/`, so a lane on a host still set to Cargo
   builds through the store as well, beside whatever its Cargo seed holds.
   - A lane also needs `.buckconfig.flake`, the link to the toolchain file that the dev shell's hook makes on entry.
     `north capacity dev-shell run` runs a command directly when its process is already inside that shell, so a lane
     made by such a process had no link and `buck2` stopped at `buckconfig [nix] clippy_driver is unset` (gex45,
     2026-11-12). It now enters the shell again for a Buck2 checkout without the link.
   - Cost: the lane's first build, 9.1 s and 0.59 GiB, once another lane has built the same inputs.
   - Saved: a 30 to 50 GiB copy, or a cold Cargo build on ext4.
   - Box 8 deletes the Cargo seed.
4. **Intermediate outputs stay in the store; a lane holds only what it runs.** The deferred materialiser keeps every
   intermediate output in the store and writes only the requested targets' outputs into `buck-out/`. A check needs an
   exit status, not a file.
   - The seam's Buck2 side passes `-M none` to every build whose result is only its exit status: the land build
     check, the offload build and box 6's check.
   - `buck2 test` runs tests on the workers when given `--unstable-allow-compatible-tests-on-re`, as the seam's test
     run is, so no test binary is materialised; without the flag it runs them on the host (Tests, north#1115 box 5).
   - Materialised are only what a command runs or ships: a `buck2 run` target, the runtime build's `--out`, and the
     proc-macro libraries and build-script outputs that rust-project hands the editor.
   - Saved: 572 MiB and 321 MiB of download per `//...`, against 0 B and 11 MiB.
5. **Every action runs remotely, on every host.** In `toolchains//platforms:exec`, `local_enabled` becomes
   `not remote`, and `use_limited_hybrid` goes. A host whose buckconfig names a remote-execution engine then runs
   nothing locally; a host without one, such as a GitHub runner, builds locally as before.
   - Why: a local action runs outside the workers, so the Tests section's fix for undeclared reads does not cover it.
     Its result is never uploaded (the prelude leaves `allow_cache_upload` off for rustc), so the next lane builds it
     again.
   - Cost: limited hybrid ran 0 local actions in the five runs, so the measured cost is nothing. The host's own cores
     still build, through its Buildbarn worker.
   - It also makes box 6's check a guarantee rather than an outcome of load.
   - On such a host `--local-only` is refused, not run: on gex45 on 2026-11-12 `buck2 build --local-only` of one
     crate exited 3 with `The desired execution strategy (LocalOnly) is incompatible with the executor config that
     was selected`, and `--prefer-local` built it remotely. Box 10c's clippy check therefore names no execution
     flag. A target that must run on the invoking host names its own executor.
   - Incremental compilation stays off. The prelude makes an incremental rustc action `local_only` (`rust/build.bzl`,
     `if incremental_enabled: local_only = True`), so it could never be shared, and under this decision it is
     refused. A one-line edit therefore recompiles the edited crate from scratch on a worker. In the cold build,
     the longest first-party compile actions took 3.9 s (north-base check), 5.4 s (north-pace), 6.1 s
     (north-providers) and 8.3 s (north-ops diagnostics), all remote under full load. Box 9 records the
     one-line-edit rebuild before and after.
   - The remote-only rule covers build, test and bxl. `run` is outside it, because a `buck2 run` binary executes in the client.
   - `bin/land-shell` refuses `buck2 run --remote-only` (north#1291), so no lane reaches that path.
6. **The worker sandbox lets a lane's daemon live** (the three failures in [Worker sandbox](#worker-sandbox)).
   - It prepares the lane's own daemon directory under `~/.buck/buckd/` before it builds a domain (north#1116).
   - It prepares `buck-out/` before it builds a domain (north#1116).
7. **A daemon lives as long as its lane.** This Buck2 has no idle exit, but a daemon checks its `buckd.pid` every
   60 s and shuts down when it cannot read it (measured: started 18:33:54 UTC, forced shutdown at 18:34:54 UTC).
   - The hourly `north lane reclaim` deletes the daemon state under `~/.buck/buckd/` of every checkout that is gone
     (`crates/north-lanes/src/lanes/buck_daemons.rs`), so that daemon stops within 60 s. One sweep covers every way
     a lane goes (`lane remove`, `reap`, `gc`, the archive, a pin's retirement, a deleted directory), where the plan
     was a `buck2 kill` in each of them. The state of a checkout that still has its `.buckconfig` is never touched.
   - Not built: killing the daemon of a lane that is idle but still there, as `north lane gc` drops a scratch Cargo
     target after 3 h (`crates/north-lanes/src/lanes/gc/scratch.rs`).
   - A killed daemon re-analyses the graph on its next command, as a fresh lane does, in about the 9.1 s measured
     above.
8. **Land-offload slots and lander lanes are fresh checkouts.** A slot is a checkout with an empty `buck-out/`.
   - The warm step (`north capacity offload warm`) and the slot's target go.
   - A slot keeps its daemon between checks (no-op 1.6 to 2.1 s), and the daemon dies with the slot.
   - Saved: gex45's 66 GiB of slot targets today. Each slot holds about 21 MiB under decision 4, so the
     `land_offload` budgets can go in box 8.
9. **The laptop is no client of the store; a build started there runs whole on a build host.** Decided 2026-11-12
   (north#1115 box 6a).
   - What runs where: `north capacity offload run` sends the lane's commits to a build host as a git bundle, checks
     them out in a land-offload slot there and runs the command in that slot's dev shell. That host's buckconfig
     names the engine, so decision 5 holds: every action runs on the workers, and box 6's check
     (`buck2 log what-ran --skip-cache-hits --skip-remote-executions | rg -c rustc`) prints nothing on the host that
     ran the build. The laptop gets the command's output and its exit status, and no build output.
   - What the laptop keeps: the editor's check, through the generated `rust-project.json`
     ([Editor and lint](#editor-and-lint-north1115-box-10)). `rust-project` and check-on-save run Buck2 on the
     laptop under the `.rust-analyzer` isolation dir. No buckconfig there names an engine, so
     `toolchains//platforms:exec` runs each action locally, and nothing is read from or written to the store. The
     laptop compiles the proc macros and build scripts of what the editor loads once (394 of 2,425 actions for the
     whole graph), then clippy for the saved file's targets on each save. Not measured on the laptop.
   - What north refuses: its own path to `buck2` on the laptop does not become a client of the store.
     `north capacity dev-shell run`, which `bin/land-shell --dev` and so the build seam go through, exits 78 with
     one line when a command that runs `buck2` on the laptop finds a `[buck2_re_client]` address in a buckconfig
     Buck2 reads or on its own command line (`crates/north-capacity/src/capacity/store_client.rs`).
   - Cost: the shipping, and the wait for a slot when the host is busy. Measured on one run from a laptop lane to
     cpx62b on 2026-11-12 (below): 6.9 s of the 90.5 s the laptop waited were not the build.
   - Not taken: two ways of making the laptop a client of the store. Either would have saved the shipping and let
     the editor download its proc macros instead of compiling them. What they are and why neither was taken is
     recorded with the host configuration, outside this repository.

### Measured with every action remote, 2026-11-12

On gex45 through `north capacity offload run`, outside any worker sandbox, at 5a6a747a86 (origin/main dd4316e8b7
with decision 5 applied). Each lane was a new worktree with no `target/` and an empty `buck-out/`, and the clock
ran from `git worktree add` to the end of `north capacity dev-shell run -- buck2 build //...`, so it holds the
dev shell's entry, the daemon's start, the analysis and the build.

- First lane, with main's newest first-party crates not yet in the store: 74.0 s.
  `Commands: 2801 (cached: 2270, remote: 531, local: 0)`, 247 of the 531 being rustc, `BUILD SUCCEEDED`.
  `buck2 log what-ran --skip-cache-hits --skip-remote-executions | rg -c rustc` printed nothing (rg exit 1).
- That build downloaded 1.1 GiB and left a 4.4 GiB `buck-out/`: `//...` now links every binary and test binary,
  which the 0.59 GiB measured on 2026-11-11 did not hold, because north-lander and everything above it failed to
  build then. A lane that asks for `//...` without `-M none` therefore holds 4.4 GiB, which is why decision 4's
  `-M none` belongs on every check build.
- Second lane, at another path, with everything in the store: 71.1 s.
  `Commands: 2801 (cached: 2801, remote: 0, local: 0)`, 1.1 GiB downloaded, a 4.4 GiB `buck-out/`. Nothing was
  built, so those 71 s are the download and the writing of outputs nothing reads.
- Third lane, the same with `buck2 build -M none //...`: 4.3 s, 2801 of 2801 cached, 5.0 MiB downloaded, a 54 MiB
  `buck-out/`. Both lanes printed nothing for the local rustc check.
- So a fresh lane's check build costs 4.3 s once any lane has built its inputs, against 71 s with the default
  materialiser, and the 9.1 s of 2026-11-11 no longer describes a full `//...`.

From a laptop lane, the same day: `north capacity offload run --host cpx62b` with `buck2 build //...`, at this
section's change on origin/main 0fcaeae345, timed on the laptop around the whole call and on the build host around
the build.

- The laptop waited 90.5 s and the call exited 0. The build took 83.5 s of it on cpx62b:
  `Commands: 2801 (cached: 2273, remote: 528, local: 0)`, 1.1 GiB downloaded, `BUILD SUCCEEDED`.
  `buck2 log what-ran --skip-cache-hits --skip-remote-executions | rg -c rustc` printed nothing there (rg exit 1).
- Shipping is the other 6.9 s. By the two machines' clocks, 5.7 s passed before the build started: the bundle
  sent, the commit checked out in a free slot, the lease (0.02 s queued) and the dev shell entered. 1.2 s passed
  after it: the check above, a `buck2 clean` and the answer coming back.
- The 83.5 s are the default materialiser's download again, on the build host, where nothing reads it. A laptop
  check passes `-M none` as a lane's does.
- The wait for a slot is not in those 6.9 s, and it was the larger cost that hour: the same call to gex45 was
  still in line after 180 s, behind 13 to 16 queued jobs with all 6 slots held, and was stopped there.

### What box 6 builds, in order

1. `toolchains//platforms:exec`: `local_enabled = not remote`, with no hybrid (decision 5). Built.
2. The worker sandbox's `~/.buck` and `buck-out/` grants (decision 6). Built (north#1116).
3. The daemon state of a lane that is gone is reclaimed, which stops its daemon (decision 7). Built.
4. The seam's Buck2 side: no `--isolation-dir` anywhere, and `-M none` on check builds (decisions 2 and 4). Built
   with box 7b: the seam passes no `--isolation-dir`, and its check builds pass `--materializations=none`
   ([Cut-over](#cut-over-the-seams-buck2-side-north1115-box-7b)).
5. Land-offload slots without warm or target (decision 8). With box 7b, which is when a slot first builds through
   Buck2.
6. The laptop sends its builds whole to a build host and is refused as a client of the store (decision 9). Built:
   the offload already carried a laptop lane's build, and the refusal is
   `crates/north-capacity/src/capacity/store_client.rs`.

Box 6 needs nothing from box 5.

## Cut-over: the seam's Buck2 side (north#1115 box 7b)

One per-host setting in the orchestration policy chooses the side, and it is off everywhere until the canary (box 7c):

- `[hosts.NAME] build_system = "buck2"` makes every caller of the build seam (`crates/north-build-system`) on that
  host build, lint and list tests with Buck2. It is the key `north lane seed` reads too (Lanes, decision 3), so a
  host has one switch, not one for lanes and one for the seam.
- `[hosts.NAME] buck2_tests = true` adds the test run. Without it a Buck2 host still compiles and runs tests with
  Cargo and nextest, because box 5's verdicts are not yet equal. It has no effect on a host that builds with Cargo.

The land check lines of `.safe-push` name Cargo until box 8 rewrites them. On a Buck2 host the land checks read each
line through the seam (`land_line`), which issues that side's own command behind the declared line's shell:

- The lint line stays as declared (2026-11-13): issued where the checks are listed, it ran the gate with no
  `buck2_lint` and no comparison with the base, so a lane that rewrote the gate script or `.buckconfig` named its own
  gate. Only `buck2_lint` moves it, through `north land lint` below.
- With `buck2_tests`, every nextest and `cargo test` line becomes
  `./bin/land-shell --dev -- buck2 test --remote-only --unstable-allow-compatible-tests-on-re //...`; the five lines are then one
  command and run once.
- Every other line runs as declared. So does every line on a Cargo host, where nothing changes.

Each list of `.safe-push` that names a check line (`land.check`, `land.after`, `land.after-only`, `land.deadline`
and the rest) is read the same way, so a reason or deadline declared for the Cargo line holds for the line issued
in its place.

**Where a tree's actions run.** A Buck2 client compiles nothing itself: it sends each action to the remote
execution engine. A tree's build rules (`BUCK`, `.bzl`, `.bxl`, `.buckconfig`, `toolchains/`) are text a lane
writes, and they name the execution platform. Two rules keep a tree's actions on the workers whatever those rules
say (decision 5 of Lanes as a guarantee of the command, not of the tree), and neither is in the build rules:

- Every build, lint and test command the seam issues passes `--remote-only`, which makes Buck2 refuse an action
  that cannot run on the workers, whatever executor the tree's platform names.
- `bin/land-shell` runs a `buck2` command in the dependency box's `fetch` mode, which a client needs to reach the
  engine, only when the command carries `--remote-only` before any `--`. Every other command runs in `run` mode, as
  a Cargo build does.

Checked on a build host through `bin/land-shell` (2026-11-12), on one crate with an edit that no cache held:

- The tree's platform changed to a local-only executor: exit 3, `The desired execution strategy (RemoteOnly) is
  incompatible with the executor config that was selected`. Nothing ran.
- The platform changed to allow both: exit 0, `Commands: 2 (cached: 1, remote: 1, local: 0)`.
- The clippy gate: `clippy: 148 targets clean`, 1283 commands, 0 local.

Not checked: where `buck2 test` runs a test whose own rule names a local executor. The test run stays behind
`buck2_tests` until that is shown the same way.

**Lint alone (`buck2_lint`).** `[hosts.NAME] buck2_lint = true` moves only the lint line of the checks before a
landing to the clippy gate. The host still builds and tests with Cargo, and `build_system` stays unset. Four rules
hold, each asked by the commit review before any host sets it:

- **Which host decides.** The checks before a landing hand their lint line to `north land lint` on the host that
  executes it, so a check offloaded from a host without the key is linted the executing host's way; the laptop,
  which is no client of the store, still never runs Buck2. A north that lacks the verb runs the line as declared.
- **The Cargo lint stays the authority.** The run after a landing never hands its lint line over and keys that
  line's verdict apart from the one before the landing, so the landed tree is linted by `cargo clippy` itself and
  never reuses a pass the gate gave. What the key changes in trust: the gate's pass rests on results read from the
  shared store, so a forged result there could at worst pass a lint before a landing, and the Cargo lint of the
  landed tree then fails it.
- **Origin.** The gate runs only while the gate script and every file of the tree the Buck2 client reads its
  configuration from are byte for byte those of the base the check was given: `lint/`, `.buckconfig`,
  `.buckconfig.local`, `.buckconfig.d/`, and `flake.nix` with `flake.lock`, whose dev shell writes the toolchain
  buckconfig the tree includes; an untracked file among them counts as a change. That toolchain buckconfig,
  `.buckconfig.flake`, can hold any section, so it must be absent or the untracked link into the store that the
  shell hook makes: a tracked one, a plain file or a link elsewhere counts as a change too. The link is resolved
  before it is placed, so one that starts with the store's path and ends outside it, or reaches nothing, counts as
  elsewhere. Otherwise the declared Cargo line runs. So a lane that rewrites the gate script or names its own endpoint is judged by Cargo, and the endpoint, the
  cache and the store are the ones the host and main name. A flag cannot do this instead: a tree's `.buckconfig`
  outranks the host's file, and neither `--config-file` nor `--config` changes where the remote execution client
  connects (build host, 2026-11-12: with a tree naming its own endpoint, both still failed to connect to it).
- **Verdict.** The check passes or fails on Buck2's exit status alone: 0 passes, 3 (Buck2's status for an error in
  the tree) fails, and any other status is no verdict (exit 75). No output is read.

Checked on a build host (2026-11-12), on one crate through `bin/land-shell`: the gate on a clean tree exits 0
(`clippy: 1 targets clean`, 3 commands, 0 local); with a lint finding it exits 3; with an
endpoint the client cannot reach it exits 2, which the verb reports as no verdict.

**Tests of listed crates (`buck2_test_crates`).** `[hosts.NAME] buck2_test_crates = ["NAME", ...]` moves the tests of
the listed crates alone, in the checks before a landing, on the host that executes the check. The nextest line that
reads its packages from the environment is handed to `north land crate-tests -- COMMAND`: each selected crate the
host lists runs as `buck2 test --remote-only --unstable-allow-compatible-tests-on-re //crates/NAME:`, one crate at a
time, and the declared line then runs for the rest of the selection (`-p NAME` removed, or `--workspace --exclude
NAME`). Crates not listed, the root package with `north::it`, and every other check line stay on Cargo. No host
sets the key, and none does before box 11 closes: the workers run the lane's own code, its tests and also its
build scripts and procedural macros.

- **The Cargo run stays the authority.** The run after a landing never takes this path. A nextest verdict before a
  landing that Buck2 may have decided part of (some host lists a selected crate) is keyed apart, so the run after
  the landing does not reuse it and Cargo tests the landed tree itself; every other line keeps its verdict reuse.
- **Origin, stricter than for lint.** A forged lint pass is caught after the landing at no cost; a forged test pass
  would land a failing change. So besides the lint gate's origin rule, the split is issued only while every file that
  defines a target is the base's, with no untracked one: every `BUCK`, `BUCK.v2`, `TARGETS` and `PACKAGE` file and
  any build file name the tree's `.buckconfig` sets, every `.bzl`, `.bxl` and `.star` file, `toolchains/` and
  `prelude/`. A lane that touches one is tested by Cargo in full.
- **Verdict.** Buck2 decides for a crate only when it says so plainly. It passed: exit 0 and its summary line counts
  at least one passed test and no failed one, so a target with no test cannot pass a crate that then leaves the
  Cargo line. It failed: exit 3, a build the tree failed, or exit 32 with a failed test counted in the summary. On
  anything else (no summary, no test, a run lost to the workers) the crate stays on the declared line and Cargo
  tests it; nothing is run again to make it pass.
- **A crate with no case, by name only.** A run that executes nothing proves nothing about a crate that has cases, so
  no observation alone exempts a crate from the rule above. `[hosts.NAME] buck2_caseless_crates = ["NAME", ...]`
  names a crate only after `north buck compare-cases NAME` at a named commit found no case on either side with
  `buck2 test` exit 0, and the name is kept in the commit that adds it. A named crate moves like a listed one and
  passes when its targets pass with no case run. The moment one of its cases runs it is judged as any listed crate.
  A unit test it gains runs in its existing `-unittest` target; an integration test file it gains has no target until
  the generated `BUCK` names it, and `north buck targets --check`, the land check on `crates/`, refuses the lane
  until then, while a lane that changes the `BUCK` file is tested by Cargo in full.
- **Host cases (`buck2_host_cases`).** The cases no remote worker can run (`build/host-cases.list`) are skipped by
  the Buck2 targets, and a passed crate's share of them runs on the declared line narrowed to them, which compiles
  that crate's test binaries with Cargo. A host that also sets `buck2_host_cases = true` takes them off Cargo: `buck2
  --isolation-dir host-cases build --remote-only --no-remote-cache`, in its own daemon so no result the cached test run
  left in the shared one is reused, builds each class package's `host-cases` target on the workers, whose one
  output is a plan naming the case runner, the test binary and the environment its test target sets, and leaves those
  files in the checkout; the case runner then runs each listed case by name on the invoking host, in the declared
  line's shell, as the same user, from the repository root, which is where and as whom Cargo runs it. Only the test
  process runs there; nothing else of the tree does. The step fails when a binary has no plan, a listed case is not
  in its binary, a case fails, or the cases run and passed differ from the class. `north land crate-tests host-cases
  [PACKAGE...]` runs the step alone.

A check build passes `--materializations=none` (`-M none`): the lint gate, the type check and the compile before a
test run leave their outputs in the store, since nothing after them reads one (Lanes: 4.3 s against 71 s and
4.4 GiB for `//...`). The runtime build keeps `--out`, which copies the one binary a later step runs.

The host's side applies to a tree only when the tree is its kind of project (`for_tree`): a Buck2 host's land checks
for a repository with no `.buckconfig` read its Cargo lines as declared and build it with Cargo.

`bin/head-north` names no build tool. When no built runtime covers a tree it asks the live runtime to build north
from the tree, `north runtime tree-build`. A host moves this build alone to Buck2 with `buck2_tree_build = true`, as
`buck2_lint` moves the lint line (2026-11-13): the verb runs `buck2 build --remote-only //:north --out
buck-out/north/dev/north --no-remote-cache` when the check names its base (`SAFE_PUSH_LAND_BASE`) and the tree's Buck2 configuration and
target definitions are the base's, the guards the crate split uses, and Cargo's build otherwise; it prints the line it
runs, and when it declines Buck2 it says why. The build reads and writes no remote cache entry: the cache takes
writes from any client, so a result another client wrote there could otherwise stand in for the tree's own compile. It reads neither `build_system` nor `here()`, which would also move the
release build of the runtime every host promotes and the readers of the test selection. The Cargo side compiles north
as the test run does (`cargo test --no-run -p north --test it`, so a tree whose tests were compiled has it already).
The verb exits 1 when the tree itself does not build (Cargo's `could not compile`, Buck2's exit 3 for a user error);
when Buck2 fails for another reason Cargo builds the tree, and the verb exits 75 when the last side failed that way.
The script keeps no build of its own: a live runtime that is missing or does not know the verb defers the check.

A host that builds and tests with Buck2 warms no land-check slot (Lanes, decision 8): `north capacity offload warm`
marks the slot as at the head and runs nothing, since the store every slot shares holds what a warm target held.
With `buck2_tests` unset the nextest lines still compile with Cargo, so the warm stays.

What each of the four does on the Buck2 side, and what it cannot yet do that the Cargo side does:

- Build: `buck2 build [@mode/release] //:BIN --out buck-out/north/PROFILE/BIN`, under a directory no Cargo run uses. It writes no per-unit timings line: `--build-report`
  records each target's outcome and outputs, not how long each compile unit took, so the runtime build's timings
  reader has nothing to read. The fails-first flake runs (`[flake] build`) still compile with Cargo.
- Lint: the clippy gate over `//...`. It takes no changed-crate selection: it lints every first-party target, and an
  unchanged crate's verdict is a cache hit, where the Cargo line lints only the crates a lane changed.
- Test listing: `buck2 uquery --json --output-attribute binary_id 'kind("^_north_test$", //...)'`, a query that compiles
  nothing. It names each test binary by its nextest id but no case, since a case name exists only in a built binary,
  so test selection by reachability cannot narrow a binary to its changed cases and the landing runs the whole
  closure of the packages a change reaches.
- Test run: `buck2 test` on the workers, with verdict reuse read from Buck2's action results. It takes no case filter
  (the case runner runs every case of a target), and the mutant kills step still runs cargo-mutants, which drives
  Cargo alone, on either setting.

A line issued by the seam has no entry in the landing's verdict cache, whose key is found by the declared line, so
its reuse is Buck2's action cache alone.

## Third-party crates: reindeer

Every crates.io package the workspace builds is a Buck2 target in `third-party/BUCK`, generated by
[reindeer](https://github.com/facebookincubator/reindeer) from the workspace's own `Cargo.toml` files and
`Cargo.lock` (north#1115 box 2). Regenerate after any change to a manifest, `Cargo.lock`, `third-party/reindeer.toml`
or a fixup, on a build host:

```sh
north capacity dev-shell run -- nix run .#reindeer -- -c third-party/reindeer.toml buckify
```

### Choices

- Workspace mode, not a pseudo-package. `reindeer.toml` points at the root `Cargo.toml`, so the dependency
  set, versions and features come from the manifests Cargo already reads and the one `Cargo.lock`. Reindeer's
  documented layout instead keeps a separate `third-party/Cargo.toml` listing every dependency again, with its
  own lock file; that second list drifts from the crates' manifests the first time a lane adds a dependency to one
  and not the other.
- Dev-dependencies are included. Upstream reindeer resolves a workspace member's normal and build dependencies
  but never its dev-dependencies, so `proptest`, `rstest` and tokio's `test-util` feature would have no target and
  the tests of box 5 could not build. `third-party/reindeer-workspace-dev-dependencies.patch` enables a
  workspace member's dev-dependencies during feature resolution and makes them public aliases; dependencies of
  third-party packages are untouched. The flake applies it to the nixpkgs package.
- Features are unified across the workspace, dev-dependencies included, as `.cargo/config.toml` already asks
  Cargo to do (`feature-unification = "workspace"`): each crate has one target with one feature set. A release
  binary therefore links tokio with `test-util` compiled in, which `cargo build` leaves out; the extra items are
  unused code, and one feature set is what lets a test build and a release build share every dependency
  artifact.
- Not vendored. Each package is an `http_archive` of `https://static.crates.io/crates/NAME/VERSION/download`
  checked against the sha256 in `Cargo.lock`, so the repository carries no crate sources and the archive is an
  action output the shared store keeps once. Vendoring would add every crate's source to Git for no reuse the
  store does not already give.
- One platform, `linux-x86_64`. Every build host and the laptop are x86_64 NixOS. Packages only other targets
  use (Windows, WASI, Redox, wasm-bindgen) get no target, so `buck2 build //third-party/...` builds only what a
  north binary or test can link.
- Cargo's environment for every crate (`cargo_env = true`): `CARGO_PKG_*` and `CARGO_MANIFEST_DIR` as Cargo
  sets them, because serde, thiserror and portable-atomic read them with `env!` and a per-crate list breaks at
  the next crate update that reads one more.
- Build scripts run, as under Cargo. Each crate with a `build.rs` has `fixups/NAME/fixups.toml` saying
  `buildscript.run = true`; the prelude runs the script with the Buck2 Rust and C toolchains, so a probe gives
  the same `--cfg` flags and generated sources Cargo's would. Skipping a script would be faster to build but
  each skip is a guess about what that script decides.
- Native libraries are built by their own build scripts. `ring` (C and assembly) and `libsqlite3-sys` (bundled
  SQLite) compile through the cc crate with `toolchains//:cxx`. Their fixups set `rustc_link_lib`, so the
  script's `-l` flags reach rustc, and name the archive's directory with `$(location :NAME-build-script-run[out_dir])`
  in `rustc_flags` instead of `rustc_link_search`. The prelude declares `OUT_DIR` with a content-hashed path, so the
  `-L` it writes from the script's output names the pre-hash placeholder `output_artifacts/OUT_DIR`, which exists only
  on the host that ran the script: every remote compile of the two crates failed with `could not find native static
  library` (2026-11-12). Rewriting either as `cxx_library` rules would split the C compile into cached per-file actions at
  the cost of keeping a hand-written source list in step with each upstream release.

### Packages in `Cargo.lock` with no target

`Cargo.lock` locks every package any feature or platform could reach. Reindeer emits a target only for packages
the workspace builds on `linux-x86_64` with the features it enables: the 175 that
`cargo tree --workspace -e normal,build,dev --target x86_64-unknown-linux-gnu` lists on 2026-11-11, the same set
package for package. The other 90 entries fall in three groups:

- other platforms: `windows-*`, `windows_*`, `winapi*`, `crossterm_winapi`, `r-efi`, `redox_syscall`, `wasi`,
  `wasip2`, `wit-bindgen`, `js-sys`, `wasm-bindgen*`, `sqlite-wasm-rs`, `rsqlite-vfs`;
- optional dependencies no enabled feature reaches, such as ratatui's termwiz and termina backends and the
  palette, pest and phf crates they bring;
- older duplicate versions only those packages use.

### External-code review: reindeer

- What: Buck build-file generator for Cargo dependencies, by Meta (facebookincubator/reindeer).
- License: MIT (`LICENSE` in the repository; nixpkgs `meta.license` MIT). We run it and commit its output;
  no reindeer code enters the repository except the patch, which is a derivative of MIT code.
- Maintenance: active; weekly release tags (latest `v2026.11.09.00`), 585 commits from 2026-05-01 to
  2026-11-10, maintained alongside Buck2 by the same team.
- Pin: nixpkgs `reindeer` 2026.05.04.00 at the flake's locked nixpkgs (`7dcdd873`), fetched from the
  `v2026.05.04.00` tag tarball with the hash nixpkgs records, patched by the file above. The Buck2 from the same
  nixpkgs is 2026-04-14, so the prelude macros the generated file loads (`cargo.rust_library`,
  `buildscript_run`) match the generator's output.
- Reaches at generation time: `cargo metadata` (the crates.io index and registry downloads for packages not
  already in `~/.cargo`) and `rustc --print cfg`. At build time the generated targets reach
  `static.crates.io` once per package version, verified by sha256, and each build script runs inside its
  action as it would under Cargo. At run time: nothing; reindeer is not part of any north binary.
- Why it beats what exists: Buck2's own repository generates its third-party targets with it, and the prelude's
  `cargo` macros are written for its output. Hand-written rules for every package would drift at the first
  `cargo update`.
- Off the dev shell: reindeer is the flake's `packages.x86_64-linux.reindeer`, not a dev-shell package,
  because the patch takes it out of the binary cache and the laptop builds nothing.

## First-party targets: `north buck targets`

Every workspace crate, binary, test and example has a Buck2 target, written by `north buck targets` from
`cargo metadata` and the manifests' `[lints]` tables. The command owns `BUCK` at the repository root and in each
`crates/<dir>`; `--check` exits 1 on any file that differs, so the two graphs cannot drift while both exist.
`--check` also holds `third-party/BUCK` to the Cargo build (north#1115 box 4a). It reads Cargo's resolved packages
and features from `cargo tree --workspace --target x86_64-unknown-linux-gnu -e normal,build,dev` (the resolver's
answer; `cargo metadata` lists features of optional dependencies the build never turns on, 86 of them on 2026-11-12)
and finds a package Cargo builds without a target, a target built without a feature Cargo turns on (as `serde_json`
lacked `float_roundtrip` for part of 2026-11-12 and `rustix` lacked `event`), a target for a package Cargo no longer builds, and
a `//third-party:` label a workspace target depends on that the file does not define. A feature only the Buck target
has is not a finding, since reindeer may add one. The two cargo reads run side by side, so the comparison adds no
wall time to the check (3.0 s for both on the laptop on 2026-11-12, against 3.0 s for `cargo metadata` alone and
2.6 s for `cargo tree` alone). It is the same land check of `.safe-push` that holds `Cargo.lock` to the manifests
(north#1239), since one command reads both from the same tree. The findings are the `buck-targets-drift` gate of
`harness/gates.toml`: in shadow each prints as `buck-targets-drift: would refuse (shadow): ...`, every decision is
appended to `$XDG_STATE_HOME/north/gates/buck-targets-drift.jsonl` and the exit status is unchanged; enforcing, or
in a tree whose registry does not name the gate, a finding exits 1.

Prior art. Reindeer generates Buck2 targets for third-party crates from a `Cargo.toml` and is what box 2 runs on
`third-party/` [reindeer]; it is built for vendored or downloaded registry crates, sets `--cap-lints` and drops
first-party lints, so it does not fit first-party code, as the prelude's own `cargo_package.bzl` says
[prelude-cargo-package]. `cargo metadata` [cargo-metadata] gives the resolved graph, targets, editions and features
exactly as Cargo computes them, so the generator reads it instead of reimplementing manifest inheritance and target
discovery.

Decisions, each carried from what Cargo does:

- Names: the library is `//crates/<dir>:<crate>`, a binary `:<bin>` (`:<bin>-bin` when it shares the library's
  name, as `north-land-sandbox` does), a unit-test harness `:<target>-unittest`, an integration test
  `:<crate>-test-<name>`, an example `:<crate>-example-<name>`, a third-party crate `//third-party:<crate>`. Every
  test is box 5's `north_rust_test` (`build/rust_test.bzl`), which adds the test binary `<name>-bin` beside it and
  carries the nextest binary id (`<package>`, `<package>::bin/<bin>`, `<package>::<test>`), and every package has
  the `:files` that tests read at run time (the root's leaves out `crates/`, `target/`, `buck-out/`, `worktrees/`).
- Features: one feature set per crate, the one `cargo metadata` resolves, which with `resolver.feature-unification =
  "workspace"` is the set `cargo test --workspace` compiles. Cargo builds a second set for `cargo build`, without the
  `test-support` features that only dev-dependencies enable (north-capacity, north-fleet, north-fleet-supervisor,
  north-pacing); north-threads' `test-support` is already on in normal builds. One set means the runtime build and
  the test build share every library artifact, which is the point of this migration; the cost is that four crates'
  fixture modules, gated `#[cfg(any(test, feature = "test-support"))]` and additive only, compile into the release
  binary, where the linker drops what nothing calls.
- Flags: each target gets the rustc flags Cargo passes for the crate's lints (`--forbid=unsafe_code` and each
  `--warn=clippy::…`, ordered by priority then name, descending, as Cargo orders them) and its `--check-cfg` lines,
  and each library and binary `manifest_dir_flags()` (tests get it from `north_rust_test`). Profile flags (codegen
  backend, debuginfo, incremental) belong to the toolchain's modes, not to the targets.
- Environment: a target sets only the `CARGO_*` variables its sources read through `env!` or `option_env!`, with
  Cargo's values; the generator refuses a variable it does not know. `CARGO_MANIFEST_DIR` is never in `env`: it is
  the package's path from the repository root, set through rustc by `manifest_dir_flags()`, for the reason the
  Tests section gives. An integration test gets `CARGO_BIN_EXE_<bin> = $(location :<bin>)` in `run_env` for every
  binary of its package, whether or not its sources name one, because the sites read it through `bin_exe!`'s
  `option_env!` of a macro argument, which no scan of literals sees. `CARGO_TARGET_TMPDIR` is `/tmp`.
- Sources: a target's sources are its source directory (`src/`, `tests/` or `examples/`). A file it reads through
  `include_str!`, `include_bytes!`, `include!` or a `#[path]` module outside that directory is found by scanning the
  sources' tokens (string literals and comments skipped); when the file belongs to another package, that package
  exports it (`export_file`, or a `filegroup` for a module directory) and the target maps it in with
  `mapped_srcs`. When a reference leaves the crate's directory, the target's source tree is rooted at the repository
  root with every file at its repository path, so `include_str!("../../../concerns.toml")` resolves unchanged.
- Build scripts: the root crate's `build.rs` is the only one; it rewrites three committed registries
  (`src/cli/reference.txt`, `src/cli/verbs.rs`, `structure.toml`) from their per-unit trees and emits nothing rustc
  reads, so its targets compile the committed files and the generator says so when it runs. The lander renders the
  same files before a lane lands, so the committed copy is the one Cargo's script would write.
- Refused shapes: a `harness = false` target and `required-features`, rather than emit a target that differs from
  Cargo's.

## Runnable from source

Deep dive b of north#1247 (2026-11-12): which of north's crates a from-source runner such as [Relax](https://github.com/makepad/relax) could run
as they are, which after one move, and which not, read from manifests, build scripts and sources with no build. The
question behind it is whether north can hold itself to a restricted set of Rust so that most of it runs from source
in the development loop and optimised native builds are paid for only where they are worth it. Nothing here adopts
Relax: no hook, skill, dependency or config changes.

Rerun: `harness/scripts/from-source-census.sh . 2026-11-11` prints the tab-separated table, the group totals and the
test seconds of that day; `MD=1 harness/scripts/from-source-census.sh . 2026-11-11` prints the table below (59 lines of
shell: `cargo metadata`, `cargo tree --workspace`, `north code-health crate-lines`, `jq`, `rg`, `awk`; the test seconds
read the nextest `PASS` lines of the lander's shadow logs in the state directory, so they hold only while those logs do).
Counts below are from origin/main at the commit that adds this section.

### What a from-source runner needs

Asked of deep dive a (Relax at ce52155e, source read, nothing run); each line is the answer, and **assumed** marks what
no source read settles and the trial must show.

- Build scripts run, as Relax programs. A script is *native* when the manifest has `links =`, it build-depends on
  `cc`, `cmake`, `pkg_config`, `bindgen`, `vcpkg` or `nasm_rs`, or its text names `DEP_`. A native script that compiles C
  stops `relax run` and `relax test` unless `--allow-c` is given, and then cargo, rustc, a C compiler and a linker must
  be on the host (Relax builds only the native scripts and their build-dependencies, and loads the result as a shared
  library). So the C columns split as Relax splits them: *C-compiling* (needs `--allow-c` and a full native toolchain)
  and *links or system library only* (does not).
- Procedural macros: any, compiled from source in a separate macro world. Open gap: derive and function-like macros
  resolve by bare name across the whole graph, the first registered wins silently, and named collisions are an error.
  Hence the column of names exported by two proc-macro crates in one graph.
- `#![feature]` is accepted; a gate is checked as on rustc 1.97.1. Whether each nightly feature is implemented is
  **assumed**. `unsafe` and `extern` blocks work (symbols resolve through assembler labels, loader symbols, then
  `dlsym`); `asm!`, `global_asm!` and `naked_asm!` are assembled by Relax on x86-64 and AArch64. `std::arch` is **assumed**.
- Raw `syscall(2)` through libc is passed through and asm syscalls go through the assembler; threads are OS threads;
  `Command` is fork then exec with the child's code precompiled first. tokio, rustix, nix and landlock have no test input
  in Relax's repository: **assumed**. The standard library lacks `uid`, `gid` and `groups` on `CommandExt`, `PidFd`, `chroot`
  and `chown`; north uses none of them (`rg` over non-test sources).
- `std::env::current_exe` returns the Relax runner, not the program. Nextest cannot drive Relax's test harness (its own
  libtest port, no `--format`, no capture). `relax test PACKAGE` runs the library's unit tests, then each binary's, then each
  `tests/*.rs` target, each in its own process; doc tests are counted and not run (north has two, both in
  north-runtime; 43 manifests set `doctest = false`). `CARGO_BIN_EXE_<bin>` is a wrapper that starts `relax run`, so
  each spawn of the package's own binary is another JIT start.
- A run borrow-checks only the app's crates. The cache is per cargo target directory, so each worktree starts cold.

Columns the census added after that answer: native C split into C-compiling and links-only (plus C reached only through a dev-dependency, because `relax test` builds those), proc-macro name clashes,
tests that run the package's own binary apart from tests that start other programs, `CARGO_BIN_EXE` sites, and
`current_exe` sites (as a host interface). Dropped: nothing. Doc tests were asked for and counted once, above.

### The 65 crates

Every workspace member: the root package and 64 crates, 243,197 non-test lines. Facts that hold for every row:

- No `#![feature]`, no `unsafe` block, `unsafe fn`, `unsafe impl`, `extern` block or `asm!` in any of the 65
  (`unsafe_code = "forbid"` in `[workspace.lints.rust]`, inherited by every manifest). `north-strategy-types` has one
  `extern crate alloc`, which is not foreign code. Nightly is in the build, not the source: `.cargo/config.toml` sets
  `[unstable] codegen-backend` and `feature-unification`, and the dev profile uses Cranelift.
- No build script reads `DEP_` and none is native with `links =` only, so the links-only case does not occur. Every native
  script is the C-compiling kind.
- No two proc-macro crates in any crate's graph export the same derive or function-like macro name (checked on literal
  `#[proc_macro_derive(Name` and `#[proc_macro]` items; `derive_more` is in the TUI graph with the `error` derive off, so
  it exports only `IsVariant`). Own macros: `serde` is the `Serialize`/`Deserialize` derive (37 crates); `tokio` is
  `#[tokio::main]` and `#[tokio::test]` (6 crates); every other derive in first-party code is the standard library's.
- The only own `build.rs` is the root's, which rewrites three committed registries and emits nothing rustc reads. Build
  scripts in the graph are third-party (59 crates run between 1 and 20).
- Native C in a graph is SQLite (`libsqlite3-sys`, bundled by `rusqlite`; 52 crates) and `ring` (C and assembly; 51
  crates). Their direct callers are in the "Native C reached" column as call sites in non-test code.
- A test counts when it carries `#[test]` or `#[tokio::test]` (1,857; nextest ran 1,808 and skipped 2 on 2026-11-12
  at another tree). By file: *in-process* when the test file has no process marker, *own binary* when it names
  `CARGO_BIN_EXE` or `bin_exe!`, *other process* when it has `Command::new`, `.spawn(` or `tokio::process`. Inline
  `mod tests` count from `#[cfg(test)]` on. This is per file: a test that calls a helper in another file is counted by its own file.
- Host interfaces: `ptrace`, `landlock`, `seccompiler` (nix, landlock and seccompiler crates), `mount` and `pty` (rustix
  modules), `pidfd_open`, and `current_exe` (the program re-executes itself).

| Crate | Non-test lines | build.rs (own / in graph) | Native C reached | Proc-macro crates in graph | Own macros | Sys crates declared | Host interfaces | unsafe, extern, asm, feature | Tests: in-process, own binary, other process | Group |
|---|---|---|---|---|---|---|---|---|---|---|
| north-buck | 997 | n / 6 | none | 1 | serde | - | - | 0, 0, 0, 0 | 0, 0, 0 | 1 as is |
| north-claude-stream | 215 | n / 6 | none | 1 | serde | - | - | 0, 0, 0, 0 | 3, 0, 0 | 1 as is |
| north-cli-reference | 45 | n / 0 | none | 0 | - | - | - | 0, 0, 0, 0 | 0, 0, 0 | 1 as is |
| north-foundation | 901 | n / 7 | none | 1 | - | - | - | 0, 0, 0, 0 | 7, 0, 1 | 1 as is |
| north-git-locks | 136 | n / 0 | none | 0 | - | - | - | 0, 0, 0, 0 | 0, 0, 2 | 1 as is |
| north-monitor | 1802 | n / 12 | none | 9 | serde | crossterm | - | 0, 0, 0, 0 | 1, 0, 0 | 1 as is |
| north-publish | 21 | n / 1 | none | 0 | - | rustix | - | 0, 0, 0, 0 | 0, 0, 0 | 1 as is |
| north-registries | 65 | n / 0 | none | 0 | - | - | - | 0, 0, 0, 0 | 1, 0, 0 | 1 as is |
| north-runtime-follow | 83 | n / 0 | none | 0 | - | - | current_exe | 0, 0, 0, 0 | 0, 0, 0 | 1 as is |
| north-shared-config | 175 | n / 0 | none | 0 | - | - | - | 0, 0, 0, 0 | 0, 0, 1 | 1 as is |
| north-strategy-types | 607 | n / 4 | none | 1 | serde | - | - | 0, 0, 0, 0 | 0, 0, 0 | 1 as is |
| north-test-cases | 187 | n / 0 | none | 0 | - | - | - | 0, 0, 0, 0 | 0, 0, 0 | 1 as is |
| north-build-system | 519 | n / 13 | SQLite, ring | 2 | serde | - | - | 0, 0, 0, 0 | 1, 0, 1 | 2a one move (north-base) |
| north-code-health | 4109 | n / 13 | SQLite, ring | 2 | serde | - | - | 0, 0, 0, 0 | 1, 0, 25 | 2a one move (north-base) |
| north-concerns | 721 | n / 13 | SQLite, ring | 2 | serde | - | - | 0, 0, 0, 0 | 3, 0, 0 | 2a one move (north-base) |
| north-context-pack | 283 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 1, 0, 0 | 2a one move (north-base) |
| north-coverage | 1534 | n / 13 | SQLite, ring | 2 | serde | - | current_exe | 0, 0, 0, 0 | 16, 0, 0 | 2a one move (north-base) |
| north-diff-gates | 3307 | n / 13 | SQLite, ring | 2 | serde | - | - | 0, 0, 0, 0 | 0, 0, 7 | 2a one move (north-base) |
| north-fleet | 6399 | n / 13 | SQLite, ring | 2 | serde,tokio | rusqlite, rustix, tokio | current_exe | 0, 0, 0, 0 | 27, 0, 13 | 2a one move (north-base) |
| north-graph-change | 1994 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 0, 0, 11 | 2a one move (north-base) |
| north-graph-ground | 738 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 3, 0, 0 | 2a one move (north-base) |
| north-graph-loops | 799 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 9, 0, 0 | 2a one move (north-base) |
| north-implicit-state | 694 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 0, 0, 5 | 2a one move (north-base) |
| north-land-checks | 6220 | n / 13 | SQLite, ring | 2 | serde,tokio | rustix, tokio | current_exe | 0, 0, 0, 0 | 14, 0, 16 | 2a one move (north-base) |
| north-landing-gates | 6059 | n / 13 | SQLite, ring | 2 | - | - | current_exe | 0, 0, 0, 0 | 16, 0, 15 | 2a one move (north-base) |
| north-land-scans | 5123 | n / 13 | SQLite, ring | 2 | tokio | rustix | current_exe | 0, 0, 0, 0 | 2, 0, 21 | 2a one move (north-base) |
| north-land-watch | 772 | n / 13 | SQLite, ring | 2 | serde | rustix | - | 0, 0, 0, 0 | 10, 0, 0 | 2a one move (north-base) |
| north-lanes | 6717 | n / 13 | SQLite, ring | 2 | serde | rustix | - | 0, 0, 0, 0 | 40, 0, 15 | 2a one move (north-base) |
| north-mutants | 1016 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 1, 0, 0 | 2a one move (north-base) |
| north-replay | 612 | n / 13 | SQLite, ring | 2 | serde | rustix | - | 0, 0, 0, 0 | 6, 0, 0 | 2a one move (north-base) |
| north-resume | 92 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 0, 0, 0 | 2a one move (north-base) |
| north-safe-push | 3606 | n / 15 | SQLite, ring | 4 | - | rustix, tokio | current_exe | 0, 0, 0, 0 | 13, 0, 3 | 2a one move (north-base) |
| north-ssh-spare | 230 | n / 1 | none (tests via north-base) | 0 | - | rustix | current_exe | 0, 0, 0, 0 | 0, 0, 1 | 2a one move (north-base) |
| north-tui | 2539 | n / 18 | SQLite, ring | 13 | - | crossterm | - | 0, 0, 0, 0 | 6, 0, 0 | 2a one move (north-base) |
| north-base | 9087 | n / 13 | SQLite, ring (calls: 12) | 2 | serde | ring, rusqlite, rustix, tokio | pty,pidfd_open,current_exe | 0, 0, 0, 0 | 27, 0, 42 | 2b one move (own SQL) |
| north-broker | 1009 | n / 13 | SQLite, ring (calls: 2) | 2 | - | ring, rustix | - | 0, 0, 0, 0 | 4, 0, 7 | 2b one move (own SQL) |
| north-calibration | 2961 | n / 13 | SQLite, ring (calls: 37) | 2 | serde | rusqlite | - | 0, 0, 0, 0 | 4, 0, 0 | 2b one move (own SQL) |
| north-capacity | 5068 | n / 13 | SQLite, ring (calls: 3) | 2 | serde | rusqlite, rustix, tokio | current_exe | 0, 0, 0, 0 | 34, 0, 10 | 2b one move (own SQL) |
| north-capacity-host | 7837 | n / 13 | SQLite, ring (calls: 52) | 2 | serde,tokio | rusqlite, rustix, tokio | current_exe | 0, 0, 0, 0 | 48, 0, 7 | 2b one move (own SQL) |
| north-convo | 6632 | n / 13 | SQLite, ring (calls: 30) | 2 | serde | rusqlite, rustix, tokio | current_exe | 0, 0, 0, 0 | 4, 0, 6 | 2b one move (own SQL) |
| north-dispatch | 6628 | n / 13 | SQLite, ring (calls: 2) | 2 | serde | rusqlite | current_exe | 0, 0, 0, 0 | 89, 0, 3 | 2b one move (own SQL) |
| north-fleet-store | 2344 | n / 13 | SQLite, ring (calls: 13) | 2 | serde | rusqlite | - | 0, 0, 0, 0 | 19, 0, 0 | 2b one move (own SQL) |
| north-flow | 8776 | n / 13 | SQLite, ring (calls: 21) | 2 | serde | rusqlite, rustix | current_exe | 0, 0, 0, 0 | 28, 0, 0 | 2b one move (own SQL) |
| north-github-mirror | 536 | n / 13 | SQLite, ring (calls: 2) | 2 | - | rusqlite | - | 0, 0, 0, 0 | 5, 0, 0 | 2b one move (own SQL) |
| north-graph | 3789 | n / 5 | SQLite (calls: 30) | 0 | - | rusqlite | - | 0, 0, 0, 0 | 42, 0, 0 | 2b one move (own SQL) |
| north-graph-ingest | 1422 | n / 13 | SQLite, ring (calls: 13) | 2 | - | rusqlite | - | 0, 0, 0, 0 | 1, 0, 6 | 2b one move (own SQL) |
| north-guard | 18540 | n / 13 | SQLite, ring (calls: 1) | 2 | serde | rusqlite, rustix | current_exe | 0, 0, 0, 0 | 74, 0, 32 | 2b one move (own SQL) |
| north-health | 1292 | n / 13 | SQLite, ring (calls: 2) | 2 | serde | rusqlite | - | 0, 0, 0, 0 | 5, 0, 8 | 2b one move (own SQL) |
| north-ops | 14709 | n / 13 | SQLite, ring (calls: 40) | 2 | serde | rusqlite, rustix, tokio | pidfd_open,current_exe | 0, 0, 0, 0 | 54, 0, 15 | 2b one move (own SQL) |
| north-pace | 6341 | n / 13 | SQLite, ring (calls: 30) | 2 | serde | rusqlite, rustix | - | 0, 0, 0, 0 | 38, 0, 0 | 2b one move (own SQL) |
| north-pacing | 3550 | n / 13 | SQLite, ring (calls: 7) | 2 | serde | rusqlite, rustix | current_exe | 0, 0, 0, 0 | 19, 0, 2 | 2b one move (own SQL) |
| north-providers | 8284 | n / 13 | SQLite, ring (calls: 33) | 2 | serde | rusqlite, rustix | - | 0, 0, 0, 0 | 55, 0, 36 | 2b one move (own SQL) |
| north-threads | 6343 | n / 13 | SQLite, ring (calls: 78) | 2 | serde | rusqlite, rustix, tokio | - | 0, 0, 0, 0 | 28, 0, 22 | 2b one move (own SQL) |
| north-check-environment | 81 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 2, 0, 0 | 2c one move (north-base SQL API) |
| north-core | 13649 | n / 13 | SQLite, ring | 2 | serde | rustix, tokio | current_exe | 0, 0, 0, 0 | 28, 0, 11 | 2c one move (north-base SQL API) |
| north-fleet-supervisor | 5774 | n / 13 | SQLite, ring | 2 | serde | rustix | current_exe | 0, 0, 0, 0 | 53, 0, 8 | 2c one move (north-base SQL API) |
| north-lander | 5640 | n / 15 | SQLite, ring | 4 | serde | rustix | current_exe | 0, 0, 0, 0 | 22, 0, 66 | 2c one move (north-base SQL API) |
| north-lander-doctor | 1917 | n / 15 | SQLite, ring | 4 | serde | - | current_exe | 0, 0, 0, 0 | 0, 0, 16 | 2c one move (north-base SQL API) |
| north-landing | 6811 | n / 13 | SQLite, ring | 2 | serde | rustix, tokio | pidfd_open,current_exe | 0, 0, 0, 0 | 37, 0, 19 | 2c one move (north-base SQL API) |
| north-machine-model | 735 | n / 13 | SQLite, ring | 2 | - | - | - | 0, 0, 0, 0 | 0, 0, 0 | 2c one move (north-base SQL API) |
| north-session-host | 1184 | n / 13 | SQLite, ring | 2 | serde,tokio | rustix, tokio | - | 0, 0, 0, 0 | 2, 0, 1 | 2c one move (north-base SQL API) |
| north-tune | 1308 | n / 13 | SQLite, ring | 2 | - | - | current_exe | 0, 0, 0, 0 | 14, 0, 0 | 2c one move (north-base SQL API) |
| north | 15667 | y / 20 | SQLite, ring (calls: 32) | 14 | serde,tokio | crossterm, rusqlite, rustix, tokio | current_exe | 0, 0, 0, 0 | 119, 128, 31 | 3 native |
| north-land-sandbox | 912 | n / 15 | SQLite, ring | 4 | - | landlock, libc, nix, rustix, seccompiler | ptrace,landlock,seccompiler,current_exe | 0, 0, 0, 0 | 3, 19, 0 | 3 native |
| north-runtime | 15054 | n / 14 | SQLite, ring (calls: 2) | 4 | serde | landlock, rusqlite, rustix, tokio | landlock,mount,current_exe | 0, 0, 0, 0 | 53, 0, 133 | 3 native |

### Three groups

Groups follow from the table by rule (the script assigns them): *native* when non-test code uses `ptrace`, `landlock`,
`seccompiler` or `mount`, or is the root package; otherwise *one move* when a C-compiling build script is in the
graph (build graph or test graph) or two proc-macro crates share a macro name; otherwise *as is*.

- **Likely runnable as they are: 12 crates, 5,234 lines (2.2%).** No C, no `links` and no kernel interface in the build graph or the test graph (normal, build and workspace dev-dependencies):
  north-buck, north-claude-stream, north-cli-reference, north-foundation, north-git-locks, north-monitor, north-publish,
  north-registries, north-runtime-follow, north-shared-config, north-strategy-types and north-test-cases. (north-ssh-spare runs as it is, but its one test needs north-base as a dev-dependency, so its test graph reaches C and it is in 2a.)
  What is still **assumed** for them: `crossterm` and ratatui's nine proc-macro crates
  (north-monitor), `rustix` (north-ssh-spare, in 2a), `current_exe` (north-runtime-follow, which would see the runner, not itself).
- **Runnable if SQLite moves: 50 crates, 206,330 lines (84.8%).** All 50 reach `libsqlite3-sys` (C, built by `cc`; 49 through the build graph and north-ssh-spare through its test graph) and all but north-graph
  reach `ring` (C and assembly) through north-base or their own `rusqlite` dependency. The one named thing is a leaf crate
  for the SQLite adapter (call it north-sqlite, layer 1) that holds `rusqlite`, with the `ring` dependency of
  north-base, which no north-base source names, dropped (north-broker is the only crate that calls `ring`).
  What flips with that move depends on what each crate calls:
  - 2a, 22 crates, 54,083 lines: they reach C only because north-base declares `rusqlite` and `ring`, and call nothing of
    north-base's SQL (`lifecycle::Store`, `platform::sqlite`, `platform::usage_table`). The move alone makes them group 1 (the `State` and `Lifecycle` traits stay behind):
    north-build-system, north-code-health, north-concerns, north-context-pack, north-coverage, north-diff-gates,
    north-fleet, north-graph-change, north-graph-ground, north-graph-loops, north-implicit-state, north-land-checks,
    north-landing-gates, north-land-scans, north-land-watch, north-lanes, north-mutants, north-replay, north-resume,
    north-safe-push, north-ssh-spare and north-tui. north-base itself (9,087 lines, five files with `rusqlite`: `platform/sqlite.rs`,
    `platform/usage_table.rs`, `lifecycle/store.rs`, `lifecycle/store/outbox.rs`, `lifecycle/store/expiry.rs`) joins them.
  - 2c, 9 crates, 37,099 lines: they call north-base's SQL (check-environment, core, fleet-supervisor, lander-doctor,
    lander, landing, machine-model, session-host, tune). After the move they depend on the leaf and stay in this group
    until the call goes through a record crate (architecture layer 2) or Relax is run with `--allow-c`.
  - 2b, 19 crates, 115,148 lines: they call `rusqlite` or `ring` themselves (408 call sites in non-test code, 442 with the root and north-runtime; the largest are
    north-threads 78, north-capacity-host 52, north-ops 40, north-calibration 37, north-providers 33, north-pace 30,
    north-graph 30, north-convo 30, north-flow 21, north-fleet-store 13, north-graph-ingest 13, north-base 12). Six need
    three sites or fewer moved: north-guard 1, north-broker 2 (`ring`), north-dispatch 2, north-github-mirror 2,
    north-health 2, north-capacity 3.
  The other route for all 50 is not a move in north: Relax with `--allow-c` builds `libsqlite3-sys` and `ring` natively
  once (cargo, rustc, `cc` and a linker on the host) and loads them; it prices one native C build per version, not per edit.
- **Must stay native: 3 crates, 31,633 lines (13.0%).**
  - north (15,667 lines): the shipped binary; 128 of its tests start the built `north` (116 `CARGO_BIN_EXE` sites), each a
    JIT start under Relax; its own `build.rs`; it calls `rusqlite` 32 times.
  - north-land-sandbox (912): ptrace through `nix`, Landlock, `seccompiler` and `libc` system call numbers; its tests start its
    built binary (4 sites). The Relax sources mention none of these interfaces (**assumed** until the trial).
  - north-runtime (15,054): `worker_sandbox` uses Landlock and `mount`. Box L18 of north#1276 moves it to
    north-worker-sandbox; north-runtime then has two `rusqlite` sites and belongs to 2b.

### Conventions that keep the first group growing

Draft, for review; none is adopted. Each lists its cost and the crates that break it today. "Plan" says
whether `docs/rationale/architecture-layers.md` and the L boxes of north#1276 already ask for it, so that these and the
architecture plan are one plan.

1. **C and assembly only in leaf crates behind a narrow interface; `rusqlite` and `ring` named in one leaf each.** Cost: a
   new leaf crate (five north-base files), the `Store` imports of 9 crates, and a land scan that refuses a new manifest edge to
   `rusqlite`, `ring` or a `-sys` crate outside the leaves. Breakers: 21 crates call `rusqlite` or `ring` (list above, plus
   north and north-broker); north-base declares `ring` and never names it. Plan: partly. Layer 1 holds the adapters
   "(git, GitHub, processes, SQLite, ssh, Nix)" and layer 2 is "one crate per durable store", so a layer-3 or layer-4
   crate with `rusqlite` calls (north-guard, north-pace, north-health, north-pacing, north-capacity-host, north-providers)
   already departs from the target; but no box splits the SQLite adapter from north-base (L48 moves the landing records, and
   `platform/sqlite.rs` stays) and nothing checks who may name `rusqlite`.
2. **No `build.rs` outside the root's registry writer, and none that compiles C.** Cost: none. Breakers: none (one own
   script, not native). Plan: not asked.
3. **A closed list of macros in first-party code: the standard derives, serde's two, and `#[tokio::main]` and
   `#[tokio::test]`; a new proc-macro crate in a graph goes through the dependency review and is refused when it exports a
   name another crate in that graph exports.** Cost: none today. Breakers: none (0 clashes in 65 graphs). Plan: not asked;
   the dependency gate (`crates/north-diff-gates/src/dep_gate.rs`) and north#1246 (dependencies by what each brings) are
   where it would sit.
4. **No `#![feature]`, and no first-party need for a nightly rustc; nightly stays in `.cargo/config.toml` and the
   toolchain pin.** Cost: none. Breakers: none (0 of 65). Plan: not asked.
5. **No `unsafe`, `extern` block or `asm!` in first-party code.** Cost: none, it is enforced (`unsafe_code = "forbid"`).
   Breakers: none. Dependencies are outside it: `ring` carries assembly and `rustix` (0.38.44 and 1.1.4, `linux_raw`
   backend, no `use-libc`) issues system calls and reads the vDSO through `asm!`; Relax assembles `asm!` (**assumed** for
   these two). Plan: not asked.
6. **Kernel-interface code (`ptrace`, Landlock, seccomp, `mount`, `pty`, `pidfd`) in named leaf crates.** Cost: L18 moves
   the sandbox; the `pty` and `pidfd_open` sites are small moves (`north-base/src/platform/process.rs` and `terminal.rs`,
   `north-landing/src/land/wake.rs`, `north-ops/src/ops/job_run/records.rs`). Breakers: north-runtime (until L18),
   north-base (`pty`, `pidfd_open`), north-landing and north-ops (`pidfd_open`). Plan: partly, L18 only.
7. **Tests that run the built binary live in the root's `tests/it`, and a crate's tests start no other program except
   through the one git fixture.** Cost: moving the 19 tests of north-land-sandbox (4 sites) next to the root's. Breakers: that one crate.
   Beyond it, 587 tests in 35 other crates start another
   process, many of them through git fixtures. Plan: partly. L35 and L36 make one git fixture, which is the other-process half; the
   built-binary half is not asked.
8. **A library crate takes the path of the program it starts instead of calling `current_exe`; the binary supplies it.**
   Cost: a program path argument at each of 26 crates' call sites, filled by the command layer. Breakers: 26 crates use
   `current_exe` in non-test code (north-runtime 14 files, north 12, north-ops 8, north-dispatch 7, north-base 7); under
   Relax each would start the runner, not north. Plan: in spirit, layer 7 "wire ports to their implementations" and layer
   4 "the ports a family needs filled from outside"; no box.

### The most a from-source test loop could take over

Yesterday's test seconds are the `PASS` times in the lander's shadow-run logs of 2026-11-11 (347 logs with results, 139,152
passing test executions, 97,165 s; a test run on several lanes counts each time, and the logs hold the selection the lander
made, not the whole suite).

- Group 1 today: 16 of 1,857 tests (0.9%), 12 in-process and 4 starting other processes; 319 s (0.3%).
- Group 1 after the SQLite leaf (adds the 22 crates of 2a and north-base): 387 tests (20.8%), 208 of them in-process;
  8,660 s (8.9%).
- Everything outside the native group, SQLite handled in every crate (route through the leaf, record crates, or
  `--allow-c`): 1,371 tests (73.8%); 44,257 s (45.5%). This is the ceiling: the native group is 486 tests (26.2%) and
  52,908 s (54.5%), and the root's two binaries (`north` and `north::it`) alone are 48,294 s (49.7%) of all test seconds.
- By group, tests and seconds: 2a 302 tests, 6,665 s; 2b 774 tests, 10,491 s (north-base 69 tests, 1,676 s of it); 2c 278
  tests, 26,781 s (north-landing, north-core and north-lander are most of it).

What follows for the loop: group 1 is too small to matter before the leaf split, and the leaf split alone reaches about a
fifth of the tests and a ninth of the seconds. The seconds sit in the root's integration tests, which run the built
binary and stay native. Relax's own test harness replaces nextest, so the graph-selected runs that the lander and the
workers use (`north land tests`, the coverage map) would need a runner for that harness before a from-source loop takes
any of this over.


## Relax: a just-in-time Rust runner (north#1247, trial of 2026-11-12)

Question: could a worker's inner loop run a crate's unit tests with Relax, with no test binary built, and is that at
least five times faster from edit to verdict than `cargo nextest run -p CRATE` with equal verdicts for at least 80%
of the tests? The source reading is [watch/2026-11-12-relax.md](watch/2026-11-12-relax.md); the external-code record
is [references.md](../references.md#relax). Relax is another compiler, so it could never be the landing's verdict or
the runtime build; this section measures only the edit-run loop. Nothing is adopted: no hook, skill or config.

### Method

- Relax at `ce52155e0aa8e1ce273ca08627b714e875f04b9d` (`relax 0.1.0 (beta)`, build `1aecaaf10d49`), cloned into its own
  directory on the build host, outside every north checkout, and built there with north's pinned `nightly-2026-04-10`
  (`cargo build --release -p relax-runner --bin relax`). North was cloned once at the lane's base; each crate had its own
  Relax and cargo target directory. The headline numbers were taken on cpx62b, which also serves land checks; a first
  set on cpx62 was taken beside another registered job and is quoted only where marked.
- Edit: one line, `let _relax_probe = N;`, inserted as the first statement of a function a test enters
  (`foundation::cause::push_token`, `north-graph`'s `parse_arg`, `north-base`'s `verdict_cache::fingerprint::without_selection`),
  with a different `N` on each of three runs so no run can be a cache hit. Edit to verdict is the wall time of the next
  `relax test CRATE` (or `cargo nextest run -p CRATE`), median of three. Memory is the growth of the job cgroup's
  `memory.peak` over the command, which counts page cache and all child processes: an upper bound, not a resident set.
- Unit tests are the tests `cargo nextest run -p CRATE` runs; none of the three crates has a `tests/` directory.
  Relax's verdicts were compared with nextest's test by test (name and pass or fail).

### Results

- Building Relax: 169 s on cpx62b (129 s on cpx62), peak 3.6 GB. Its `Cargo.lock` (generated, the pin has none) holds
  36 packages, none from a registry. The link needs system OpenSSL development libraries that north's
  dev shell lacks (first error below).
- `north-foundation` (leaf, 8 unit tests, no C): `relax check` passes with 0 errors; `relax test` ran 8 of 8 tests and
  all 8 verdicts equal nextest's. Edit to verdict on cpx62b: `relax test` 2.11 s (2.03, 2.20, 2.11), `relax check` 1.01 s
  (1.11, 1.01, 0.87), nextest 1.92 s (1.91, 1.92, 2.00): Relax is 0.9 times as fast, not five times. On cpx62 it was
  1.59 s against nextest's 1.91 s (1.2 times). Memory: `relax test` 290 to 370 MB warm and 880 MB cold, `relax check`
  210 to 230 MB after an edit and 720 MB cold, nextest 140 to 160 MB. The tests themselves are the floor: Relax ran
  them in 2.14 s where nextest ran them in 0.71 s.
- `north-graph` (SQLite through `rusqlite` 0.41 and bundled `libsqlite3-sys` 0.39.1; 42 tests): `relax check` passes
  without `--allow-c` (0 errors, 497 body owners) and with it (10.6 s cold, 1.38 s after an edit, 830 MB cold);
  `relax test` stops before any test, 0 of 42 ran. nextest ran 42 of 42, edit to verdict 1.59 s (1.86, 1.53, 1.59),
  230 to 290 MB.
- `north-base` (hub, 50 dependants, SQLite and `ring`; 69 tests): `relax check` passes without `--allow-c` and exits 1
  with it; `relax test` stops before any test, 0 of 69 ran. nextest ran 69 of 69, edit to verdict 8.45 s (8.45, 8.55,
  8.43), of which 6.7 s is the tests, 370 to 450 MB.
- Across all 56 library crates that have unit tests (1,504 `#[test]` functions by count), `relax test` with
  `MAKEPAD_RUN_ALLOW_C=1` and the workspace cut to the one member: 2 crates ran (`north-tui` 6 of 6, `north-claude-stream`
  3 of 3, all passed) and 54 stopped at the first error of blocker 1 below before running a test (`north-monitor` on
  `rustix 0.38.44` instead of `1.1.4`). With `north-foundation`'s 8 (run without the flag), 17 of the 1,501 library unit
  tests run today (1%); the other 1,484 do not.
- North itself from source: first try `relax run --quiet north -- --version` stopped at `could not determine which binary to run:
  use --bin (available: north, verify-north, fake_claude, push_trace_git)`; the second run below got past it.

### Second run: past blocker 1 (cpx62, later the same day)

With `CARGO_RESOLVER_FEATURE_UNIFICATION=selected` in the environment of the cargo Relax starts (the env value overrides
north's config key), `MAKEPAD_RUN_ALLOW_C=1`, `RUSTC_WRAPPER` unset and the real cargo first on `PATH`, one fresh target
directory per crate:

- The refusal is gone. `north-graph` (bundled SQLite) ran 42 of 42 tests and `north-base` (SQLite and `ring`) 69 of 69,
  every verdict equal to nextest's (119 of 119 with `north-foundation`'s 8). The `rusqlite` and `ring` C builds went
  through cargo, the C compiler and the linker with no further error.
- Seconds, first verdict in a fresh target directory (native C build included): `north-foundation` 12.7 s, `north-graph`
  14.7 s, `north-base` 16.3 s; peak memory growth 0.84, 1.6 and 2.5 GB. After the optimized native library is built in the
  background, the warm run is 1.52 s (graph) and 8.95 s (base). Cache size was not measured.
- Edit to verdict, median of three, against `cargo nextest` on the same host: `north-graph` Relax 2.03 s (2.08, 1.93, 2.03)
  against 1.47 s (1.93, 1.43, 1.47), 0.7 times as fast; `north-base` Relax 8.11 s (8.32, 8.11, 8.06) against 8.30 s
  (8.20, 8.31, 8.30), 1.0 times, 6.7 s of which are the tests themselves. `relax check` after the edit: 1.27 s (graph),
  2.94 s (base). Memory after an edit: Relax 240 to 290 MB (graph) and about 1.0 GB (base) against nextest's 240 to 260 and
  360 MB.
- All 56 library crates with unit tests, `relax test` once each (1,517 `#[test]` functions by this loop's count): 36 crates
  exit 0 and 20 do not; 718 tests pass and 5 fail, 48% of the library unit tests, compared with nextest test by test only
  on the three crates above. The 20: 15 crates (734 tests, none ran) stop at a borrow-check error on valid code,
  `error[E0597]: pathspecs does not live long enough` or `base`, `config.sources`, or `error[E0521]: borrowed data
  escapes outside of function` (`north-runtime`, `north-guard`, `north-providers`, `north-ops`, `north-lanes`,
  `north-landing`, `north-landing-gates`, `north-land-checks`, `north-lander`, `north-land-scans`, `north-safe-push`,
  `north-lander-doctor`, `north-land-watch`, `north-replay`, `north-mutants`); `north-fleet-supervisor` (62) stops at
  `E0433: failed to resolve: use of undeclared type Paper` in a test file; `north-dispatch` ran 90 of 94 and one panicked
  in the JIT (`compile error in time::num_fmt::str_from_raw_parts`); `north-pacing` ran 20 of 21 and failed on
  `assertion left == right failed, left: Some(45.0), right: None`; `north-broker` 9 of 11 (`refused: a lane-only
  session`, a test that reads its caller's identity); `north-land-sandbox` 21 of 22.
- North itself from source: `relax run --quiet north --bin north -- --version` started the `north` binary: 30.6 s the
  first time (the native build included, 3.4 GB peak growth), 1.02 s the second (290 MB), and the binary's own parser
  answered `error: unrecognized option '--version'` (exit 2), as the real binary does.

### Third run: the workspace case (cpx62b, 13 November)

The second run timed the case where rustc is fast: one crate, where most of the time is the tests. This run times the case
stage 0's second bar names, one line in `north-foundation` and the unit tests of its dependants. Same host throughout,
8 cores shared with other jobs (host load average 3 to 96 across the rounds, printed beside each figure). Relax at
`ce52155e`, a north clone at `87e45eb0e`, `RUSTC_WRAPPER` unset, the real cargo first on `PATH`. Relax side:
`CARGO_RESOLVER_FEATURE_UNIFICATION=selected MAKEPAD_RUN_ALLOW_C=1`, `CARGO_TARGET_DIR` one directory for all crates, each
crate run from its own directory by `relax test CRATE`, the crates one after the other. Nextest side: north's own
config (workspace unification, cranelift dev profile), its own target directory.

The set. The dependants of `north-foundation` among the workspace members are 51 crates (the root package `north`
left out; edges from `cargo metadata`, dev-dependencies included). `relax test` once on each, 4 at a time, 120 s cap:
29 crates exit 0 with at least one test, 514 tests, none failed; 18 exit 101 (`north-broker`, `north-dispatch`,
`north-fleet-supervisor`, `north-guard`, `north-land-checks`, `north-lander-doctor`, `north-lander`, `north-landing-gates`,
`north-landing`, `north-land-scans`, `north-land-watch`, `north-lanes`, `north-mutants`, `north-pacing`, `north-providers`,
`north-replay`, `north-runtime`, `north-safe-push`); 2 hit the 120 s cap (`north-land-sandbox`, `north-ops`, under a host
load average near 30); 2 exit 0 with no test (`north-machine-model`, `north-resume`). Nextest over the same 29 packages
(`cargo nextest run -p A -p B ... --no-fail-fast`): 514 tests across 29 binaries, 514 passed.

The edit. One line after `fn push_token` in `crates/north-foundation/src/foundation/cause.rs`:
`let _relax_probe = 111;` (222, then 333). Each round: the edit, `relax test` over the 29, then `cargo nextest run` over
the 29 with the same edit. Seconds from the edit to the last verdict; `before` is everything but the tests'
own run (build, JIT compile, link, load, per-crate start), `tests` is from the first `running N tests` line to the
`test result:` line of each Relax test program summed, and from nextest's `Starting` line to its `Summary` line.
`cpu` is user plus system seconds of the command's process tree (bash `time`), which load does not inflate as much.

- Round 1 (load 7.6): Relax 449.6 s (before 414.8, tests 34.8; cpu 324.8 + 85.6 = 410.4); nextest 38.3 s (before 14.1,
  tests 24.2; cpu 46.8 + 33.3 = 80.0).
- Round 2 (load 5.5 at the Relax start, 15.1 at nextest's): Relax 421.2 s (before 385.7, tests 35.6; cpu 163.8 + 67.8 =
  231.7); nextest 50.3 s (before 21.9, tests 28.4; cpu 52.5 + 32.8 = 85.4).
- Round 3 (load 23 at the Relax start, 96 at nextest's): Relax 632.7 s (before 560.8, tests 71.8; cpu 199.6 + 66.1 =
  265.7); nextest 98.4 s (before 52.7, tests 45.8; cpu 50.4 + 29.4 = 79.8).
- Medians: Relax 449.6 s against nextest 50.3 s, nextest 8.9 times as fast; by cpu seconds 265.7 against 80.0, 3.3 times
  as much. All 514 tests passed on every round on both sides. Stage 0's bar was a fifth of nextest's time (about 10 s).
- Around the edits, same commands, no edit: Relax warm 76.3 s and 71.1 s (tests 33.7 and 33.3), the first run in a
  fresh target directory 600.5 s (cpu 929.5 + 238.6); nextest warm 17.6 s (0.8 s before the first test, 16.8 s tests), the
  first run in a fresh target directory 65.3 s (48.4 s before the first test).

What limits it: after an edit to a crate every dependant uses, `relax test` recompiles each of the 29 crates in turn
and the time before the first test is 92% of the round (414.8 of 449.6 s, 385.7 of 421.2 s, 560.8 of 632.7 s), while
nextest's rebuild and link of the same 29 crates is 14 to 53 s across the same three rounds.

### Fourth run: `relax test --live` and the run case (cpx62b, 13 November)

Same host, clone (`87e45eb0e`), Relax pin and environment as the third run, same 29 crates and same edit (`let _relax_probe = N;`
after `fn push_token` in `north-foundation`, a different N each time). The host's load average (1 minute) is printed beside
each figure; it was 12 to 130 on 8 cores, other land checks and workers running throughout. `cpu` is user plus system
seconds from the job cgroup's `cpu.stat` over the timed window.

`relax test --live` across the crates. One `relax test --live CRATE` per crate, all 29 started at once in each crate's own
directory with one shared `CARGO_TARGET_DIR`, each left running until it printed its first verdict; then the edit, and the
time from the edit to the last of the 29 new `test result` lines. Nextest, same edit and round, `cargo nextest run` over the
same 29 packages (514 tests, 514 passed on every round):

- Round 1: live 12.25 s (cpu 80.3, load at the edit 73.9); nextest 40.20 s (cpu 89.4, load at the start 12.1; 16.1 s before the
  first test).
- Round 2: live 12.89 s (cpu 58.7, load 67.6); nextest 41.51 s (cpu 88.2, load 57.3; 17.2 s before the first test).
- Round 3: live 12.61 s (cpu 65.0, load 130.9); nextest 49.20 s (cpu 97.4, load 71.0; 21.1 s before the first test).
- Medians: live 12.61 s against nextest 41.51 s, 3.3 times as fast; cpu 65.0 against 89.4. Stage 0's bar, a fifth of nextest's
  time, is 8.3 s, so `--live` misses it by 4 s. Against the non-live `relax test` of the third run (449.6 s) it is 36 times as fast.
- The standing cost: starting the 29 processes took 192, 148 and 119 s to their first verdicts, and the job cgroup held 13.4 to
  15.5 GiB while they ran (peak 14.4 to 16.2 GiB in the window; page cache included; the cgroup's limit is 23 GiB).
- Issue 33 did not stop it at any of the 29 crates: in round 1 every crate printed a new verdict after the edit, and the later
  rounds waited for a new verdict from all 29 as well, but round 2's report line for `north-base` is missing (its count may
  have taken an earlier line) and `north-capacity`'s lines in rounds 2 and 3 are lost to a binary byte in its log, so those are
  not quoted per crate. The slowest edit-to-verdict were `north-threads` (12.2, 12.7, 12.6 s; its 50 tests take 11.9 s) and
  `north-base` (11.6 s and 9.7 s; 69 tests, 8 to 14 s); the rest came in under 5.6 s, 13 of them under 1 s in round 1.
- What it reran: the edited function is entered by no dependant's test, so 27 of 29 crates reran every test (`reached by no
  test here: north_foundation::foundation::cause::push_token`); `north-fleet` and `north-fleet-store` reran only the tests
  Relax selected (8 of 38 and 1 of 20). The case where a test enters the edited function, and live reruns only the tests that
  did, was not measured.
- Verdicts: 27 crates passed every test on every rerun. `north-capacity-host` failed 2 of 55 on each round
  (`handover_tests::capacity_a_waiting_run_prints_the_takers_stream_and_exits_with_its_code_and_a_taker_that_dies_gives_it_back`
  and an `offering_tests` case; one panic showed `Compiling northrunning 3 tests` in a captured stream) and `north-fleet` failed 1 of
  38 in a rerun (`fleet::watch::tests::a_turn_is_charged_to_the_account_its_host_started_with_not_the_one_the_record_holds_when_it_ends`),
  where nextest passed all 514 on all three rounds. Whether the host's load or the live harness causes the three is not isolated.

What limits it: the wait after the edit tracks the slowest dependant's own test run under the JIT (`north-threads` 12.2 s from
edit to verdict for 11.9 s of tests, `north-base` 9.7 to 11.6 s), so a crate's tests are the floor again; the price is 29
resident processes and up to three minutes to start them.

The run case. `relax run --quiet north --bin north -- capacity offload choose` against `cargo build --bin north` then
`target/debug/north capacity offload choose`, the edit being a word added to a message in
`crates/north-runtime/src/land_offload/mod.rs`; seconds from the edit to the changed text on screen, one warm-up edit
of each first (relax 5.06 s; cargo 38.50 s, cpu 100.7), then three rounds:

- Round 1 (load 18.6): relax 4.30 s (exit at 6.07 s, cpu 15.1); cargo 7.27 s (cpu 7.1).
- Round 2 (load 18.3): relax 2.42 s (exit at 3.86 s, cpu 11.4); cargo 6.21 s (cpu 6.9).
- Round 3 (load 17.6): relax 2.33 s (exit at 3.36 s, cpu 11.6); cargo 5.76 s (cpu 6.9).
- Medians: relax 2.42 s against cargo 6.21 s, 2.6 times as fast, at 11.6 against 6.9 cpu seconds, 1.7 times as much.

What limits it: cargo's 5.8 to 7.3 s are all in `cargo build` (running the binary took 0.02 s); Relax's 2.3 to 4.3 s are its own
compile and load of the edit, and it spends more cpu to get there.

The single-crate split (north-base's tests against the rest, and JIT against native test-body time) was not measured:
cpx62b closed for another eval before its job could start (2026-11-13, 4:08 AM).

### Blockers, in the order they stop north

1. Feature sets, a setting of north's own. `relax test` and `relax check` with `--allow-c` refuse any package whose
   graph has a native build script (`rustix` 1.1.4 has one), with: `--allow-c: cargo and Relax resolve rustix 1.1.4
   with different features (cargo [alloc, default, fs, std, stdio, termios], relax [alloc, default, fs, std])`. It is
   raised at `libs/relax/front/src/native.rs:581` (`run_cargo_with`), which runs `cargo check --release
   --message-format=json -p PACKAGE --target TRIPLE` in the workspace directory (`native.rs:488-494`; a test selection
   becomes `--tests` so dev-dependencies resolve, `native.rs:601-625`), reads each `compiler-artifact`'s `features`,
   and refuses when none equals Relax's own resolution of the same package. North's `.cargo/config.toml` sets
   `[resolver] feature-unification = "workspace"` (with `[unstable] feature-unification = true`), so the cargo Relax
   starts resolves a `-p` package's features as `--workspace` does: the `stdio` and `termios` on `rustix` come from
   `crossterm` and `termina` elsewhere in the lock, while the only package in `north-graph`'s closure asking for
   `rustix` 1.x is `tempfile` (`fs`, `std`), which is Relax's set. A second run with `CARGO_RESOLVER_FEATURE_UNIFICATION=selected` in the environment settled it: the refusal is gone and
   the C crates build (second run, below). It also fits the first run: `north-tui`, whose own graph has `crossterm`, ran
   6 of 6, and cutting `members` to one crate (my one try) failed the same way because the root package `north` stays a
   member. Relax's own options do not move either side except `--features` and `--no-default-features`, which go to
   both (`native.rs:908-912`); the test page takes `<package> [--lib|--bins|--tests] [-- test arguments]` and the check
   page `--workspace` and `--also=<pkg,..>` (`tools/run/runner/src/help.rs:307, 334-343`); `-p` appears only on `clean`
   (`help.rs:396`). So blocker 1 is north's setting, not a Relax gap, and the cargo Relax starts needs that environment
   value, which a worker's loop would have to export.
2. C libraries. Without `--allow-c`, `relax test` refuses `libsqlite3-sys` 0.39.1 and `ring` 0.17.14 at once
   (`registry package libsqlite3-sys 0.39.1 compiles C or C++ (build-dependency cc): it is built only with
   --allow-c`). With it and blocker 1 lifted they build: a fresh target directory's first verdict took 12.7 to 16.3 s,
   cached afterwards. The flag makes Relax start real cargo, rustc, the C compiler and the linker for those packages,
   so Relax no longer replaces them there.
3. Valid code Relax rejects or miscompiles. Past blockers 1 and 2, 15 of the 56 crates (734 of the tests) stop at a
   borrow-check error on code rustc accepts (`E0597`, `E0521`), one at `E0433` on a test file's type, and a few tests
   fail in the JIT or on a platform difference (`time`'s `str_from_raw_parts`, a `None` where `Some(45.0)` is expected,
   a caller-identity refusal). These are Relax fidelity gaps, each with a first error above; none is north's to fix.
4. The count and the speed. Measured: 718 of 1,501 library unit tests pass (48%), 119 of 119 equal to nextest's on the
   three crates compared. Where a crate runs, the edit loop is no faster: 0.9 to 1.2 times for the leaf, 0.7 times for
   `north-graph` and 1.0 times for the hub, because the tests dominate (0.7 s native, 2.1 s under the JIT, 6.7 s for
   the hub's) and `cargo nextest`'s rebuild and link of a crate is under 1.6 s on these hosts. Lifting blockers 1 to 3
   would not by itself reach five times.

### Failures, with the first error of each

- Link of Relax itself: `rust-lld: error: unable to find library -lssl` (and `-lcrypto`), cause: no OpenSSL development
  libraries in the dev shell; the runner links system OpenSSL (`#[link(name = "ssl")]`), which the README does not
  list. Fixed for the trial by the store's x86-64 `openssl-3.5.9` lib directory through an untracked
  `.cargo/config.toml` in the scratch clone (the 32-bit variant failed with `is incompatible with elf64-x86-64`).
- Any build script under the dev shell's `RUSTC_WRAPPER=sccache`: `libc-0.2.189/build.rs: build script failed (exit
  101): failed to run rustc: sccache: error: failed to execute compile ... Compiler not supported: makepad: not
  rustc`, and `rustversion-1.0.23/build/build.rs: ... unexpected output from rustc --version: ""`, then `castaway` and
  `compact_str` derive failures (`north-monitor`, `north-foundation`). Cause: the wrapper in the environment; with
  `RUSTC_WRAPPER` unset `north-foundation` ran.
- `--allow-c` through the dev shell's cargo shim: `could not execute process <relax>/target/release/relax rustc -vV (never
  executed)`: cargo started by Relax ran where Relax's own binary was not visible. The runs above put the real cargo
  first on `PATH`.
- Blocker 1 and blocker 2 above, for 54 crates; `relax check` on `north-base` and `north-tune` with `--allow-c`
  exits 1 with the same blocker-1 line as its first error.
- `relax run north --version`: `unit graph: flag --version is not supported by makepad run's resolver` (it wants
  `--` before program arguments).

### What cannot work, whatever its speed

- North's integration tests run the built `north` binary, so they need the real build. By a count of `#[test]`
  functions: 1,501 are unit tests in the 64 library crates' `src` (including `#[cfg(test)]` modules), 83 are in the
  crates' `tests/` directories, 94 are in the root package's `src` and 183 are in the root `tests/`, 147 of them in
  files that name a built binary (`bin_exe!` or `CARGO_BIN_EXE`). Unit tests in library crates are 1,501 of 1,861
  (81%) and are the only tests that could run this way; the other 360 (19%) cannot.
- Relax's std replaces parts of the real one, which a unit test can touch (from the source reading, not measured):
  `std::env::current_exe` names the Relax runner; `CommandExt` has no `uid`, `gid` or `groups`; there is no `chown`,
  `chroot` or `PidFd`; doc tests are counted, not run.
- Nightly: north's source uses no `#![feature]` (searched across `src`, `crates` and `tests`); the pin shows only in
  `.cargo/config.toml` (`[unstable] codegen-backend`, cranelift), a setting Relax does not read. Edition 2024 and
  `rust-version = "1.96"` did not stop `relax check` or `relax test` on `north-foundation`, `north-tui` or
  `north-claude-stream`. No nightly feature stopped a run.

### Verdict: drop for the edit loop

By the box's rule (at least 80% of the three crates' unit tests run with equal verdicts, and an edit to verdict at least
five times shorter): the tests of the three crates run and agree (119 of 119, once blocker 1 is lifted), but the edit to
verdict is 0.9, 0.7 and 1.0 times as fast as nextest's on the leaf, the SQLite crate and the hub, so the second
condition fails by a wide margin and the answer is drop. The staged plan's bars (docs/rationale/watch/2026-11-12-relax.md)
read the same way: 718 of the 1,501 library unit tests pass against at least 1,201 required (48%), a hub's edit to
verdict is 1.0 times against at least five, and a fresh target directory's first verdict is inside 60 s (12.7 to 16.3 s;
cache size not measured). What reopens it is a hub whose rebuild and link, not its tests, dominate its loop, and Relax
fixing the borrow-check false positives behind blocker 3; neither is in hand.

[reindeer]: https://github.com/facebookincubator/reindeer
[prelude-cargo-package]: https://github.com/facebook/buck2-prelude/blob/main/rust/cargo_package.bzl
[prelude-build]: https://github.com/facebook/buck2-prelude/blob/main/rust/build.bzl
[cargo-metadata]: https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
