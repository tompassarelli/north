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

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with_promoted_link() -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let promoted = home.path().join(".local/state/north/current");
        std::fs::create_dir_all(&promoted).unwrap();
        std::fs::write(promoted.join("north"), "").unwrap();
        let link = promoted.join("north");
        (home, link)
    }

    #[test]
    fn an_explicit_binary_wins_over_the_promoted_link_and_the_executable() {
        let (home, _) = home_with_promoted_link();
        assert_eq!(resolve(Some("/opt/north".into()), home.path(), Some("/tmp/self".into())), PathBuf::from("/opt/north"));
    }

    #[test]
    fn the_promoted_link_wins_over_the_executable_when_it_exists() {
        let (home, link) = home_with_promoted_link();
        assert_eq!(resolve(None, home.path(), Some("/tmp/self".into())), link);
    }

    #[test]
    fn a_replaced_executable_loses_its_deleted_suffix() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(resolve(None, home.path(), Some("/runtimes/abc/north (deleted)".into())), PathBuf::from("/runtimes/abc/north"));
        assert_eq!(resolve(None, home.path(), Some("/runtimes/abc/north".into())), PathBuf::from("/runtimes/abc/north"));
    }

    #[test]
    fn no_executable_falls_back_to_the_path_name() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(resolve(None, home.path(), None), PathBuf::from("north"));
    }
}
