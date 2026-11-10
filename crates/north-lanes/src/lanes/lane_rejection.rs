use std::path::{Path, PathBuf};

use super::repo::git_out;

const RECORDS: &str = "north/lane-rejections";

fn path(main: &Path, branch: &str) -> Option<PathBuf> {
    if branch.is_empty() {
        return None;
    }
    let common = git_out(main, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Some(PathBuf::from(common).join(RECORDS).join(branch))
}

pub(super) fn recorded(main: &Path, branch: &str) -> Option<String> {
    path(main, branch).and_then(|path| std::fs::read_to_string(path).ok()).map(|text| text.trim().to_owned()).filter(|verdict| !verdict.is_empty())
}

pub(super) fn record(main: &Path, branch: &str, verdict: &str) -> Result<(), String> {
    let path = path(main, branch).ok_or("a detached lane has no branch to record its verdict on")?;
    let parent = path.parent().ok_or("the lane rejection record has no directory")?;
    std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    std::fs::write(&path, format!("{verdict}\n")).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn forget(main: &Path, branch: &str) {
    if let Some(path) = path(main, branch) {
        let _ = std::fs::remove_file(path);
    }
}
