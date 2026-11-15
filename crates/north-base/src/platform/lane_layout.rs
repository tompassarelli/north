use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub fn exempt(code: &Path, home: &Path, worktree: &Path) -> bool {
    let text = worktree.to_string_lossy();
    let under = |marker: &str| text.split_once(marker).is_some_and(|(_, rest)| !rest.is_empty());
    under("/store-runtime/deployments/")
        || under("/experiments/")
        || worktree.strip_prefix(code.join("resources")).is_ok_and(|rest| !rest.as_os_str().is_empty())
        || owned_state(home).iter().any(|owned| worktree.strip_prefix(owned).is_ok_and(|rest| !rest.as_os_str().is_empty()))
        || fleet_scratch(worktree)
}

/// A fleet session's scratch checkout under the temporary directory (`north-<provider>-<session>`): a command's
/// disposable clone or worktree, not a lane that waits for a move into `worktrees/`.
fn fleet_scratch(worktree: &Path) -> bool {
    worktree
        .strip_prefix(std::env::temp_dir())
        .ok()
        .and_then(|rest| rest.components().next())
        .is_some_and(|first| first.as_os_str().to_string_lossy().starts_with("north-"))
}

fn owned_state(home: &Path) -> Vec<PathBuf> {
    let mut named = vec![home.join(".cache"), home.join(".local/state"), home.join(".local/state/north")];
    named.extend(runtime_dir(home).map(|runtime| runtime.join("north-health")));
    let resolved = named.iter().filter_map(|path| fs::canonicalize(path).ok()).collect::<Vec<_>>();
    named.into_iter().chain(resolved).collect()
}

fn runtime_dir(home: &Path) -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).or_else(|| Some(PathBuf::from(format!("/run/user/{}", fs::metadata(home).ok()?.uid()))))
}
