use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Set on a command whose sandbox has hidden the sockets that reach the user manager, so a verb that needs a transient scope asks the outside broker for it.
pub const SOCKETS_HIDDEN: &str = "NORTH_SOCKETS_HIDDEN";

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
    joined_path(OsStr::from_bytes(login), &caller)
}

fn joined_path(login: &OsStr, caller: &OsStr) -> Option<OsString> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for directory in std::env::split_paths(login).chain(std::env::split_paths(caller)) {
        if !directory.as_os_str().is_empty() && !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    std::env::join_paths(directories).ok()
}

/// The unit-name prefix of a fleet session whose harness runs only inside the lane-only sandbox (DeepSeek's OpenCode and Pi). The launcher fixes the name when it starts the session's scope, so the outside broker and the credential broker read the confinement of a caller from its cgroup, a fact the caller cannot change, never from its environment or arguments (south#104).
pub const LANE_ONLY_UNIT_PREFIX: &str = "north-session-lane-";
const SESSION_UNIT_PREFIX: &str = "north-session-";
const LANE_ONLY_PROVIDERS: [&str; 2] = ["opencode", "pi"];

/// The scope unit a fleet session of `provider` starts in, named by `id`.
pub fn session_unit(provider: &str, id: &str) -> String {
    let prefix = if LANE_ONLY_PROVIDERS.contains(&provider) { LANE_ONLY_UNIT_PREFIX } else { SESSION_UNIT_PREFIX };
    format!("{prefix}{id}.scope")
}

/// Whether the process whose `/proc/PID/cgroup` reads `cgroup` runs in a lane-only session's scope.
pub fn lane_only_cgroup(cgroup: &str) -> bool {
    cgroup.lines().filter_map(|line| line.rsplit('/').next()).any(|leaf| leaf.starts_with(LANE_ONLY_UNIT_PREFIX) && leaf.ends_with(".scope"))
}

/// Whether process `pid` runs in a lane-only session's scope; `None` when its cgroup cannot be read, which a caller must treat as lane-only.
pub fn lane_only_process(pid: i32) -> Option<bool> {
    std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok().map(|cgroup| lane_only_cgroup(&cgroup))
}

#[cfg(test)]
#[path = "session_scope_tests.rs"]
mod tests;
