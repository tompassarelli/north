//! The fleet's host table: South's orchestration.toml and the `[hosts.NAME]` machines it names with an ssh destination, which placement, the offload and the laptop runner read.

use toml_edit::{DocumentMut, Item};

/// South's orchestration.toml.
pub fn config() -> Result<DocumentMut, String> {
    std::fs::read_to_string(north_base::platform::orchestration::path()).map_err(|e| e.to_string())?.parse().map_err(|e: toml_edit::TomlError| e.to_string())
}

/// A `[hosts.NAME]` machine: its name, the operator's ssh destination and the agent login when it declares one.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub name: String,
    pub ssh: String,
    pub agent_ssh: Option<String>,
}

impl Machine {
    pub fn session_login(&self) -> &str {
        self.agent_ssh.as_deref().unwrap_or(&self.ssh)
    }
}

pub fn ssh_destination(value: &str) -> Option<&str> {
    let plain = value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._@-".contains(&byte));
    (plain && !value.is_empty() && !value.starts_with('-')).then_some(value)
}

pub fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).map(|array| array.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// Machines in `[placement] order`, then any other `[hosts.NAME]` naming an `ssh` destination; a host whose `ssh` or `agent_ssh` does not parse as a destination is left out.
pub fn machines(cfg: &DocumentMut) -> Vec<Machine> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    let order = strings(cfg.get("placement").and_then(|p| p.get("order")));
    let rest: Vec<String> = hosts.iter().map(|(name, _)| name.to_owned()).filter(|name| !order.contains(name)).collect();
    order
        .into_iter()
        .chain(rest)
        .filter_map(|name| {
            let host = hosts.get(&name)?;
            let ssh = ssh_destination(host.get("ssh")?.as_str()?)?.to_owned();
            let agent_ssh = match host.get("agent_ssh") {
                Some(login) => Some(ssh_destination(login.as_str()?)?.to_owned()),
                None => None,
            };
            Some(Machine { name, ssh, agent_ssh })
        })
        .collect()
}

/// Whether `[hosts.NAME] job_source = true` lets this host take jobs planted in that host's ledger.
pub fn job_source(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("job_source")).and_then(Item::as_bool) == Some(true)
}
