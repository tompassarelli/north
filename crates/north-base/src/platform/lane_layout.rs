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

#[cfg(test)]
mod tests {
    use super::exempt;

    #[test]
    fn a_session_scratch_checkout_under_the_temp_directory_is_not_a_stray() {
        let scratch = tempfile::tempdir().unwrap();
        let (home, code) = (scratch.path().join("home"), scratch.path().join("home/code"));
        let session = std::env::temp_dir().join("north-opencode-n1082/tmp/verify-main");
        assert!(exempt(&code, &home, &session));
        assert!(!exempt(&code, &home, &std::env::temp_dir().join("elsewhere/verify-main")));
    }

    #[test]
    fn a_lander_worktree_under_the_state_symlink_target_is_owned_state_not_a_stray() {
        let scratch = tempfile::tempdir().unwrap();
        let (home, code) = (scratch.path().join("home"), scratch.path().join("home/code"));
        std::fs::create_dir_all(code.join("north-data/land-queue/q/post-land-4c43bc5c465d")).unwrap();
        std::fs::create_dir_all(home.join(".local/state")).unwrap();
        std::os::unix::fs::symlink(code.join("north-data"), home.join(".local/state/north")).unwrap();
        assert!(exempt(&code, &home, &code.join("north-data/land-queue/q/post-land-4c43bc5c465d")));
        assert!(!exempt(&code, &home, &code.join("north/elsewhere")));
    }
}
