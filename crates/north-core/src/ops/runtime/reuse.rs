use super::layout::{self, Layout, PROGRAMS, install};
use crate::platform::process;
use std::fs;
use std::path::Path;

const ROOT_FILES: [&str; 8] = ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "rust-toolchain", "flake.nix", "flake.lock", "bin/cargo"];
const ROOT_DIRS: [&str; 4] = ["crates/", "src/", ".cargo/", "harness/claude/agents/"];
const EMBEDDED: [&str; 7] = [
    "harness/defaults.toml",
    "harness/model-catalog.toml",
    "harness/model-watch-sources.toml",
    "agent-machinery/file-skills.toml",
    "north-data/private-checks.toml",
    "north-data/expectations.toml",
    "bin/threads",
];

pub(super) fn rust_input(path: &str) -> bool {
    ROOT_FILES.contains(&path) || ROOT_DIRS.iter().any(|dir| path.starts_with(dir)) || EMBEDDED.contains(&path) || path == ".gitattributes" || path.ends_with("/.gitattributes")
}

pub(super) fn reuse(layout: &Layout, checkout: &Path, target: &str, runtime: &Path, own_build_only: bool) -> bool {
    let Some(live) = fs::read_link(layout.current()).ok().and_then(|link| layout::linked_sha(&link).map(str::to_owned)) else {
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
    println!("north runtime: {} changes no Rust build input since {}; reusing its binaries without a build", layout::short(target), layout::short(&live));
    true
}

fn unchanged(checkout: &Path, live: &str, target: &str) -> bool {
    let args = ["diff", "--name-only", "--no-renames", "--no-ext-diff", "-z", live, target];
    let Ok(output) = process::run("git", Some(checkout), &args) else {
        return false;
    };
    output.status.success() && output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()).all(|path| std::str::from_utf8(path).is_ok_and(|path| !rust_input(path)))
}

fn install_programs(source: &Path, runtime: &Path) -> std::io::Result<()> {
    fs::create_dir_all(runtime)?;
    PROGRAMS.iter().try_for_each(|program| install(&source.join(program), &runtime.join(program)))
}

#[cfg(test)]
mod tests;
