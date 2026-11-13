//! `north runtime tree-build`: builds north from the checkout it runs in and prints where the binary is, with Buck2 on a
//! host whose `buck2_tree_build` is set and a tree whose Buck2 configuration is its base's, else with Cargo.
//! `bin/head-north` calls it to run a tree's own north when no built runtime covers the tree, so the script names no
//! build tool.

use std::process::Stdio;

use super::failure::{self, Failure};

const BINARY: &str = "north";
/// The base of the range a land check checks, which a Buck2 build compares the tree's Buck2 configuration with.
const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
/// What `bin/head-north` exits with when the tree itself does not build, so the check fails.
const BROKEN: u8 = 1;

pub(super) fn run() -> Result<(), Failure> {
    let undecided = |error: std::io::Error| failure::failed(failure::DEFERRED, format!("building north from the tree failed, so no older north runs in its place: {error}"));
    let root = std::env::current_dir().map_err(undecided)?;
    let base = std::env::var(BASE_ENV).ok().filter(|base| !base.is_empty());
    let mut sides = north_build_system::tree_builders(&root, base.as_deref()).into_iter().peekable();
    while let Some(side) = sides.next() {
        let (invocation, binary) = side.tree_binary(&root, BINARY);
        binary.parent().map_or(Ok(()), std::fs::create_dir_all).map_err(undecided)?;
        if side.copies_out() {
            std::fs::remove_file(&binary).or_else(|error| if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }).map_err(undecided)?;
        }
        eprintln!("north runtime tree-build: {}", invocation.words.join(" "));
        let output = invocation.command().stdin(Stdio::null()).output().map_err(undecided)?;
        if output.status.success() {
            println!("{}", binary.canonicalize().map_err(undecided)?.display());
            return Ok(());
        }
        let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        eprint!("{log}");
        if side.broken(output.status.code(), &log) {
            return Err(failure::failed(BROKEN, "north does not compile from this tree, so the check fails"));
        }
        if sides.peek().is_some() {
            eprintln!("north runtime tree-build: the build failed for a reason other than the tree ({}), so the next side builds it", output.status);
        }
    }
    Err(failure::failed(failure::DEFERRED, "building north from the tree failed, so no older north runs in its place"))
}
