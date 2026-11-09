use super::failure::{Failure, failed};
use super::layout::{self, Layout, PROGRAMS, install};
use crate::platform::process;
use north_base::build_inputs::rust_build_input;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn live(layout: &Layout) -> Option<String> {
    fs::read_link(layout.current()).ok().and_then(|link| layout::linked_sha(&link).map(str::to_owned))
}

const TOOLCHAIN: &str = "rust-toolchain.toml";
const LOCK: &str = "Cargo.lock";
const LOCKED: [&str; 4] = ["name", "version", "source", "checksum"];

pub(super) fn covers(layout: &Layout, checkout: &Path, head: &str, code: &[String]) -> Result<PathBuf, Failure> {
    let live = live(layout).ok_or_else(|| failed(1, format!("no live runtime at {}", layout.current().display())))?;
    let north = layout.runtime(&live).join("north");
    if !north.is_file() {
        return Err(failed(1, format!("{} is missing", north.display())));
    }
    let lock_moved = std::cell::OnceCell::new();
    let input = |path: &str| match path {
        _ if code.is_empty() => rust_build_input(path),
        LOCK => *lock_moved.get_or_init(|| !keeps_every_locked_package(checkout, &live, head)),
        _ => path == TOOLCHAIN || code.iter().any(|prefix| path.starts_with(prefix.as_str())),
    };
    if !unchanged_by(checkout, &live, head, input) {
        let what = if code.is_empty() { "a Rust build input".to_owned() } else { format!("{}, a package Cargo.lock held or {TOOLCHAIN}", code.join(", ")) };
        return Err(failed(1, format!("{head} changes {what} since the live runtime {}", layout::short(&live))));
    }
    Ok(north)
}

pub(super) fn reuse(layout: &Layout, checkout: &Path, target: &str, runtime: &Path, own_build_only: bool) -> bool {
    let Some(live) = live(layout) else {
        return false;
    };
    let source = layout.runtime(&live);
    if live == target || !layout::usable(&source, own_build_only) || !unchanged(checkout, &live, target) {
        return false;
    }
    if install_programs(&source, runtime).is_err() {
        return false;
    }
    if source.join(layout::BUILT_HERE).is_file() && fs::write(runtime.join(layout::BUILT_HERE), layout::short(target)).is_err() {
        return false;
    }
    if layout::stamp(runtime, target).is_err() {
        return false;
    }
    println!("north runtime: {} changes no Rust build input since {}; reusing its binaries without a build", layout::short(target), layout::short(&live));
    true
}

fn unchanged(checkout: &Path, live: &str, target: &str) -> bool {
    unchanged_by(checkout, live, target, rust_build_input)
}

fn unchanged_by(checkout: &Path, live: &str, target: &str, input: impl Fn(&str) -> bool) -> bool {
    let args = ["diff", "--name-only", "--no-renames", "--no-ext-diff", "-z", live, target];
    let Ok(output) = process::run("git", Some(checkout), &args) else {
        return false;
    };
    output.status.success() && output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()).all(|path| std::str::from_utf8(path).is_ok_and(|path| !input(path)))
}

fn keeps_every_locked_package(checkout: &Path, live: &str, head: &str) -> bool {
    let packages = |rev: &str| -> Option<BTreeSet<String>> {
        let output = process::run("git", Some(checkout), &["show", &format!("{rev}:{LOCK}")]).ok().filter(|output| output.status.success())?;
        let lock: toml_edit::DocumentMut = String::from_utf8(output.stdout).ok()?.parse().ok()?;
        Some(lock.get("package")?.as_array_of_tables()?.iter().map(|package| LOCKED.map(|key| package.get(key).and_then(|value| value.as_str()).unwrap_or_default()).join(" ")).collect())
    };
    matches!((packages(live), packages(head)), (Some(held), Some(locked)) if held.is_subset(&locked))
}

fn install_programs(source: &Path, runtime: &Path) -> std::io::Result<()> {
    fs::create_dir_all(runtime)?;
    PROGRAMS.iter().try_for_each(|program| install(&source.join(program), &runtime.join(program)))
}

#[cfg(test)]
mod tests;
