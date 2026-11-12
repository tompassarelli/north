use north_base::platform::{land_queue, orchestration};

pub const SSH_OPTIONS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"];
pub const REMOTE_NORTH: &str = "n=\"$HOME/.local/state/north/current/north\"; [ -x \"$n\" ] || n=north;";

/// The fleet's ssh hosts in the shared-config snapshot's orchestration.toml, none when it is stale or a test fixture root is declared and the file lies outside it.
pub fn fleet_hosts() -> Vec<String> {
    let fleet = land_queue::confined(&orchestration::snapshot_path()).ok().and_then(|()| orchestration::snapshot().ok());
    fleet.map(|fleet| orchestration::ssh_hosts(&fleet)).unwrap_or_default()
}

pub trait Courier {
    fn here(&self, session: &str, message: &str) -> bool;
    fn on(&self, host: &str, session: &str, message: &str) -> bool;
}

pub fn host_of<'a>(hosts: &'a [String], session: &str) -> Option<&'a str> {
    let named = |host: &&String| session.strip_prefix(host.as_str()).and_then(|rest| rest.strip_prefix('-')).is_some_and(|rest| !rest.is_empty());
    hosts.iter().filter(named).max_by_key(|host| host.len()).map(String::as_str)
}

pub fn deliver(courier: &dyn Courier, hosts: &[String], session: &str, message: &str) -> bool {
    match host_of(hosts, session) {
        Some(host) => courier.on(host, session, message),
        None => courier.here(session, message),
    }
}

pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
