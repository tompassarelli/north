use std::path::{Path, PathBuf};

use super::super::enqueue::{default_branch, git};
use super::{Request, lane_changes, literal, proposed_message};

pub(super) fn chosen(root: &Path, request: &Request, paths: &[PathBuf], writes_files: bool) -> Result<String, String> {
    if !request.message.trim().is_empty() {
        return Ok(request.message.to_owned());
    }
    match (head(root, paths, writes_files), request.check) {
        (Some(carried), _) => Ok(carried),
        (None, Some(_)) => proposed_message(root).ok_or_else(|| "the message is empty and the brief proposed none".to_owned()),
        (None, None) => Err("the message is empty".to_owned()),
    }
}

fn head(root: &Path, paths: &[PathBuf], writes_files: bool) -> Option<String> {
    let named: Vec<&str> = paths.iter().filter_map(|path| path.to_str()).collect();
    let unchanged = !writes_files && literal(root, &[&["status", "--porcelain", "--untracked-files=all", "--"][..], &named].concat()).ok()?.is_empty();
    let ahead = !lane_changes(root, &default_branch(root)).ok()?.is_empty();
    let message = git(root, &["log", "-1", "--format=%B", "HEAD"]).ok()?;
    (unchanged && ahead && !message.trim().is_empty()).then_some(message)
}
