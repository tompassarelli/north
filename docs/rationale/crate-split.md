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
