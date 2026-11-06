use crate::pacing::placement::gate::ssh_destination;
use toml_edit::{DocumentMut, Item};


#[derive(Debug, Default, Eq, PartialEq)]
pub struct Deployment {
    pub destinations: Vec<String>,
    pub skipped: Vec<String>,
    pub relays_facts: Vec<Relayed>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Relayed {
    pub destination: String,
    pub host: String,
}

impl Deployment {
    pub fn skip_line(&self) -> Option<String> {
        (!self.skipped.is_empty()).then(|| format!("north runtime deploy: skipping {}: it builds no runtime, and the host that settles the fleet ships to it", self.skipped.join(", ")))
    }
}

/// Where this host ships runtimes. A host that builds no runtime (`builds_runtime = false`) is reached
/// only from a host that settles the fleet, the one host with an ssh route to it.
pub(super) fn deploy(cfg: &DocumentMut, here: &str) -> Deployment {
    let mut plan = Deployment::default();
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return plan };
    let settles = settles_fleet(cfg, here);
    for (name, host) in hosts.iter().filter(|(name, _)| *name != here) {
        let destination = ["deploy_ssh", "ssh"].iter().find_map(|key| ssh_destination(host.get(key)?.as_str()?));
        let Some(destination) = destination.filter(|destination| !plan.destinations.iter().any(|seen| seen == destination)) else { continue };
        if !settles && host.get("builds_runtime").and_then(Item::as_bool) == Some(false) {
            plan.skipped.push(name.to_owned());
        } else {
            if settles && !publishes_facts(host) {
                plan.relays_facts.push(Relayed { destination: destination.to_owned(), host: name.to_owned() });
            }
            plan.destinations.push(destination.to_owned());
        }
    }
    plan
}

fn publishes_facts(host: &Item) -> bool {
    host.get("publishes_host_facts").and_then(Item::as_bool).unwrap_or(true)
}

pub(super) fn publishes_host_facts(cfg: &DocumentMut, here: &str) -> bool {
    cfg.get("hosts").and_then(Item::as_table_like).and_then(|hosts| hosts.get(here)).is_none_or(publishes_facts)
}

pub(super) fn builds_runtime(cfg: &DocumentMut, here: &str) -> bool {
    cfg.get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("builds_runtime"))
        .and_then(Item::as_bool)
        .unwrap_or(true)
}

pub(super) fn settles_fleet(cfg: &DocumentMut, here: &str) -> bool {
    cfg.get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("settles_fleet"))
        .and_then(Item::as_bool)
        .unwrap_or(false)
}

pub(super) fn offloads_builds(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("offload_builds")).and_then(Item::as_bool) == Some(true)
}

/// The hosts this host may send a build to, or take one from: those it declares in `reaches` that offload builds. A host that declares no `reaches` list reaches only itself, so it sends nothing and is polled instead.
pub(super) fn build_hosts(cfg: &DocumentMut, here: &str) -> Vec<(String, String)> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    let reached = hosts.get(here).and_then(|host| host.get("reaches")?.as_array()).map(|names| names.iter().filter_map(|name| name.as_str()).collect::<Vec<_>>()).unwrap_or_default();
    hosts
        .iter()
        .filter(|(name, _)| *name != here && reached.contains(name) && offloads_builds(cfg, name))
        .filter_map(|(name, host)| Some((name.to_owned(), ssh_destination(host.get("ssh")?.as_str()?)?.to_owned())))
        .collect()
}

pub(super) fn pull_sources(cfg: &DocumentMut, here: &str) -> Vec<String> {
    let hosts = cfg.get("hosts").and_then(Item::as_table_like);
    let refused: Vec<&str> = hosts
        .into_iter()
        .flat_map(|hosts| hosts.iter())
        .filter(|(_, host)| host.get("runtime_source").and_then(Item::as_bool) == Some(false))
        .flat_map(|(_, host)| ["deploy_ssh", "ssh"].into_iter().filter_map(|key| host.get(key)?.as_str()))
        .collect();
    deploy(cfg, here).destinations.into_iter().filter(|destination| !refused.contains(&destination.as_str())).collect()
}

#[cfg(test)]
#[path = "destinations_tests.rs"]
mod tests;
