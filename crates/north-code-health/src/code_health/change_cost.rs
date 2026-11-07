use super::history::{BULK_COMMIT_FILES, WINDOW_COMMITS};
use super::snapshot::git;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;

static REVERTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)(?:reverts commit|^(?:sheriff-)?reverts:)\s+([0-9a-f]{7,40})").unwrap());

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(super) struct ChangeCost {
    pub(super) feature_commits: usize,
    pub(super) median_files: f64,
    pub(super) reworked: usize,
    pub(super) rework_rate: f64,
}

struct Commit {
    sha: String,
    reverts: Vec<String>,
    revert: bool,
    hand_files: usize,
    files: usize,
}

fn commits(repo: &Path, range: &str, hand: &dyn Fn(&str) -> bool) -> Result<Vec<Commit>, String> {
    let out = git(repo, &["log", &format!("-n{WINDOW_COMMITS}"), "--no-renames", "--no-merges", "--format=%x1e%H%x1f%s%x1f%b%x1f", "--name-only", range])?;
    Ok(out
        .split('\x1e')
        .skip(1)
        .filter_map(|block| {
            let mut fields = block.split('\x1f');
            let (sha, subject, body, names) = (fields.next()?, fields.next()?, fields.next()?, fields.next().unwrap_or_default());
            let paths: Vec<&str> = names.lines().filter(|line| !line.is_empty()).collect();
            Some(Commit {
                sha: sha.trim().to_owned(),
                reverts: REVERTS.captures_iter(body).map(|found| found[1].to_owned()).collect(),
                revert: subject.starts_with("Revert"),
                hand_files: paths.iter().filter(|path| hand(path)).count(),
                files: paths.len(),
            })
        })
        .collect())
}

pub(super) fn change_cost(repo: &Path, range: &str, hand: &dyn Fn(&str) -> bool) -> Result<ChangeCost, String> {
    let all = commits(repo, range, hand)?;
    let reverted: Vec<&str> = all.iter().flat_map(|commit| commit.reverts.iter().map(String::as_str)).collect();
    let features: Vec<&Commit> = all.iter().filter(|commit| !commit.revert && commit.hand_files > 0 && commit.files <= BULK_COMMIT_FILES).collect();
    let mut sizes: Vec<usize> = features.iter().map(|commit| commit.hand_files).collect();
    sizes.sort_unstable();
    let median_files = match sizes.len() {
        0 => 0.0,
        n if n % 2 == 1 => sizes[n / 2] as f64,
        n => (sizes[n / 2 - 1] + sizes[n / 2]) as f64 / 2.0,
    };
    let reworked = features.iter().filter(|commit| reverted.iter().any(|sha| commit.sha.starts_with(sha))).count();
    let rework_rate = if features.is_empty() { 0.0 } else { reworked as f64 / features.len() as f64 };
    Ok(ChangeCost { feature_commits: features.len(), median_files, reworked, rework_rate })
}
