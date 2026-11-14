use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Set on a command whose sandbox has hidden the sockets that reach the user manager, so a verb that needs a transient scope asks the outside broker for it.
pub const SOCKETS_HIDDEN: &str = "NORTH_SOCKETS_HIDDEN";

/// Set on a command a fleet worker runs inside its confining domain, so a verb that writes the host's capacity admission state asks the outside broker for it instead of writing it in place; the domain, not this mark, is what keeps the worker out of that state.
pub const CONFINED: &str = "NORTH_WORKER_CONFINED";

/// Set on a command whose sandbox keeps it from every other host, reading no ssh key and reaching no agent or master socket (south#118), so `north ssh-session` answers its ssh with the verb that reaches a build host.
pub const HOSTS_HIDDEN: &str = "NORTH_WORKER_HOSTS_HIDDEN";

/// The trees, as a JSON array of paths, a worker command may remove beneath, for the process the outside broker hands its work to: depbox sets it in the sandbox where its payload applies it, since a mount namespace cannot be built inside the worker's Landlock domain.
pub const REMOVAL_ROOTS: &str = "NORTH_WORKER_REMOVAL_ROOTS";

/// Set on a command inside a lane-only session's domain (DeepSeek's OpenCode and Pi): its code holds no GitHub token and no cloud call (south#104).
pub const LANE_ONLY: &str = "NORTH_WORKER_LANE_ONLY";

/// Where under the state directory each fleet session has its own spool location, named after it: the one spool path the worker sandbox lets a worker write, and the one a worker's fleet commands write to.
pub const SESSION_SPOOLS: &str = "north/fleet/spool/sessions";

/// The role a fleet launcher gives a session's processes: `proxy`, `lead`, `worker` or `reviewer`.
pub const ROLE_ENV: &str = "NORTH_FLEET_ROLE";

/// The one launcher role whose session keeps the domain open: the coordinator's seat, which drives windows and units.
pub const PROXY_ROLE: &str = "proxy";

/// The fleet session a request runs as, which the outside broker sets beside `PROVED` from the caller's own scope; a name without the proof is the client's word.
pub const SESSION: &str = "NORTH_FLEET_SESSION";

/// Set to `1` by the outside broker on a request whose session name it read from the caller's scope, never the client's word.
pub const PROVED: &str = "NORTH_FLEET_SESSION_PROVED";

/// The session `session` names when the broker set `proof` to `1`, else none.
pub fn proved_session(session: Option<String>, proof: Option<&str>) -> Option<String> {
    session.filter(|name| !name.trim().is_empty() && proof == Some("1"))
}

/// The fleet session this process runs as, when the outside broker proved its name.
pub fn proved_session_here() -> Option<String> {
    proved_session(std::env::var(SESSION).ok(), std::env::var(PROVED).ok().as_deref())
}

/// Whether a session whose launcher gave it `role` runs in the worker domain: every launched role, worker, lead and reviewer, with a task or without one, whose launches, revives, holds and forgets go through its spool to the host's fleet tick (fleet/registry/lead_grant.rs). The exemptions are the proxy seat and an adopted human window, which no launcher gave a role (north#1485).
pub fn confined_role(role: Option<&str>) -> bool {
    role.is_some_and(|role| role != PROXY_ROLE)
}

/// Whether this process belongs to a fleet session its launcher confined ([`confined_role`]).
pub fn confined_session() -> bool {
    confined_role(std::env::var(ROLE_ENV).ok().as_deref())
}

/// The confined session this process runs for, named by its launch's environment, whose own spool location its fleet rows go to.
pub fn spooling_worker() -> Option<String> {
    confined_session().then(|| std::env::var("NORTH_FLEET_SESSION").ok()).flatten().filter(|session| !session.is_empty())
}

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
/// The unit-name prefix of every fleet session's scope, which a lane-only one extends.
pub const SESSION_UNIT_PREFIX: &str = "north-session-";
const LANE_ONLY_PROVIDERS: [&str; 2] = ["opencode", "pi"];

