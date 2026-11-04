use std::path::{Path, PathBuf};

use super::candidate::{self, git};
use super::queue;

pub struct Lander {
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub queue: PathBuf,
    pub checker: Vec<String>,
}

pub(super) fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
}

impl Lander {
    pub fn new(repo: &Path, checker: Vec<String>) -> Result<Self, String> {
        let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
        Ok(Self { repo: repo.to_owned(), worktree: candidate::lander_worktree(repo)?, queue: queue::directory(&state_home(), &origin), checker })
    }
}
