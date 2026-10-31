use toml_edit::{DocumentMut, Item};

#[cfg(test)]
mod tests;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Deployment {
    pub destinations: Vec<String>,
    pub skipped: Vec<String>,
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
        let destination = ["deploy_ssh", "ssh"].iter().find_map(|key| host.get(key)?.as_str());
        let Some(destination) = destination.filter(|destination| !plan.destinations.iter().any(|seen| seen == destination)) else { continue };
        if !settles && host.get("builds_runtime").and_then(Item::as_bool) == Some(false) {
            plan.skipped.push(name.to_owned());
        } else {
            plan.destinations.push(destination.to_owned());
        }
    }
    plan
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

pub(super) fn build_hosts(cfg: &DocumentMut, here: &str) -> Vec<(String, String)> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    hosts
        .iter()
        .filter(|(name, host)| *name != here && host.get("offload_builds").and_then(Item::as_bool) == Some(true))
        .filter_map(|(name, host)| Some((name.to_owned(), host.get("ssh")?.as_str()?.to_owned())))
        .collect()
}
