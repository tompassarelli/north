use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Clone, Default)]
pub(super) struct Trees {
    pub(super) base: String,
    pub(super) head: String,
}

impl Trees {
    pub(super) fn of(root: &Path, base: &str, head: &str) -> Self {
        let tree = |revision: &str| {
            if revision.is_empty() {
                return String::new();
            }
            std::process::Command::new("git")
                .args(["rev-parse", "--verify", "--quiet", &format!("{revision}^{{tree}}")])
                .current_dir(root)
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .unwrap_or_default()
        };
        Self { base: tree(base), head: tree(head) }
    }
}

pub(super) struct BaseTree {
    repo: PathBuf,
    root: tempfile::TempDir,
}

fn git_in(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").args(arguments).current_dir(root).stdin(Stdio::null()).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn state_dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    state.join("safe-push/base-trees")
}

impl BaseTree {
    pub(super) fn detached(repo: &Path, base: &str) -> Result<Self, String> {
        Self::detached_in(repo, base, &state_dir())
    }

    pub(super) fn detached_in(repo: &Path, base: &str, state: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(state).map_err(|error| format!("{}: {error}", state.display()))?;
        let root = tempfile::Builder::new().prefix("base-").tempdir_in(state).map_err(|error| error.to_string())?;
        git_in(repo, &["worktree", "add", "--quiet", "--detach", &root.path().to_string_lossy(), base])?;
        Ok(Self { repo: repo.to_owned(), root })
    }

    pub(super) fn path(&self) -> &Path {
        self.root.path()
    }
}

impl Drop for BaseTree {
    fn drop(&mut self) {
        if let Err(error) = git_in(&self.repo, &["worktree", "remove", "--force", &self.root.path().to_string_lossy()]) {
            super::warn(&format!("safe-push: {error}"));
        }
    }
}

#[cfg(test)]
#[path = "checkout_tests.rs"]
mod tests;
