use std::path::{Path, PathBuf};

use north_base::foundation::step::cancelled;
use north_base::platform::git;

pub trait Git {
    fn default_branch(&self) -> String;
    fn symbolic_ref(&self, name: &str) -> Option<String>;
    fn valid_branch_name(&self, branch: &str) -> bool;
    fn has_commit(&self, oid: &str) -> bool;
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool;
    fn executable_hook(&self, name: &str) -> Option<PathBuf>;
    fn setting(&self, remote_ref: &str, key: &str) -> String;
    fn push_url(&self) -> Option<String>;
}

pub struct Live {
    directory: PathBuf,
}

impl Live {
    pub fn new(directory: &Path) -> Self {
        Self { directory: directory.to_path_buf() }
    }

    fn read(&self, arguments: &[&str]) -> Option<String> {
        quiet(&self.directory, arguments)
    }
}

impl Git for Live {
    fn default_branch(&self) -> String {
        default_branch(&self.directory)
    }

    fn symbolic_ref(&self, name: &str) -> Option<String> {
        symbolic_ref(&self.directory, name)
    }

    fn valid_branch_name(&self, branch: &str) -> bool {
        self.read(&["check-ref-format", &format!("refs/heads/{branch}")]).is_some()
    }

    fn has_commit(&self, oid: &str) -> bool {
        self.read(&["cat-file", "-e", &format!("{oid}^{{commit}}")]).is_some()
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        self.read(&["merge-base", "--is-ancestor", ancestor, descendant]).is_some()
    }

    fn executable_hook(&self, name: &str) -> Option<PathBuf> {
        let path = PathBuf::from(self.read(&["rev-parse", "--path-format=absolute", "--git-path", &format!("hooks/{name}")])?);
        std::fs::metadata(&path).is_ok_and(|meta| meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0).then_some(path)
    }

    fn setting(&self, remote_ref: &str, key: &str) -> String {
        git::safe_push_setting(&self.directory, remote_ref, key)
    }

    fn push_url(&self) -> Option<String> {
        self.read(&["remote", "get-url", "--push", "origin"])
    }
}

pub(super) fn quiet(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = git::run(directory, arguments).ok()?;
    cancelled().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn default_branch(repository: &Path) -> String {
    let origin_head = symbolic_ref(repository, "refs/remotes/origin/HEAD").unwrap_or_default();
    let default = origin_head.strip_prefix("refs/remotes/origin/").unwrap_or(&origin_head);
    if default.is_empty() { "main".to_owned() } else { default.to_owned() }
}

pub fn symbolic_ref(repository: &Path, name: &str) -> Option<String> {
    let output = git::run(repository, &["symbolic-ref", "--quiet", name]).ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}
