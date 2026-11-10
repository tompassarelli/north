use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const SOCKET_DIRECTORY: &str = "north-session-hosts";
const SOCKET_ID_BYTES: usize = 8;
const SOCKET_PATH_LIMIT: usize = 107;

pub fn socket_path(state_dir: &Path) -> PathBuf {
    let name = format!("{}.sock", socket_id(state_dir));
    let preferred = socket_directory(runtime_directory()).join(&name);
    if preferred.as_os_str().len() < SOCKET_PATH_LIMIT {
        return preferred;
    }
    let fallback = socket_directory(temporary_directory()).join(&name);
    if fallback.as_os_str().len() < SOCKET_PATH_LIMIT { fallback } else { preferred }
}

fn socket_directory(runtime: PathBuf) -> PathBuf {
    let directory = runtime.join(SOCKET_DIRECTORY);
    let _ = fs::create_dir_all(&directory);
    directory
}

pub fn socket_id(state_dir: &Path) -> String {
    let absolute = if state_dir.is_absolute() { state_dir.to_path_buf() } else { std::env::current_dir().map_or_else(|_| state_dir.to_path_buf(), |cwd| cwd.join(state_dir)) };
    Sha256::digest(absolute.as_os_str().as_bytes()).iter().take(SOCKET_ID_BYTES).map(|byte| format!("{byte:02x}")).collect()
}

fn runtime_directory() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|runtime| !runtime.is_empty())
        .map_or_else(temporary_directory, PathBuf::from)
}

fn temporary_directory() -> PathBuf {
    let directory = std::env::temp_dir().join(format!("north-{}", rustix::process::getuid().as_raw()));
    let _ = fs::DirBuilder::new().recursive(true).mode(0o700).create(&directory);
    directory
}

pub fn pid_path(dir: &Path) -> PathBuf {
    dir.join("host.pid")
}

pub fn scope_path(dir: &Path) -> PathBuf {
    dir.join("host.scope")
}

pub fn pid(dir: &Path) -> Option<u32> {
    fs::read_to_string(pid_path(dir)).ok()?.trim().parse().ok()
}

pub fn scope(dir: &Path) -> Option<String> {
    fs::read_to_string(scope_path(dir)).ok().map(|unit| unit.trim().to_owned()).filter(|unit| !unit.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_state_root_and_session_name_keep_the_socket_path_short() {
        let state = Path::new("/tmp").join("state".repeat(40)).join("session".repeat(40));
        let path = socket_path(&state);
        assert!(path.as_os_str().len() < SOCKET_PATH_LIMIT, "{}", path.display());
        assert_eq!(path.file_name().unwrap().as_encoded_bytes().len(), 2 * SOCKET_ID_BYTES + 5);
        assert_ne!(path, socket_path(&Path::new("/tmp").join("another")));
    }
}
