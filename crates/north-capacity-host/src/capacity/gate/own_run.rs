//! A fleet session's own `cargo clippy` through the gate, recorded by the clean tree it passed on so the land checks running the same clippy on that tree reuse it (north#902).

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use north_base::foundation::clock::Timestamp;
use north_base::verdict_cache::{Pass, Store};

pub struct OwnRun {
    directory: PathBuf,
    tree: String,
    key: String,
}

impl OwnRun {
    /// The run `cargo ARGUMENTS` would be, when it is a clippy whose verdict a land check can reuse: no build-changing environment, run from a clean worktree's top level.
    pub fn of(program: &str, arguments: &[String]) -> Option<Self> {
        let canonical = (program == "cargo").then(|| north_tree_verdict::canonical(arguments)).flatten()?;
        if std::env::vars_os().any(|(name, _)| name.to_str().is_none_or(north_tree_verdict::changes_build)) {
            return None;
        }
        let directory = std::env::current_dir().ok()?;
        let tree = north_tree_verdict::clean_tree(&directory)?;
        let key = north_tree_verdict::key(&tree, &canonical);
        Some(Self { directory, tree, key })
    }

    /// Records the pass when the run exited 0 and the worktree still holds the tree it started on.
    pub fn settle(self, code: u8) -> u8 {
        if code == 0 && north_tree_verdict::clean_tree(&self.directory).as_deref() == Some(self.tree.as_str()) {
            let _ = Store::here().record(&self.key, Some(&Pass { at: Timestamp::now(), sha: self.tree }));
        }
        code
    }
}

/// Runs `command` to its end so its exit can be recorded, where the gate would otherwise become it.
pub fn wait(command: &mut Command) -> std::io::Result<u8> {
    let status = command.status()?;
    Ok(u8::try_from(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))).unwrap_or(u8::MAX))
}
