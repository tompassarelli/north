use std::path::PathBuf;

use super::Context;
use super::removable::{git_writable, projects};

/// Every path a worker command may open for writing or create a file, directory, link or node beneath: what it may remove (its lane, the git state it writes, scratch, caches and build output), the device files a shell writes to, the agent state it writes (state.rs), the `FETCH_HEAD` of each git directory it fetches into, and the worktrees directory of each project it works in, where `north lane new` and `git worktree add` create a lane. The operator's dotfiles and configuration, the local bin directories, the north runtimes, the fleet's session records, the lifecycle ledger, the host's capacity admission state, the client profiles, a project's git hooks and configuration, every main checkout's files and every other project stay unwritable.
pub fn writable(context: &Context, removable: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = removable.to_vec();
    paths.push(PathBuf::from("/dev"));
    paths.extend(super::state::writable(&context.home.join(".local/state"), context.session.as_deref()));
    paths.extend(git_writable(context));
    paths.extend(projects(context).into_iter().map(|project| project.join("worktrees")));
    paths
}
