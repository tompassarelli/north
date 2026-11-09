use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::repo::{git_checked, git_out};

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w.-]+#\d+$").unwrap());

fn key(branch: &str) -> String {
    format!("branch.{branch}.north-issue")
}

pub(super) fn valid(item: &str) -> bool {
    ITEM.is_match(item)
}

pub(super) fn recorded(main: &Path, branch: &str) -> Option<String> {
    if branch.is_empty() {
        return None;
    }
    git_out(main, &["config", "--get", &key(branch)]).map(|item| item.trim().to_string()).filter(|item| valid(item))
}

pub(super) fn record(main: &Path, branch: &str, item: &str) -> Result<(), String> {
    git_checked(main, &["config", &key(branch), item])
}
