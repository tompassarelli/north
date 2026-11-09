use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use super::exec::git;

pub(super) struct MainContent {
    patches: HashSet<String>,
    trees: HashSet<String>,
}

fn patch_ids(repo: &Path, range: &[&str]) -> Option<Vec<String>> {
    let mut log = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repo)
        .args(["log", "--no-merges", "--no-ext-diff", "--no-color", "-p", "--format=commit %H"])
        .args(range)
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut ids = Command::new("git").args(["patch-id", "--stable"]).stdin(log.stdout.take()?).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut text = String::new();
    ids.stdout.take()?.read_to_string(&mut text).ok()?;
    let (logged, identified) = (log.wait().ok()?, ids.wait().ok()?);
    (logged.success() && identified.success()).then(|| text.lines().filter_map(|line| line.split_whitespace().next()).map(str::to_owned).collect())
}

impl MainContent {
    pub(super) fn read(repo: &Path, base: &str) -> Self {
        let trees = git(repo, &["log", "--format=%T", base]);
        Self {
            patches: patch_ids(repo, &[base]).unwrap_or_default().into_iter().collect(),
            trees: if trees.ok { trees.out.lines().map(str::to_owned).collect() } else { HashSet::new() },
        }
    }

    pub(super) fn holds(&self, repo: &Path, base: &str, tip: &str) -> Option<&'static str> {
        let tree = git(repo, &["rev-parse", &format!("{tip}^{{tree}}")]);
        if tree.ok && self.trees.contains(&tree.out) {
            return Some("tree on main");
        }
        let lane = patch_ids(repo, &[tip, &format!("^{base}")])?;
        (!lane.is_empty() && lane.iter().all(|patch| self.patches.contains(patch))).then_some("patches on main")
    }
}
