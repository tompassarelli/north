use std::collections::BTreeSet;
use std::path::Path;

use crate::workspace::{HostCase, Kind, Package, Target, Workspace};

/// The tree's one reviewed list of host cases, `BINARY_ID CASE` a line and `#` a comment, read from the tree under test rather than built in, so a lane is held to its own list; a tree without it has none.
pub(crate) const LIST: &str = "build/host-cases.list";

type Member = (String, String);

pub fn reviewed(root: &Path) -> Result<BTreeSet<Member>, String> {
    let text = match std::fs::read_to_string(root.join(LIST)) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(why) => return Err(format!("{LIST}: {why}")),
    };
    let mut listed = BTreeSet::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let (binary, case) = line.split_once(' ').filter(|(_, case)| !case.contains(' ')).ok_or_else(|| format!("{LIST}: {line:?} is not `BINARY_ID CASE`"))?;
        if !listed.insert((binary.to_owned(), case.to_owned())) {
            return Err(format!("{LIST}: {line} is listed twice"));
        }
    }
    Ok(listed)
}

fn binary_id(package: &Package, case: &HostCase) -> String {
    case.test.as_ref().map_or_else(|| package.name.clone(), |test| format!("{}::{test}", package.name))
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

/// Refuses a manifest host case the reviewed list lacks, a listed case no manifest names, and a case whose test target the package lacks, so the class grows only by a diff to the list.
pub fn check(workspace: &Workspace) -> Result<(), String> {
    let listed = reviewed(&workspace.root)?;
    let mut named = BTreeSet::new();
    for package in &workspace.packages {
        for case in &package.host_cases {
            if !package.targets.iter().any(|target| holds(case, target)) {
                return Err(format!("{}: host case {} names a test target the package does not have", package.name, case.name));
            }
            let member = (binary_id(package, case), case.name.clone());
            if !listed.contains(&member) {
                return Err(format!("{}: host case {} of {} is not in the reviewed list {LIST}; a host case enters the class only through that file", package.name, member.1, member.0));
            }
            named.insert(member);
        }
    }
    listed.difference(&named).next().map_or(Ok(()), |(binary, case)| Err(format!("{LIST}: {binary} {case} is a host case no manifest names; drop it from the list or name it in its package's manifest")))
}

/// The class, or the part of it in `packages`, as one nextest filterset without spaces, so the declared line takes it through an unquoted variable; `None` when no case is in it.
pub fn filter(root: &Path, packages: &[String]) -> Result<Option<String>, String> {
    let chosen = |binary: &String| packages.is_empty() || packages.iter().any(|package| binary == package || binary.strip_prefix(package.as_str()).is_some_and(|rest| rest.starts_with("::")));
    let terms: Vec<String> = reviewed(root)?.into_iter().filter(|(binary, _)| chosen(binary)).map(|(binary, case)| format!("(binary_id(={binary})&test(={case}))")).collect();
    Ok((!terms.is_empty()).then(|| terms.join("|")))
}
