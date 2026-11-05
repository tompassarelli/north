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
