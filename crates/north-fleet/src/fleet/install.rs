use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::runtime::binary;

pub fn runtime(name: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let chosen = std::env::var_os(format!("{}_RUNTIME", name.to_uppercase())).map(PathBuf::from);
    resolve(name, home.as_deref(), chosen)
}

fn resolve(name: &str, home: Option<&Path>, chosen: Option<PathBuf>) -> PathBuf {
    let candidate = chosen.or_else(|| home.map(|home| installed(home, name)));
    candidate.filter(|path| executable(path)).unwrap_or_else(|| binary(name))
}

fn installed(home: &Path, name: &str) -> PathBuf {
    home.join(".local/lib").join(name).join("current/bin").join(name)
}

fn executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

