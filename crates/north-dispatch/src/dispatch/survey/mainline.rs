use super::super::issues::Issue;
use super::super::retired::{self, Mainline, Verbs};
use std::collections::{BTreeMap, BTreeSet};

fn git(checkout: &std::path::Path, args: &[&str]) -> Option<String> {
    let directory = checkout.to_str()?;
    let mut full = vec!["-C", directory];
    full.extend_from_slice(args);
    let output = crate::platform::git::run_within(&full, super::PROBE).filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn checkout(repo: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::Path::new(&home).join("code").join(crate::threads::catalog::checkout(repo)).join("main"))
}

/// The paths the takeable boxes of `issues` name in `repo` that origin/main lacks and a landed commit deleted.
fn gone(repo: &str, issues: &[&Issue], at: f64, notes: &mut Vec<String>) -> BTreeSet<String> {
    let named: BTreeSet<String> = issues.iter().flat_map(|issue| issue.takeable(at)).flat_map(retired::candidates).collect();
    let Some(checkout) = checkout(repo).filter(|_| !named.is_empty()) else { return BTreeSet::new() };
    let Some(tree) = git(&checkout, &["ls-tree", "-r", "--name-only", "origin/main"]) else {
        notes.push(format!("{repo}'s origin/main tree is unread, so a box naming a deleted path is not held"));
        return BTreeSet::new();
    };
    let files: BTreeSet<String> = tree.lines().map(str::to_owned).collect();
    let present = |path: &String| {
        let inside = format!("{path}/");
        files.contains(path) || files.range(inside.clone()..).next().is_some_and(|file| file.starts_with(&inside))
    };
    named.into_iter().filter(|path| !present(path) && git(&checkout, &["log", "origin/main", "--diff-filter=D", "-1", "--format=%H", "--", path]).is_some_and(|deleted| !deleted.trim().is_empty())).collect()
}

/// What origin/main no longer has that the open boxes name: the verbs its CLI reference lists and the paths it deleted.
pub(super) fn read(issues: &[Issue], at: f64, notes: &mut Vec<String>) -> Mainline {
    let reference = checkout("north").and_then(|checkout| git(&checkout, &["show", "origin/main:src/cli/reference.txt"]));
    if reference.is_none() {
        notes.push("north's CLI reference on origin/main is unread, so a box naming a retired verb is not held".into());
    }
    let mut by_repo: BTreeMap<&str, Vec<&Issue>> = BTreeMap::new();
    for issue in issues {
        by_repo.entry(issue.repo.as_str()).or_default().push(issue);
    }
    Mainline { verbs: reference.as_deref().map(Verbs::parse), gone: by_repo.into_iter().map(|(repo, issues)| (repo.to_owned(), gone(repo, &issues, at, notes))).collect() }
}
