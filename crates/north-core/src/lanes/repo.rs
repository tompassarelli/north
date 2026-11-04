use std::path::Path;

use crate::platform::git;

pub(super) fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    git::run(dir, args)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_string())
}

pub(super) fn git_ok(dir: &Path, args: &[&str]) -> bool {
    git::run(dir, args).is_ok_and(|output| output.status.success())
}

pub(super) fn git_checked(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = git::run(dir, args).map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim_end().to_string())
}

pub(super) fn find_container(start: &Path) -> Option<std::path::PathBuf> {
    start
        .ancestors()
        .filter(|dir| *dir != Path::new("/"))
        .find(|dir| dir.join("main/.git").exists())
        .map(Path::to_path_buf)
}

pub(super) fn is_lander_tree(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name == "lander" || name.starts_with("lander-"))
}
