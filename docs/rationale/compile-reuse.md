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

## Worker sandbox

A Buck2 daemon outlives the command that starts it and keeps that command's Landlock domain. The worker sandbox
grants Execute entry by entry at a lane's root, around `target/` (crates/north-runtime/src/worker_sandbox/executable.rs),
so a `buck-out/` created after the daemon's domain was built is never executable for that daemon, and `~/.buck` is
not among the trees a worker command may remove from, so a second daemon start fails to clean its directory. Until
the sandbox treats them as build output and cache, a worker creates `buck-out/` before its first `buck2` command.
