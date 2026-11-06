use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const DELETED_SUFFIX: &[u8] = b" (deleted)";

/// The north binary a child process should run: an explicit AGENT_CAPACITY_NORTH or NORTH_BIN, else the promoted runtime link, else this process's own executable.
pub fn north_binary() -> PathBuf {
    let explicit = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    resolve(explicit("AGENT_CAPACITY_NORTH").or_else(|| explicit("NORTH_BIN")), &home, std::env::current_exe().ok())
}

pub fn superseded() -> bool {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::current_exe().is_ok_and(|own| promoted_elsewhere(&home, &without_deleted_suffix(own)))
}

fn promoted_elsewhere(home: &Path, own: &Path) -> bool {
    let state = home.join(".local/state/north");
    let Ok(promoted) = std::fs::canonicalize(state.join("current/north")) else {
        return false;
    };
    let runtimes = state.join("runtimes");
    let within = own.starts_with(&runtimes) || std::fs::canonicalize(&runtimes).is_ok_and(|runtimes| own.starts_with(runtimes));
    within && !std::fs::canonicalize(own).is_ok_and(|own| own == promoted)
}

fn resolve(explicit: Option<PathBuf>, home: &Path, executable: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    let promoted = home.join(".local/state/north/current/north");
    if promoted.exists() {
        return promoted;
    }
    executable.map_or_else(|| PathBuf::from("north"), without_deleted_suffix)
}

/// Linux reports /proc/self/exe with a " (deleted)" suffix once the running binary has been replaced on disk.
fn without_deleted_suffix(path: PathBuf) -> PathBuf {
    match path.as_os_str().as_bytes().strip_suffix(DELETED_SUFFIX) {
        Some(stripped) => PathBuf::from(OsString::from_vec(stripped.to_vec())),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::promoted_elsewhere;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn a_runtime_binary_is_superseded_once_current_names_another_runtime_and_a_checkout_build_never_is() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".local/state/north");
        for sha in ["old", "new"] {
            fs::create_dir_all(state.join("runtimes").join(sha)).unwrap();
            fs::write(state.join("runtimes").join(sha).join("north"), sha).unwrap();
        }
        let old = state.join("runtimes/old/north");
        let checkout = home.path().join("code/north/target/debug/north");
        fs::create_dir_all(checkout.parent().unwrap()).unwrap();
        fs::write(&checkout, "dev").unwrap();
        assert!(!promoted_elsewhere(home.path(), &old), "no current link yet");
        symlink("runtimes/old", state.join("current")).unwrap();
        assert!(!promoted_elsewhere(home.path(), &old), "current names this runtime");
        fs::remove_file(state.join("current")).unwrap();
        symlink("runtimes/new", state.join("current")).unwrap();
        assert!(promoted_elsewhere(home.path(), &old), "current moved to another runtime");
        assert!(!promoted_elsewhere(home.path(), &checkout), "a checkout build is never superseded");
        fs::remove_file(&old).unwrap();
        assert!(promoted_elsewhere(home.path(), &old), "a runtime removed from disk is superseded");
        let data = home.path().join("code/north-data");
        fs::create_dir_all(data.join("runtimes/new")).unwrap();
        fs::write(data.join("runtimes/new/north"), "new").unwrap();
        fs::create_dir_all(data.join("runtimes/newer")).unwrap();
        fs::write(data.join("runtimes/newer/north"), "newer").unwrap();
        fs::remove_dir_all(state.join("runtimes")).unwrap();
        symlink(data.join("runtimes"), state.join("runtimes")).unwrap();
        let running = data.join("runtimes/new/north");
        assert!(!promoted_elsewhere(home.path(), &running), "a runtime read through the link to its data directory is current");
        fs::remove_file(state.join("current")).unwrap();
        symlink("runtimes/newer", state.join("current")).unwrap();
        assert!(promoted_elsewhere(home.path(), &running), "the kernel names a running binary by its resolved path, which lies under the linked runtimes directory; north#677");
    }
}
