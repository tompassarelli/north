use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::platform::{land_queue, orchestration};
use toml_edit::DocumentMut;

pub const SSH_OPTIONS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"];
pub const REMOTE_NORTH: &str = "n=\"$HOME/.local/state/north/current/north\"; [ -x \"$n\" ] || n=\"$(head -n 1 /etc/north/agent-runtime-link 2>/dev/null)/north\"; [ -x \"$n\" ] || n=north;";

pub fn fleet_hosts() -> Vec<String> {
    fleet().map(|fleet| orchestration::ssh_hosts(&fleet)).unwrap_or_default()
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

pub(super) fn fleet() -> Option<DocumentMut> {
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

pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn plain(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('-') && text.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@'))
}

pub fn ssh(host: &str, script: &str) -> Result<String, String> {
    ssh_with_input(host, script, b"")
}

pub fn ssh_with_input(host: &str, script: &str, input: &[u8]) -> Result<String, String> {
    let mut child = Command::new("ssh").args(SSH_OPTIONS).args(["-T", host, "--", script]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("ssh {host}: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    let output = child.wait_with_output().map_err(|error| format!("ssh {host}: {error}"))?;
    if !output.status.success() {
        return Err(format!("ssh {host} exited {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
