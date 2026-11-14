use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use super::Selected;

/// The selected crates, or for a workspace selection every member in `packages`, that do not move and name no test, each
/// said in one line.
pub(super) fn case_free(root: &Path, base: &str, selected: &Selected, moved: &[String], packages: &BTreeMap<String, String>) -> BTreeSet<String> {
    let candidates: Vec<&String> = match selected {
        Selected::Packages(selected) => selected.iter().collect(),
        Selected::Workspace => packages.keys().collect(),
    };
    let left = candidates.into_iter().filter(|name| !moved.contains(name)).filter_map(|name| Some((name, packages.get(name)?.as_str()))).collect();
    let free = untested(root, base, left);
    for name in &free {
        eprintln!("north land crate-tests: {name} names no test at the base or in this tree, so the declared line has no case of it to run and leaves it out");
    }
    free
}

/// The crates of `left`, each with its Buck2 package, whose Rust sources under that package name no test, by path or
/// text in any case, both at `base` and in the tree at `root`, so a lane that deletes a crate's every test still leaves
/// it to the declared line, which fails when it selects none. The root package is never one, and none is when git
/// cannot tell.
fn untested(root: &Path, base: &str, left: Vec<(&String, &str)>) -> BTreeSet<String> {
    let dirs: Vec<(&String, String)> = left.into_iter().filter(|(_, package)| *package != "//:").map(|(name, package)| (name, format!("{}/", package.trim_start_matches("//").trim_end_matches(':')))).collect();
    if dirs.is_empty() {
        return BTreeSet::new();
    }
    let globs: Vec<String> = dirs.iter().map(|(_, dir)| format!(":(glob){dir}**/*.rs")).collect();
    let prefixes: Vec<String> = dirs.iter().map(|(_, dir)| dir.clone()).collect();
    let lines = |arguments: &[&str]| -> Option<Vec<String>> {
        let specs = if arguments[0] == "ls-tree" { &prefixes } else { &globs };
        let output = Command::new("git").arg("-C").arg(root).args(arguments).arg("--").args(specs).stderr(Stdio::null()).output().ok()?;
        let answered = output.status.success() || (arguments[0] == "grep" && output.status.code() == Some(1));
        answered.then(|| String::from_utf8_lossy(&output.stdout).lines().map(|line| line.strip_prefix(&format!("{base}:")).unwrap_or(line).to_owned()).collect())
    };
    let (Some(at_base), Some(in_tree), Some(mentioned_at_base), Some(mentioned)) = (lines(&["ls-tree", "-r", "--name-only", base]), lines(&["ls-files"]), lines(&["grep", "-l", "-i", "-e", "test", base]), lines(&["grep", "-l", "-i", "-e", "test"])) else { return BTreeSet::new() };
    let named: Vec<String> = at_base.into_iter().chain(in_tree).filter(|path| path.ends_with(".rs") && path.to_ascii_lowercase().contains("test")).chain(mentioned_at_base).chain(mentioned).collect();
    dirs.into_iter().filter(|(_, dir)| !named.iter().any(|path| path.starts_with(dir.as_str()))).map(|(name, _)| name.clone()).collect()
}

/// Whether the selected crate `name` stays on the declared line once Buck2 passed `passed`: Buck2 did not pass it and it is not
/// one of `case_free`.
pub(super) fn stays(case_free: &BTreeSet<String>, name: &str, passed: &BTreeSet<String>) -> bool {
    !passed.contains(name) && !case_free.contains(name)
}
