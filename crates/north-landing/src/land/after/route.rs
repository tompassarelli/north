use north_base::platform::{land_queue, orchestration};
use toml_edit::DocumentMut;

pub const SSH_OPTIONS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"];
pub const REMOTE_NORTH: &str = "n=\"$HOME/.local/state/north/current/north\"; [ -x \"$n\" ] || n=\"$(head -n 1 /etc/north/agent-runtime-link 2>/dev/null)/north\"; [ -x \"$n\" ] || n=north;";

pub fn fleet_hosts() -> Vec<String> {
    fleet().map(|fleet| orchestration::ssh_hosts(&fleet)).unwrap_or_default()
}

pub fn session_login(host: &str) -> Result<String, String> {
    login_in(fleet().as_ref(), host, orchestration::agent_login)
}

pub fn operator_login(host: &str) -> Result<String, String> {
    login_in(fleet().as_ref(), host, orchestration::operator_login)
}

fn fleet() -> Option<DocumentMut> {
    land_queue::confined(&orchestration::snapshot_path()).ok().and_then(|()| orchestration::snapshot().ok())
}

fn login_in(fleet: Option<&DocumentMut>, host: &str, declared: fn(&DocumentMut, &str) -> Option<String>) -> Result<String, String> {
    match fleet.and_then(|fleet| declared(fleet, host)) {
        None if plain(host) => Ok(host.to_owned()),
        None => Err(format!("{host:?} is not an ssh host name")),
        Some(login) if plain(&login) => Ok(login),
        Some(login) => Err(format!("{host} declares the login {login:?}, which is not an ssh login")),
    }
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

pub fn plain(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('-') && text.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@'))
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
