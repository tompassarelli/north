use std::path::Path;
use std::process::Command;

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn published(root: &Path, commit: &str, base: &str) -> bool {
    Command::new("git").arg("-C").arg(root).args(["merge-base", "--is-ancestor", commit, base]).output().is_ok_and(|output| output.status.success())
}

fn from_author_dates(root: &Path, base: &str) -> Option<i64> {
    git(root, &["log", "--format=%at", &format!("{base}..HEAD")])?.lines().filter_map(|line| line.parse().ok()).min()
}

pub fn seconds(root: &Path, branch: &str, base: &str) -> Option<i64> {
    let log = git(root, &["reflog", "show", "--date=unix", "--format=%H %gd", &format!("refs/heads/{branch}")]).unwrap_or_default();
    let mut began = None;
    for entry in log.lines() {
        let (commit, selector) = entry.split_once(' ')?;
        if published(root, commit, base) {
            break;
        }
        began = selector.rsplit_once("@{").and_then(|(_, stamp)| stamp.strip_suffix('}')).and_then(|stamp| stamp.parse().ok()).or(began);
    }
    began.or_else(|| from_author_dates(root, base))
}
