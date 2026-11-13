use crate::platform::git as platform_git;
use std::path::Path;

pub(super) fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    platform_git::stdout(repo, args, |output| String::from_utf8_lossy(&output.stderr).into_owned())
        .map(|stdout| String::from_utf8_lossy(&stdout).into_owned())
}
