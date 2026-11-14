use north_pacing::pacing::placement::gate::ssh_destination;
use north_base::platform::orchestration::reaches;
use toml_edit::{DocumentMut, Item};


#[derive(Debug, Default, Eq, PartialEq)]
pub struct Deployment {
    pub destinations: Vec<String>,
    pub names: Vec<String>,
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
            plan.names.push(name.to_owned());
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

/// Whether a host other than `here` settles the fleet.
pub(super) fn settled_elsewhere(cfg: &DocumentMut, here: &str) -> bool {
    cfg.get("hosts").and_then(Item::as_table_like).is_some_and(|hosts| hosts.iter().any(|(name, _)| name != here && settles_fleet(cfg, name)))
}

/// Whether a host other than `here` settles the fleet by the configuration `read` from `path`: a home that keeps no south at all (`south_kept` false) and has no file at `path` declares no fleet, so no host settles one for it; a file missing from a south it keeps, and any other failure to read it, is an error naming it.
pub(super) fn settled_elsewhere_read(read: Result<DocumentMut, String>, here: &str, path: &std::path::Path, south_kept: bool) -> Result<bool, String> {
    match read {
        Ok(cfg) => Ok(settled_elsewhere(&cfg, here)),
        Err(_) if !south_kept && std::fs::metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) => Ok(false),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Whether `here` runs the pinned runtime another host settles and ships to it, rather than following origin/main itself: some other host settles the fleet and `here` has the ssh destination that host ships through.
pub(super) fn pinned_by_settler(cfg: &DocumentMut, here: &str) -> bool {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return false };
    let settler = settled_elsewhere(cfg, here);
    let reached = hosts.get(here).is_some_and(|host| ["deploy_ssh", "ssh"].iter().any(|key| host.get(key).and_then(Item::as_str).and_then(ssh_destination).is_some()));
    settler && reached
}

pub(super) fn opts_out_of_builds(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("offload_builds")).and_then(Item::as_bool) == Some(false)
}

pub(super) fn runs_no_checks(cfg: &DocumentMut, host: &str) -> bool {
    north_base::platform::orchestration::runs_no_checks(cfg, host)
}

pub(super) fn takes_land_checks(cfg: &DocumentMut, standing: super::room::Standing, host: &str) -> bool {
    !opts_out_of_builds(cfg, host) && (standing != super::room::Standing::Fallback || offloads_builds(cfg, host))
}

pub(super) fn warms_slots(cfg: &DocumentMut, here: &str) -> bool {
    !opts_out_of_builds(cfg, here)
}

/// The login a lane's offloaded check runs as on `machine`: its agent login once `[hosts.NAME] agent_offload = true` enables it, else its ssh destination; none when agent_offload is set on a host that declares no agent login, so its checks run nowhere rather than as the operator.
pub(super) fn check_login(cfg: &DocumentMut, machine: &north_pacing::pacing::placement::gate::Machine) -> Option<String> {
    let enabled = cfg.get("hosts").and_then(|hosts| hosts.get(&machine.name)).and_then(|host| host.get("agent_offload")).and_then(Item::as_bool) == Some(true);
    let login = if enabled { machine.agent_ssh.clone() } else { Some(machine.ssh.clone()) };
    if login.is_none() {
        eprintln!("north capacity offload: {} sets agent_offload = true but names no agent_ssh, so no offloaded check runs there", machine.name);
    }
    login
}

/// Whether `login` is the agent login of `host`, so the check runs confined (land_offload/ship/fill.sh confine).
pub(super) fn runs_as_agent(cfg: &DocumentMut, host: &str, login: &str) -> bool {
    north_pacing::pacing::placement::gate::machines(cfg).iter().any(|machine| machine.name == host && machine.agent_ssh.as_deref() == Some(login) && machine.ssh != login)
}

/// Each build host `here` reaches, with the login a lane's check runs as there (check_login); a host whose agent login does not parse, or that sets agent_offload with no agent login, is left out rather than dialed as the operator.
pub(super) fn check_hosts(cfg: &DocumentMut, here: &str) -> Vec<(String, String)> {
    let machines = north_pacing::pacing::placement::gate::machines(cfg);
    build_hosts(cfg, here).into_iter().filter_map(|(name, _)| machines.iter().find(|machine| machine.name == name).and_then(|machine| check_login(cfg, machine)).map(|login| (name, login))).collect()
}

pub(super) fn offloads_builds(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("offload_builds")).and_then(Item::as_bool) == Some(true)
}

/// The hosts `here` reaches whose `offload_builds = false` closes them to offloaded builds for now.
pub(super) fn closed_build_hosts(cfg: &DocumentMut, here: &str) -> Vec<String> {
    let closed = |name: &str| cfg.get("hosts").and_then(|hosts| hosts.get(name)).and_then(|host| host.get("offload_builds")).and_then(Item::as_bool) == Some(false);
    reached_destinations(cfg, here).into_iter().map(|(name, _)| name).filter(|name| closed(name)).collect()
}

/// The other hosts `here` reaches over ssh, each with its destination, in the order `[hosts]` declares them. A host that declares no `reaches` list reaches only itself.
pub(super) fn reached_destinations(cfg: &DocumentMut, here: &str) -> Vec<(String, String)> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    hosts
        .iter()
        .filter(|(name, _)| *name != here && reaches(cfg, here, name))
        .filter_map(|(name, host)| Some((name.to_owned(), ssh_destination(host.get("ssh")?.as_str()?)?.to_owned())))
        .collect()
}

/// The login a lane's check runs as on `host` when `here` reaches it, else why an offload to it is refused before any dial, naming the hosts `here` reaches.
pub(super) fn reached_destination(cfg: &DocumentMut, here: &str, host: &str) -> Result<String, String> {
    let reached = reached_destinations(cfg, here);
    if reached.iter().any(|(name, _)| name == host) {
        let machines = north_pacing::pacing::placement::gate::machines(cfg);
        return machines.iter().find(|machine| machine.name == host).and_then(|machine| check_login(cfg, machine)).ok_or_else(|| format!("{host} has no login a check may run as, so no offload is dialed for it"));
    }
    let names: Vec<&str> = reached.iter().map(|(name, _)| name.as_str()).collect();
    let names = if names.is_empty() { "no other host".to_owned() } else { names.join(", ") };
    Err(format!("{host} is not a host this one reaches over ssh, so no offload is dialed for it; this one reaches {names}"))
}

/// The hosts this host may send a build to, or take one from: those it reaches that offload builds.
pub(super) fn build_hosts(cfg: &DocumentMut, here: &str) -> Vec<(String, String)> {
    reached_destinations(cfg, here).into_iter().filter(|(name, _)| offloads_builds(cfg, name)).collect()
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
