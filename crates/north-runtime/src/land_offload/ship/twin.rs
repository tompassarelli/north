use std::time::Duration;

use north_base::foundation::offload;

use super::{Land, OFFLOAD_ROOT, quote};
use crate::platform::process::run_within;

const LIMIT: Duration = Duration::from_secs(3);
const SETTING: &str = "capacity_line_wait";

/// The probe a build host answers with its own name when a job for `$1` and `$2` (a head and a command) holds that host's in-flight marker, which the offload script keeps locked from its ticket until it exits.
const HOLDS: &str = r#"f=@ROOT@/inflight/$(printf '%s\n%s' "$1" "$2" | sha256sum | cut -c1-32); [ -e "$f" ] && ! flock -n "$f" true && uname -n; true"#;

/// The build hosts to ask: none unless the caller opted in with `away` (`TWIN_AWAY_ENV`, the hosts the run failed on), else every host not in it.
pub(super) fn targets(away: Option<&str>, hosts: Vec<(String, String)>) -> Vec<(String, String)> {
    let Some(away) = away else { return Vec::new() };
    hosts.into_iter().filter(|(name, _)| !away.split_whitespace().any(|failed| failed == name)).collect()
}

/// The build host a job of this head and `command` (the command as the build host runs it) already waits or runs on, among every build host this host reaches, or None when the caller did not opt in, no host holds one, a host does not answer in time, this run is on this host, or `capacity_line_wait` is false.
pub(super) fn held_elsewhere(land: &Land, command: &str) -> Option<String> {
    if land.ssh.is_empty() || !enabled() {
        return None;
    }
    let probe = HOLDS.replace("@ROOT@", OFFLOAD_ROOT);
    let hosts = targets(std::env::var(offload::TWIN_AWAY_ENV).ok().as_deref(), super::super::check_hosts());
    std::thread::scope(|scope| {
        let asks: Vec<_> = hosts
            .iter()
            .map(|(_, ssh)| {
                let probe = &probe;
                scope.spawn(move || {
                    let remote = format!("sh -c {} twin {} {}", quote(probe), quote(land.head.as_str()), quote(command));
                    let master = super::master::options(ssh);
                    let mut arguments = vec!["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=2"];
                    arguments.extend(master.iter().map(String::as_str));
                    arguments.extend([ssh.as_str(), "--", remote.as_str()]);
                    let told = run_within(&offload::ssh(), &arguments, LIMIT)?;
                    told.status.success().then(|| String::from_utf8_lossy(&told.stdout).trim().to_owned()).filter(|host| !host.is_empty())
                })
            })
            .collect();
        asks.into_iter().find_map(|ask| ask.join().ok().flatten())
    })
}

fn enabled() -> bool {
    north_base::platform::harness::document(north_base::platform::harness::DEFAULTS).get(SETTING).and_then(|entry| entry.get("value")).and_then(|value| value.as_bool()).unwrap_or(true)
}
