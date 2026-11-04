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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(root: &Path, at: i64, arguments: &[&str]) -> String {
        let stamp = format!("@{at} +0000");
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .env("GIT_COMMITTER_DATE", &stamp)
            .env("GIT_AUTHOR_DATE", &stamp)
            .env("GIT_EDITOR", "true")
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn repository() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        run(root, 1_000, &["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "Lane"), ("user.email", "lane@example.invalid"), ("commit.gpgsign", "false"), ("core.logAllRefUpdates", "true")] {
            run(root, 1_000, &["config", key, value]);
        }
        run(root, 1_000, &["commit", "-q", "--allow-empty", "-m", "base"]);
        directory
    }

    #[test]
    fn the_lane_began_at_its_first_commit_not_at_its_branch_or_its_amend() {
        let directory = repository();
        let root = directory.path();
        let base = run(root, 1_000, &["rev-parse", "HEAD"]);
        run(root, 2_000, &["checkout", "-q", "-b", "lane"]);
        run(root, 3_000, &["commit", "-q", "--allow-empty", "-m", "first"]);
        run(root, 4_000, &["commit", "-q", "--allow-empty", "-m", "second"]);
        run(root, 9_000, &["commit", "-q", "--allow-empty", "--amend", "-m", "second again"]);
        run(root, 9_500, &["rebase", "-q", "--force-rebase", "main"]);
        assert_eq!(seconds(root, "lane", &base), Some(3_000));
    }

    #[test]
    fn work_after_the_lane_was_published_starts_a_new_first_commit() {
        let directory = repository();
        let root = directory.path();
        run(root, 2_000, &["checkout", "-q", "-b", "lane"]);
        run(root, 3_000, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let landed = run(root, 3_000, &["rev-parse", "HEAD"]);
        run(root, 5_000, &["reset", "-q", "--hard", &landed]);
        run(root, 6_000, &["commit", "-q", "--allow-empty", "-m", "next"]);
        assert_eq!(seconds(root, "lane", &landed), Some(6_000));
    }

    #[test]
    fn a_lane_without_a_reflog_falls_back_to_its_earliest_author_date() {
        let directory = repository();
        let root = directory.path();
        let base = run(root, 1_000, &["rev-parse", "HEAD"]);
        run(root, 3_000, &["checkout", "-q", "-b", "lane"]);
        run(root, 3_000, &["commit", "-q", "--allow-empty", "-m", "first"]);
        run(root, 4_000, &["commit", "-q", "--allow-empty", "-m", "second"]);
        assert_eq!(seconds(root, "other-name", &base), Some(3_000));
    }
}
