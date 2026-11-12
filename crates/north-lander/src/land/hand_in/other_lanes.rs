use std::path::{Path, PathBuf};

use super::super::enqueue::{default_branch, git};

pub(super) struct Unlanded {
    pub lane: PathBuf,
    pub commits: usize,
}

/// The lanes of the same name in the project containers beside `lane`'s (`~/code/PROJECT/worktrees/SLUG`) that hold commits not on their repository's main: the other repositories a task that spans two has committed to.
pub(super) fn unlanded(lane: &Path) -> Vec<Unlanded> {
    let Some((slug, container)) = lane.file_name().zip(lane.parent().and_then(Path::parent)) else { return Vec::new() };
    let Some(entries) = container.parent().and_then(|parent| std::fs::read_dir(parent).ok()) else { return Vec::new() };
    let mut found: Vec<Unlanded> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("worktrees").join(slug))
        .filter(|other| other != lane && other.join(".git").exists())
        .filter_map(|other| {
            let range = format!("refs/remotes/origin/{}..HEAD", default_branch(&other));
            let commits = git(&other, &["rev-list", "--count", &range]).ok()?.parse().ok().filter(|commits| *commits > 0)?;
            Some(Unlanded { lane: other, commits })
        })
        .collect();
    found.sort_by(|a, b| a.lane.cmp(&b.lane));
    found
}

pub(super) fn refusal(found: &[Unlanded]) -> Option<String> {
    if found.is_empty() {
        return None;
    }
    let named: Vec<String> = found.iter().map(|other| format!("{} ({} commit(s) not on its main)", other.lane.display(), other.commits)).collect();
    Some(format!("the task also changed {}; each repository a task changes is handed in from its own lane, so hand that one in first with `hand-in`", named.join(", ")))
}
