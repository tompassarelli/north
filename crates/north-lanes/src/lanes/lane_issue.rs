use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::repo::git_out;

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w.-]+#\d+$").unwrap());

const RECORDS: &str = "north/lane-issues";
const SESSIONS: &str = "north/lane-sessions";

fn key(branch: &str) -> String {
    format!("branch.{branch}.north-issue")
}

pub(super) fn common_dir(main: &Path) -> Option<PathBuf> {
    git_out(main, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).map(PathBuf::from)
}

fn path(main: &Path, branch: &str) -> Option<PathBuf> {
    path_in(main, RECORDS, branch)
}

fn path_in(main: &Path, records: &str, branch: &str) -> Option<PathBuf> {
    if branch.is_empty() {
        return None;
    }
    Some(common_dir(main)?.join(records).join(branch))
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
    write(path(main, branch), item)
}

/// The fleet session this process runs in as the one that made the lane, which the run ledger reads to name the lane its run works in (north#1339); nothing outside a fleet session.
pub(super) fn record_session(main: &Path, branch: &str) -> Result<(), String> {
    match std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.is_empty()) {
        Some(session) => write(path_in(main, SESSIONS, branch), &session),
        None => Ok(()),
    }
}

fn write(path: Option<PathBuf>, text: &str) -> Result<(), String> {
    let path = path.ok_or("cannot resolve the repository's git directory")?;
    let parent = path.parent().ok_or("the lane issue record has no directory")?;
    std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    std::fs::write(&temporary, format!("{text}\n")).map_err(|error| format!("{}: {error}", path.display()))?;
    std::fs::rename(&temporary, &path).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn forget(main: &Path, branch: &str) {
    for path in [path(main, branch), path_in(main, SESSIONS, branch)].into_iter().flatten() {
        let _ = std::fs::remove_file(path);
    }
}

