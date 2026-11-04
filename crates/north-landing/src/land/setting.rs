use std::path::Path;

use super::candidate::git;

pub fn landing(repo: &Path, remote_ref: &str, key: &str) -> String {
    let committed = git(repo, &["config", "--blob", &format!("{remote_ref}:.safe-push"), "--get", key]).unwrap_or_default();
    if committed.is_empty() { git(repo, &["config", "--get", key]).unwrap_or_default() } else { committed }
}
