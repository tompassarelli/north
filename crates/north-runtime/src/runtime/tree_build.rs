//! `north runtime tree-build`: builds north from the checkout it runs in, with the build system this host runs, and
//! prints where the binary is. `bin/head-north` calls it to run a tree's own north when no built runtime covers the
//! tree, so the script names no build tool.

use std::process::Stdio;

use super::failure::{self, Failure};

const BINARY: &str = "north";
/// What `bin/head-north` exits with when the tree itself does not build, so the check fails.
const BROKEN: u8 = 1;

pub(super) fn run() -> Result<(), Failure> {
    let undecided = |error: std::io::Error| failure::failed(failure::DEFERRED, format!("building north from the tree failed, so no older north runs in its place: {error}"));
    let root = std::env::current_dir().map_err(undecided)?;
    let side = north_build_system::for_tree(&root);
    let (invocation, binary) = side.tree_binary(&root, BINARY);
    binary.parent().map_or(Ok(()), std::fs::create_dir_all).map_err(undecided)?;
    let output = invocation.command().stdin(Stdio::null()).output().map_err(undecided)?;
    if output.status.success() {
        println!("{}", binary.canonicalize().map_err(undecided)?.display());
        return Ok(());
    }
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    eprint!("{log}");
    match side.broken(output.status.code(), &log) {
        true => Err(failure::failed(BROKEN, "north does not compile from this tree, so the check fails")),
        false => Err(failure::failed(failure::DEFERRED, "building north from the tree failed, so no older north runs in its place")),
    }
}
