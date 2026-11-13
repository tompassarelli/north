use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::repo::git_out;

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w.-]+#\d+$").unwrap());

const RECORDS: &str = "north/lane-issues";

fn key(branch: &str) -> String {
    format!("branch.{branch}.north-issue")
}

pub(super) fn common_dir(main: &Path) -> Option<PathBuf> {
    git_out(main, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).map(PathBuf::from)
}

fn path(main: &Path, branch: &str) -> Option<PathBuf> {
    if branch.is_empty() {
        return None;
    }
    Some(common_dir(main)?.join(RECORDS).join(branch))
}

pub(super) fn valid(item: &str) -> bool {
    ITEM.is_match(item)
}

pub(super) fn recorded(main: &Path, branch: &str) -> Option<String> {
    if let Some(item) = path(main, branch).and_then(|path| std::fs::read_to_string(path).ok()).map(|text| text.trim().to_owned()).filter(|item| valid(item)) {
        return Some(item);
    }
    if branch.is_empty() {
        return None;
    }
    git_out(main, &["config", "--get", &key(branch)]).map(|item| item.trim().to_string()).filter(|item| valid(item))
}

pub(super) fn record(main: &Path, branch: &str, item: &str) -> Result<(), String> {
    let path = path(main, branch).ok_or("cannot resolve the repository's git directory")?;
    let parent = path.parent().ok_or("the lane issue record has no directory")?;
    std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    std::fs::write(&temporary, format!("{item}\n")).map_err(|error| format!("{}: {error}", path.display()))?;
    std::fs::rename(&temporary, &path).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn forget(main: &Path, branch: &str) {
    if let Some(path) = path(main, branch) {
        let _ = std::fs::remove_file(path);
    }
}
