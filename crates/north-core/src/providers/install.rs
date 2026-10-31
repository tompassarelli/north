use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::fleet::runtime::binary;

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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn the_installed_runtime_wins_over_any_wrapper_on_path() {
        let home = tempfile::tempdir().unwrap();
        let runtime = installed(home.path(), "codex");
        fs::create_dir_all(runtime.parent().unwrap()).unwrap();
        fs::write(&runtime, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve("codex", Some(home.path()), None), runtime);
        assert_eq!(resolve("codex", Some(home.path()), Some(PathBuf::from("/nonexistent/codex"))), binary("codex"));
        assert_eq!(resolve("claude", Some(home.path()), None), binary("claude"), "a provider without an installed runtime falls back to PATH");
    }
}