/// The scope unit a fleet session of `provider` starts in, named by `id`.
pub fn session_unit(provider: &str, id: &str) -> String {
    let prefix = if LANE_ONLY_PROVIDERS.contains(&provider) { LANE_ONLY_UNIT_PREFIX } else { SESSION_UNIT_PREFIX };
    format!("{prefix}{id}.scope")
}

/// Whether the process whose `/proc/PID/cgroup` reads `cgroup` runs in a lane-only session's scope.
pub fn lane_only_cgroup(cgroup: &str) -> bool {
    cgroup.lines().flat_map(|line| line.split('/')).any(|leaf| leaf.starts_with(LANE_ONLY_UNIT_PREFIX) && leaf.ends_with(".scope"))
}

/// The unit name, and the start of every other name, of the outside broker's service, which starts what a sandboxed worker asks for.
pub const WORKER_BROKER_UNIT: &str = "north-worker-outside";

/// Whether the process whose `/proc/PID/cgroup` reads `cgroup` runs for a fleet worker: in a session's scope, lane-only or not, or in the outside broker's service.
pub fn worker_cgroup(cgroup: &str) -> bool {
    cgroup.lines().flat_map(|line| line.split('/')).any(|unit| (unit.starts_with(SESSION_UNIT_PREFIX) && unit.ends_with(".scope")) || unit.starts_with(WORKER_BROKER_UNIT))
}

/// Whether process `pid` runs in a lane-only session's scope; `None` when its cgroup cannot be read, which a caller must treat as lane-only.
pub fn lane_only_process(pid: i32) -> Option<bool> {
    std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok().map(|cgroup| lane_only_cgroup(&cgroup))
}

/// Whether a command with the lane-only `marker` and the caller's `cgroup` runs outside its session's own scope, as the
/// outside broker's fresh `--lane` child does: the broker decides what a lane-only caller may land from its cgroup, and a
/// child it started reads as the broker's own service. `None` counts as lane-only, as `lane_only_process` says.
pub fn lane_only_command_outside_scope(marker: bool, cgroup: Option<&str>) -> bool {
    marker && cgroup.is_some_and(|cgroup| !lane_only_cgroup(cgroup))
}

/// The service unit of the runtime promotion whose canaries start their sessions on the operational reserve; a lander unit is none, since it launches owners' workers.
pub const RESERVE_CALLER_UNIT: &str = "north-runtime-follow.service";

/// The prefix of the user scope a runtime canary starts its offloaded land check in, on the host it judges or over ssh on a build host; a worker command reaches no user manager to start one.
pub const CANARY_SCOPE_PREFIX: &str = "north-runtime-canary-";

/// Whether `unit` is a unit only the runtime promotion starts: its follow service, or a canary probe's scope.
pub fn is_reserve_caller(unit: &str) -> bool {
    unit == RESERVE_CALLER_UNIT || unit.strip_prefix(CANARY_SCOPE_PREFIX).and_then(|rest| rest.strip_suffix(".scope")).is_some_and(|id| !id.is_empty())
}

/// The unit a process whose `/proc/PID/cgroup` reads `cgroup` runs in when it is one of the reserve's callers, read from its cgroup, which a fleet session cannot leave.
pub fn reserve_caller_unit(cgroup: &str) -> Option<String> {
    let leaf = cgroup.lines().last()?.rsplit('/').next()?;
    is_reserve_caller(leaf).then(|| leaf.to_owned())
}

/// The reserve caller unit this process runs in, or none.
pub fn reserve_caller() -> Option<String> {
    std::fs::read_to_string("/proc/self/cgroup").ok().as_deref().and_then(reserve_caller_unit)
}

/// Whether this process is a lane-only command outside its session's own scope.
pub fn lane_only_outside_scope() -> bool {
    lane_only_command_outside_scope(std::env::var_os(LANE_ONLY).is_some(), std::fs::read_to_string("/proc/self/cgroup").ok().as_deref())
}

#[cfg(test)]
#[path = "session_scope_tests.rs"]
mod tests;
