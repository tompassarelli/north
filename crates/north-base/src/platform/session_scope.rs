use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn bind(command: &mut Command) {
    if let Ok(unit) = std::env::var("NORTH_FLEET_SCOPE") {
        command.args([format!("--property=BindsTo={unit}"), format!("--property=After={unit}")]);
    }
}

pub fn run(arguments: &[String]) -> std::io::Error {
    let mut command = Command::new("systemd-run");
    bind(&mut command);
    if !arguments.iter().any(|argument| argument == "--scope")
        && let Ok(unit) = std::env::var("NORTH_FLEET_SCOPE") {
        command.arg(format!("--setenv=NORTH_FLEET_SCOPE={unit}"));
    }
    command.args(arguments).exec()
}

pub fn login_path(systemctl: &Path) -> Option<OsString> {
    let output = Command::new(systemctl)
        .args(["--user", "show-environment"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let login = output.stdout.split(|byte| *byte == b'\n').find_map(|line| line.strip_prefix(b"PATH="))?;
    let caller = std::env::var_os("PATH").unwrap_or_default();
    let user_bin = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(|home| PathBuf::from(home).join(".local/bin"));
    joined_path(OsStr::from_bytes(login), &caller, user_bin)
}

/// The user manager's PATH, then the caller's, then the user's `~/.local/bin`, which a user manager's PATH does not carry and where cargo lives on a build host.
fn joined_path(login: &OsStr, caller: &OsStr, user_bin: Option<PathBuf>) -> Option<OsString> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for directory in std::env::split_paths(login).chain(std::env::split_paths(caller)).chain(user_bin) {
        if !directory.as_os_str().is_empty() && !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    std::env::join_paths(directories).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_path_ends_with_the_users_local_bin_once_when_the_user_manager_path_lacks_it() {
        let login = OsStr::new("/run/wrappers/bin:/run/current-system/sw/bin");
        let caller = OsStr::new("/run/current-system/sw/bin:/home/tom/.local/bin");
        let user_bin = Some(PathBuf::from("/home/tom/.local/bin"));
        assert_eq!(joined_path(login, OsStr::new(""), user_bin.clone()).unwrap(), "/run/wrappers/bin:/run/current-system/sw/bin:/home/tom/.local/bin");
        assert_eq!(joined_path(login, caller, user_bin).unwrap(), "/run/wrappers/bin:/run/current-system/sw/bin:/home/tom/.local/bin");
    }
}
