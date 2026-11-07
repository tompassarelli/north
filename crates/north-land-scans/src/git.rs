use std::path::Path;

use crate::platform::git;

pub fn bytes(directory: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    git::run(directory, arguments)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
}

pub fn text(directory: &Path, arguments: &[&str]) -> Option<String> {
    bytes(directory, arguments).map(|stdout| String::from_utf8_lossy(&stdout).into_owned())
}

pub fn merge_base(directory: &Path, base: &str, head: &str) -> String {
    text(directory, &["merge-base", base, head]).map_or_else(
        || base.to_owned(),
        |sha| sha.trim_end_matches('\n').to_owned(),
    )
}

pub fn messages(directory: &Path, base: &str, head: &str) -> String {
    text(
        directory,
        &["log", "--format=%B", &format!("{base}..{head}")],
    )
    .unwrap_or_default()
}
