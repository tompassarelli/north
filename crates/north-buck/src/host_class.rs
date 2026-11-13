use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::workspace::{HostCase, Kind, Package, Target, Workspace};

/// The one reviewed list of host cases, `BINARY_ID CASE` a line and `#` a comment. A land check reads it from the landing base, never from the lane, so a case enters the class only by a change to this file that has landed; a tree without it has none.
pub const LIST: &str = "build/host-cases.list";

/// The variable a land check names the commit of main it judges the lane against in.
const LANDING_BASE: &str = "SAFE_PUSH_LAND_BASE";

pub type Member = (String, String);

/// The landing base of the land check this runs in; `None` outside one, where the checked-out tree's list is read.
pub fn landing_base() -> Option<String> {
    std::env::var(LANDING_BASE).ok().filter(|base| !base.is_empty())
}

/// The class as `base`'s list holds it, or the list in the tree at `root` when there is no base.
pub fn reviewed(root: &Path, base: Option<&str>) -> Result<BTreeSet<Member>, String> {
    let text = match base {
        Some(base) => committed(root, base)?,
        None => match std::fs::read_to_string(root.join(LIST)) {
            Ok(text) => text,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(why) => return Err(format!("{LIST}: {why}")),
        },
    };
    parse(&text)
}

fn committed(root: &Path, base: &str) -> Result<String, String> {
    let git = |arguments: &[&str]| Command::new("git").arg("-C").arg(root).args(arguments).output().map_err(|why| format!("{LIST} at {base}: git: {why}"));
    let shown = git(&["cat-file", "-p", &format!("{base}:{LIST}")])?;
    if shown.status.success() {
        return Ok(String::from_utf8_lossy(&shown.stdout).into_owned());
    }
    let tree = git(&["ls-tree", "--name-only", base, "--", LIST])?;
    if tree.status.success() && tree.stdout.is_empty() {
        return Ok(String::new());
    }
    Err(format!("{LIST} at {base}: {}", String::from_utf8_lossy(&shown.stderr).trim()))
}

fn parse(text: &str) -> Result<BTreeSet<Member>, String> {
    let mut listed = BTreeSet::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let (binary, case) = line.split_once(' ').filter(|(_, case)| !case.contains(' ')).ok_or_else(|| format!("{LIST}: {line:?} is not `BINARY_ID CASE`"))?;
        if !listed.insert((binary.to_owned(), case.to_owned())) {
            return Err(format!("{LIST}: {line} is listed twice"));
        }
    }
    Ok(listed)
}

/// The list's member for case `case` of package `package`, held by its integration test `test` or else by its unit tests.
pub fn member(package: &str, test: Option<&str>, case: &str) -> Member {
    (test.map_or_else(|| package.to_owned(), |test| format!("{package}::{test}")), case.to_owned())
}

fn holds(case: &HostCase, target: &Target) -> bool {
    match &case.test {
        None => target.kind == Kind::Lib && target.test,
        Some(test) => target.kind == Kind::Test && target.name == *test,
    }
}

/// The host cases the test rule of `target` leaves to the declared Cargo line; `unittest` is the rule of its own unit tests.
pub fn skipped(package: &Package, target: &Target, unittest: bool) -> Vec<String> {
    let runs_here = |case: &&HostCase| holds(case, target) && unittest == case.test.is_none();
    package.host_cases.iter().filter(runs_here).map(|case| case.name.clone()).collect()
}

/// Refuses a manifest host case that `listed`, the class of the landing base, lacks or that `own`, the lane's own copy of the list, lacks, and a case whose test target the package lacks. A listed case no manifest names yet is one whose list line landed first, and its manifest entry follows in a later lane; a case leaves the list only after its manifest entry, in the same or an earlier change.
pub fn check(workspace: &Workspace, listed: &BTreeSet<Member>, own: &BTreeSet<Member>) -> Result<(), String> {
    for package in &workspace.packages {
        for case in &package.host_cases {
            if !package.targets.iter().any(|target| holds(case, target)) {
                return Err(format!("{}: host case {} names a test target the package does not have", package.name, case.name));
            }
            let (binary, name) = member(&package.name, case.test.as_deref(), &case.name);
            if !listed.contains(&(binary.clone(), name.clone())) {
                return Err(format!("{}: host case {name} of {binary} is not in the reviewed list {LIST} of the landing base; a case enters the class by a change to that list alone, and its manifest entry follows once that change has landed", package.name));
            }
            if !own.contains(&(binary.clone(), name.clone())) {
                return Err(format!("{}: host case {name} of {binary} is in the landing base's {LIST} but not in the lane's own copy; a lane that drops a case from the list takes its manifest entry out in the same or an earlier change", package.name));
            }
        }
    }
    Ok(())
}

/// The class as `base`'s list holds it, or the part of it in `packages`, as one nextest filterset without spaces, so the declared line takes it through an unquoted variable; `None` when no case is in it.
pub fn filter(root: &Path, base: Option<&str>, packages: &[String]) -> Result<Option<String>, String> {
    let chosen = |binary: &String| packages.is_empty() || packages.iter().any(|package| binary == package || binary.strip_prefix(package.as_str()).is_some_and(|rest| rest.starts_with("::")));
    let terms: Vec<String> = reviewed(root, base)?.into_iter().filter(|(binary, _)| chosen(binary)).map(|(binary, case)| format!("(binary_id(={binary})&test(={case}))")).collect();
    Ok((!terms.is_empty()).then(|| terms.join("|")))
}

/// The target `north buck targets` writes in each package whose tests hold host cases, whose one output plans how the invoking host runs them (build/rust_test.bzl).
pub const TARGET: &str = "host-cases";
