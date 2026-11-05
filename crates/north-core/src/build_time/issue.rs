use std::path::Path;

use super::ledger::{Row, Verdict};
use crate::platform::rolling_issue::{Tracker, section_box};

pub(super) const REPO: &str = "tompassarelli/north";
pub(super) const SPLIT_ISSUE: u64 = 259;
pub(super) const SECTION: &str = "## Split north-core";

pub(super) struct SplitBox {
    key: String,
    line: String,
    open: bool,
}

pub(super) fn split_box(row: &Row<'_>, leaf: &str) -> Option<SplitBox> {
    let name = if row.workspace == "." { row.repo.to_owned() } else { format!("{}/{}", row.repo, row.workspace) };
    let key = format!("Leaf-edit cargo test --no-run in {name} is ");
    let seconds = row.times.leaf_test;
    let date = row.recorded_at.get(..10).unwrap_or(row.recorded_at);
    let at = format!("measured {date} on {} at {}", row.host, row.sha);
    let cap = row.threshold;
    let (open, line) = match row.verdict {
        Verdict::Over => (true, format!("- [ ] {key}{seconds:.1} s, over the {cap} s cap: split the crate holding {leaf} ({at})")),
        Verdict::Near => (true, format!("- [ ] {key}{seconds:.1} s, near the {cap} s cap: split the crate holding {leaf} ({at})")),
        Verdict::Within => (false, format!("- [x] {key}{seconds:.1} s, within the {cap} s cap ({at})")),
        Verdict::Contended => return None,
    };
    Some(SplitBox { key, line, open })
}

pub(super) fn edited(body: &str, boxes: &[SplitBox]) -> String {
    boxes.iter().fold(body.to_owned(), |body, split| section_box(&body, SECTION, &split.key, &split.line, split.open))
}

pub(super) fn file(tracker: &mut dyn Tracker, boxes: &[SplitBox]) -> Result<(), String> {
    let body = tracker.body(SPLIT_ISSUE)?;
    let next = edited(&body, boxes);
    if next.trim() != body.trim() {
        tracker.edit(SPLIT_ISSUE, &next)?;
    }
    Ok(())
}

pub(super) fn github_repo(repo: &Path) -> Result<String, String> {
    let url = super::git(repo, &["remote", "get-url", "origin"]).ok_or(format!("{} has no origin remote to file into", repo.display()))?;
    let path = url.split_once("github.com").map(|(_, rest)| rest.trim_start_matches([':', '/'])).ok_or(format!("origin {url} is not on GitHub"))?;
    Ok(path.trim_end_matches('/').trim_end_matches(".git").to_owned())
}
