use std::fmt::Write as _;
use std::path::Path;

use super::ledger::{Row, Verdict};
use crate::platform::rolling_issue::{Tracker, settle};

const RATIONALE: &str = "docs/rationale/rust-build-times.md";

pub(super) fn title(repo: &str) -> String {
    format!("Split a slow Rust workspace in {repo}: build time near or over its threshold")
}

pub(super) fn split_box(row: &Row<'_>) -> Option<String> {
    let limit = row.threshold?;
    matches!(row.verdict, Verdict::Near | Verdict::Over).then(|| {
        format!(
            "- [ ] Split `{}` along the layers named in {RATIONALE} of {}: leaf-edit `cargo test --no-run` took {:.1}s against a threshold of {limit:.0}s ({}) at {} on {}; critical path {}. Done when: the daily `north code-health build-time` measures it within 75% of the threshold.",
            row.workspace, row.repo, row.times.leaf_test, row.verdict.word(), row.sha, row.host, row.times.critical_path
        )
    })
}

fn body(repo: &str, boxes: &[String]) -> String {
    let mut text = format!(
        "Category: tooling\nSpec: measured\nScope: cross-module\nSurface: rust\nVerify: local-test\n\n\
         The one rolling issue the daily `north code-health build-time --file` keeps for {repo}. It lists each workspace whose leaf-edit test build is near (75%) or over its `[[build_time]]` threshold in `structure.toml`, edits itself after every run and closes itself when every workspace is within. The split each workspace takes is in {RATIONALE}.\n\n## Done when\n"
    );
    for line in boxes {
        let _ = writeln!(text, "{line}");
    }
    text
}

pub(super) fn file(tracker: &mut dyn Tracker, repo: &str, boxes: &[String]) -> Result<(), String> {
    let body = (!boxes.is_empty()).then(|| body(repo, boxes));
    settle(tracker, &title(repo), body.as_deref(), &format!("`north code-health build-time --file` measured every workspace of {repo} within 75% of its threshold; it closed this issue."))
}

pub(super) fn github_repo(repo: &Path) -> Result<String, String> {
    let url = super::git(repo, &["remote", "get-url", "origin"]).ok_or(format!("{} has no origin remote to file into", repo.display()))?;
    let path = url.split_once("github.com").map(|(_, rest)| rest.trim_start_matches([':', '/'])).ok_or(format!("origin {url} is not on GitHub"))?;
    Ok(path.trim_end_matches('/').trim_end_matches(".git").to_owned())
}
