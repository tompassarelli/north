use std::path::{Component, Path, PathBuf};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outbox {
    pub host: String,
    pub login: String,
    pub agent: bool,
}

impl Outbox {
    pub fn name(&self) -> &str {
        if self.agent { &self.login } else { &self.host }
    }
}

/// The outboxes the lander reads on `host`: the operator's, and the agent user's where `[hosts.NAME].agent_ssh` logs in as a user other than the operator.
pub fn outboxes(host: &str) -> Result<Vec<Outbox>, String> {
    let fleet = fleet();
    let operator = Outbox { host: host.to_owned(), login: login_in(fleet.as_ref(), host, orchestration::operator_login)?, agent: false };
    let agent = agent_in(fleet.as_ref(), host).map(|(login, _)| Outbox { host: host.to_owned(), login, agent: true });
    Ok(std::iter::once(operator).chain(agent).collect())
}

/// The login that reads, fetches and settles the lane at `root` on `host`: the agent user's when `root` lies under that user's home, the operator's otherwise.
pub fn lane_login(host: &str, root: &Path) -> Result<String, String> {
    let fleet = fleet();
    match agent_in(fleet.as_ref(), host) {
        Some((login, home)) if within(root, &home) => Ok(login),
        _ => login_in(fleet.as_ref(), host, orchestration::operator_login),
    }
}

fn agent_in(fleet: Option<&DocumentMut>, host: &str) -> Option<(String, PathBuf)> {
    let login = fleet.and_then(|fleet| orchestration::agent_login(fleet, host)).filter(|login| plain(login))?;
    let home = orchestration::login_home(&login, Path::new(&std::env::var_os("HOME").unwrap_or_default()))?;
    Some((login, home))
}

fn within(root: &Path, home: &Path) -> bool {
    root.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_))) && root.starts_with(home)
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
