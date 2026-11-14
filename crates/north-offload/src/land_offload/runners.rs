use north_pacing::pacing::placement::gate::ssh_destination;
use toml_edit::{DocumentMut, Item};

use north_base::platform::orchestration::runners;

use super::destinations::opts_out_of_builds;

/// The coordinator's floor of live remote-execution runners the other open hosts keep while a host's runners close for an eval: a guess that lets one cpx host close (16 left of 28) at four times the 4 left on 2026-11-14, settled by the line wait in the next eval closure, and raised if that passes 120 s.
pub const RUNNER_FLOOR: u64 = 16;

/// Whether `host`, closed by `offload_builds = false`, also stops its runners, or why not: the other hosts that are `live` and open keep at least RUNNER_FLOOR runners between them.
pub fn closure(cfg: &DocumentMut, host: &str, live: impl Fn(&str) -> bool) -> Result<(), String> {
    let own = runners(cfg, host);
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Ok(()) };
    if !opts_out_of_builds(cfg, host) {
        return Ok(());
    }
    let left: u64 = hosts.iter().map(|(name, _)| name).filter(|name| *name != host && !opts_out_of_builds(cfg, name) && live(name)).map(|name| runners(cfg, name)).sum();
    if left >= RUNNER_FLOOR {
        return Ok(());
    }
    Err(format!("closing {host}'s {own} runners would leave {left} live runners on the other open hosts, under the coordinator's floor of {RUNNER_FLOOR}, so its executor keeps running while its land checks and offloaded builds stay closed"))
}

/// Each refused runner closure the configuration declares, every declared host counted live and a host declaring no runners left out: the operator check's reading of a commit.
pub fn refused_closures(cfg: &DocumentMut) -> Vec<String> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    hosts.iter().filter(|(name, _)| runners(cfg, name) > 0).filter_map(|(name, _)| closure(cfg, name, |_| true).err()).collect()
}

/// Whether `host` answers on its ssh port within two seconds, the run-time reading of a live host.
pub(super) fn answers(cfg: &DocumentMut, host: &str) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let Some(destination) = cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("ssh")).and_then(Item::as_str).and_then(ssh_destination) else { return false };
    let name = destination.rsplit('@').next().unwrap_or(destination);
    let Ok(addresses) = (name, 22).to_socket_addrs() else { return false };
    addresses.into_iter().any(|address| TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2)).is_ok())
}

#[cfg(test)]
#[path = "runners_tests.rs"]
mod tests;
