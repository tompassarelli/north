//! The lane-only session scope a `--lane` child the outside broker starts joins before it runs the session's shell command, so the `safe-push` or `task check` that command asks the broker for comes from the session's own cgroup, the one fact lane_grant.rs grants by (north#1068).

use std::path::Path;

use north_base::platform::session_scope;

use super::{FLAG, LANE, SESSION_SCOPE};

/// The variable that names, to the `--lane` child the broker starts for a lane-only session's shell command (`arguments`), the `cgroup.procs` of the session scope `cgroup` (the caller's `/proc/PID/cgroup`) names, in the cgroup filesystem at `root`. The child would otherwise run in the broker's own cgroup, so a `safe-push` or `task check` its command asks for would reach the broker from outside every session and be refused; it joins this scope first and keeps its session's grant (north#1068).
pub(super) fn scope_for(arguments: &[String], cgroup: Option<&str>, root: &Path) -> Option<(String, String)> {
    let [flag, lane, ..] = arguments else { return None };
    if flag != FLAG || lane != LANE {
        return None;
    }
    let path = cgroup?.lines().find_map(|line| line.strip_prefix("0::"))?;
    let procs = root.join(path.trim_start_matches('/')).join("cgroup.procs");
    session_scope::lane_only_cgroup(path).then(|| (SESSION_SCOPE.to_owned(), procs.to_string_lossy().into_owned()))
}

/// Moves this process, and the command it then runs, into the session scope the broker named with [`scope_for`], telling the caller when it cannot.
pub(super) fn join_named_scope() {
    let Some(procs) = std::env::var_os(SESSION_SCOPE) else { return };
    if let Err(error) = join_scope(Path::new(&procs)) {
        eprintln!("north worker-sandbox: cannot join the session's scope through {} ({error}), so a safe-push or task check this command asks the broker for is refused", Path::new(&procs).display());
    }
}

/// Moves this process into the cgroup whose `cgroup.procs` is `procs`.
pub(super) fn join_scope(procs: &Path) -> std::io::Result<()> {
    std::fs::write(procs, "0")
}
