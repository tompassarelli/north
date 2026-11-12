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
north#1116 a worker created `buck-out/` before its first `buck2` command; the sandbox now prepares a Buck2 lane's
`buck-out/` before it builds a command's domain (crates/north-runtime/src/worker_sandbox/buck.rs), and
`north lane remove`, `reap` and `gc` count `buck-out/` and `.buckconfig.flake` as disposable, as they do `target/`.

Reads fail the same way: the sandbox fixes what a domain may read when it builds the domain
(`crates/north-runtime/src/worker_sandbox/readable.rs`), so a directory created later is unreadable inside it. On gex45
on 2026-11-11, a worker's first `buck2 build` on the host created `~/.buck` inside a domain built before `~/.buck`
existed. The daemon could not read its own `buckd.pid` and shut itself down at its 60 s self-check. The client gave
up after 803 attempts over 89.8 s. The next start in that lane stopped at
`remove_dir_all(~/.buck/buckd/<lane>/v2/prev): Permission denied`.
Box 6 fixes all three in the sandbox ([Lanes](#lanes-north1115-box-6), decision 6).

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
warmed slot, and the laptop as a client that runs nothing itself. It was written for box 6 before box 6 is built.
Every number was measured on gex45 on 2026-11-11 at 2bd22a2744 unless another source is named. The runs were
`buck2 build //...` in fresh worktrees of origin/main, inside the dev shell and under a 6-core heavy capacity lease.
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
2. **Every checkout builds under the default isolation dir `v2`.** A second isolation dir shares nothing with `v2`
   (see Measured on Buck2) and rebuilds everything once for its name: 47 to 74 s cold and about 2,400 more
   entries in the 64 GiB store. The build seam passes no `--isolation-dir`, so `north build-time` measures with
   `--no-remote-cache` under `v2`, not in a scratch dir (box 7a's inventory planned a scratch dir).
   - The one exception is rust-project's `.rust-analyzer` dir (box 10), which keeps an editor's builds from waiting
     on the lane's build. The path is in no key, so its proc-macro and build-script outputs (394 of 2,425 actions)
     are built once per input across the fleet under that name, not once per checkout.
3. **A lane starts with an empty `buck-out/`, and nothing is seeded.** On a host set to `build_tool = "buck2"`,
   `north lane seed` already creates only an empty `buck-out/`, as its own subvolume on btrfs (90df989383).
   - Cost: the lane's first build, 9.1 s and 0.59 GiB, once another lane has built the same inputs.
   - Saved: a 30 to 50 GiB copy, or a cold Cargo build on ext4.
   - Box 8 deletes the Cargo seed.
4. **Intermediate outputs stay in the store; a lane holds only what it runs.** The deferred materialiser keeps every
   intermediate output in the store and writes only the requested targets' outputs into `buck-out/`. A check needs an
   exit status, not a file.
   - The seam's Buck2 side passes `-M none` to every build whose result is only its exit status: the land build
     check, the offload build and box 6's check.
   - `buck2 test` runs tests on the workers, so no test binary is materialised.
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
   - Incremental compilation stays off. The prelude makes an incremental rustc action `local_only` (`rust/build.bzl`,
     `if incremental_enabled: local_only = True`), so it could never be shared, and under this decision it is
     refused. A one-line edit therefore recompiles the edited crate from scratch on a worker. In the cold build,
     the longest first-party compile actions took 3.9 s (north-base check), 5.4 s (north-pace), 6.1 s
     (north-providers) and 8.3 s (north-ops diagnostics), all remote under full load. Box 9 records the
     one-line-edit rebuild before and after.
6. **The worker sandbox lets a lane's daemon live** (the three failures in [Worker sandbox](#worker-sandbox)).
   - The sandbox creates `~/.buck` before it builds a domain.
   - It grants `~/.buck` read, write and remove, as it does `~/.cache`.
   - It prepares `buck-out/` before it builds a domain (north#1116).
7. **A daemon lives as long as its lane.** This Buck2 has no idle exit, but a daemon checks its `buckd.pid` every
   60 s and shuts down when it cannot read it (measured: started 18:33:54 UTC, forced shutdown at 18:34:54 UTC).
   - Before removing a lane, `north lane remove`, the lane-free unit and `lane gc` run `buck2 kill` in it, then
     delete `~/.buck/buckd/<lane path>/`, so a daemon whose kill was missed stops within 60 s.
   - `north lane gc`, which today drops a scratch Cargo target with no build in its idle window (3 h by default,
     `crates/north-lanes/src/lanes/gc/scratch.rs`), kills the daemon of a lane idle that long instead.
   - A killed daemon re-analyses the graph on its next command, as a fresh lane does, in about the 9.1 s measured
     above.
8. **Land-offload slots and lander lanes are fresh checkouts.** A slot is a checkout with an empty `buck-out/`.
   - The warm step (`north capacity offload warm`) and the slot's target go.
   - A slot keeps its daemon between checks (no-op 1.6 to 2.1 s), and the daemon dies with the slot.
   - Saved: gex45's 66 GiB of slot targets today. Each slot holds about 21 MiB under decision 4, so the
     `land_offload` budgets can go in box 8.
9. **The laptop is a client and runs nothing itself.** Through the system configuration it gets the build hosts'
   `/etc/buckconfig.d/remote-execution` and a place on the private mesh, both in the private host configuration.
   - With that file, decision 5 makes every action remote, so a local compile is refused rather than run.
   - It materialises only what its user runs and what the editor reads: `buck2 run` targets and rust-project's outputs.
     A `-M none` `//...` downloads 11 MiB.
   - Its daemon costs the same 0.5 to 0.9 GiB as a lane's.
   - Box 6's second check is `buck2 log what-ran --skip-cache-hits --skip-remote-executions | rg -c rustc` printing
     nothing on the laptop.

### What box 6 builds, in order

1. `toolchains//platforms:exec`: `local_enabled = not remote`, with no hybrid (decision 5).
2. The worker sandbox's `~/.buck` and `buck-out/` grants (decision 6).
3. Lane removal and `lane gc` kill the lane's daemon and delete its daemon dir (decision 7).
4. The seam's Buck2 side: no `--isolation-dir` anywhere, and `-M none` on check builds (decisions 2 and 4).
5. Land-offload slots without warm or target (decision 8).
6. The laptop's buckconfig file and mesh membership, in the private host configuration (decision 9). Box 6's
   laptop check runs once these are live.

Box 6 needs nothing from box 5.

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
  root with every file at its repository path, so `include_str!("../../../goals.toml")` resolves unchanged.
- Build scripts: the root crate's `build.rs` is the only one; it rewrites three committed registries
  (`src/cli/reference.txt`, `src/cli/verbs.rs`, `structure.toml`) from their per-unit trees and emits nothing rustc
  reads, so its targets compile the committed files and the generator says so when it runs. The lander renders the
  same files before a lane lands, so the committed copy is the one Cargo's script would write.
- Refused shapes: a `harness = false` target and `required-features`, rather than emit a target that differs from
  Cargo's.

[reindeer]: https://github.com/facebookincubator/reindeer
[prelude-cargo-package]: https://github.com/facebook/buck2-prelude/blob/main/rust/cargo_package.bzl
[prelude-build]: https://github.com/facebook/buck2-prelude/blob/main/rust/build.bzl
[cargo-metadata]: https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
