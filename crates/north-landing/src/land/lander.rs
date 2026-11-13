use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::candidate::{self, git};
use super::heartbeat::Heartbeat;
use super::jobs::Chores;
use super::queue;
use super::setting;
use super::wake::Wake;

pub const OFFLOAD_OWNER: &str = "north-land";

pub struct Lander {
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub queue: PathBuf,
    pub checker: Vec<String>,
    pub app_token: Vec<String>,
    pub offload_owner: String,
    pub round: std::sync::atomic::AtomicU64,
    pub chores: Chores,
    pub heartbeat: Heartbeat,
    pub wake: Arc<Wake>,
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
}

pub fn offload_owner(queue: &Path) -> String {
    format!("{OFFLOAD_OWNER}-{}", queue.file_name().and_then(|name| name.to_str()).unwrap_or_default())
}

pub use super::after::route::plain;

impl Lander {
    pub fn base_moved(&self, base: &str) -> Result<bool, String> {
        let now = git(&self.worktree, &["ls-remote", "origin", "refs/heads/main"])?;
        Ok(now.split_whitespace().next() != Some(base))
    }

    pub fn new(repo: &Path, checker: Vec<String>) -> Result<Self, String> {
        let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
        let queue = queue::directory(&state_home(), &origin);
        Ok(Self {
            repo: repo.to_owned(),
            worktree: candidate::lander_worktree(repo, "lander")?,
            offload_owner: offload_owner(&queue),
            queue,
            checker,
            app_token: super::credential::mint_program(),
            round: std::sync::atomic::AtomicU64::new(0),
            chores: Chores::default(),
            heartbeat: Heartbeat::default(),
            wake: Arc::default(),
        })
    }

    pub fn detached(&self) -> Self {
        Self {
            repo: self.repo.clone(),
            worktree: self.worktree.clone(),
            queue: self.queue.clone(),
            checker: self.checker.clone(),
            app_token: self.app_token.clone(),
            offload_owner: self.offload_owner.clone(),
            round: std::sync::atomic::AtomicU64::new(self.round.load(std::sync::atomic::Ordering::Relaxed)),
            chores: self.chores,
            heartbeat: Heartbeat::default(),
            wake: Arc::clone(&self.wake),
        }
    }

    pub fn remote_hosts(&self) -> Vec<String> {
        setting::outbox_hosts(&self.repo, "refs/remotes/origin/main")
    }
}
