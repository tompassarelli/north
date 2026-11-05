use crate::capacity::agent_load;
use crate::pacing::placement::{config, gate};
use ship::CommitId;
use std::path::Path;

mod destinations;
pub mod room;
pub mod ship;

const USAGE: &str = "usage: north capacity offload choose [--any [--taken HOST]... [--avoid HOST]...] (prints the host to run on, nothing when this host has the most room; each --taken counts one land check already on its way to HOST, and each --avoid leaves HOST out) | cost | free --host NAME (prints how many of the host's slots for this checkout are free) | reap --owner OWNER | run --host NAME --base BASE --head HEAD --timeout SECONDS -- COMMAND";

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["choose"] => choose(false, &Placement::default()),
        ["choose", "--any", rest @ ..] => match Placement::parse(rest) {
            Some(placement) => choose(true, &placement),
            None => usage(),
        },
        ["cost"] => cost(),
        ["free", "--host", host] => free(host),
        ["reap", "--owner", owner] if !owner.is_empty() => reap(owner),
        ["run", "--host", host, "--base", base, "--head", head, "--timeout", seconds, "--", command] => {
            let Ok(timeout_seconds) = seconds.parse() else { return usage() };
            let resolved = std::env::current_dir().map_err(|error| format!("current directory: {error}")).and_then(|checkout| commits_in(&checkout, base, head).map(|commits| (checkout, commits)));
            match resolved {
                Ok((checkout, (base, head))) => offload(host, ship::Land { ssh: "", checkout: &checkout, base, head, timeout_seconds, command }),
                Err(why) => {
                    eprintln!("north capacity offload: {why}");
                    2
                }
            }
        }
        _ => usage(),
    }
}

fn commits_in(checkout: &Path, base: &str, head: &str) -> Result<(Option<CommitId>, CommitId), String> {
    let base = if base.is_empty() { None } else { Some(CommitId::resolve(checkout, base)?) };
    Ok((base, CommitId::resolve(checkout, head)?))
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn machines() -> Vec<gate::Machine> {
    config().map(|cfg| gate::machines(&cfg)).unwrap_or_default()
}

pub fn build_hosts() -> Vec<(String, String)> {
    config().map(|cfg| destinations::build_hosts(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

pub use destinations::Deployment;

pub fn deployment() -> Deployment {
    config().map(|cfg| destinations::deploy(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

pub fn other_destinations() -> Vec<String> {
    deployment().destinations
}

pub fn pull_sources() -> Vec<String> {
    config().map(|cfg| pull_sources_in(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

pub fn pull_sources_in(cfg: &toml_edit::DocumentMut, here: &str) -> Vec<String> {
    destinations::pull_sources(cfg, here)
}

pub fn builds_runtime_here() -> bool {
    config().map(|cfg| destinations::builds_runtime(&cfg, &crate::platform::host::name())).unwrap_or(true)
}

pub fn settles_fleet_here() -> bool {
    config().is_ok_and(|cfg| destinations::settles_fleet(&cfg, &crate::platform::host::name()))
}

pub fn accepts_peer_runtimes_here() -> bool {
    config().is_ok_and(|cfg| !destinations::settles_fleet(&cfg, &crate::platform::host::name()))
}

pub fn in_game_reserve() -> bool {
    let local = agent_load::Host::from_env();
    agent_load::gaming_declared(&local)
}

pub fn checks_confined_here() -> bool {
    crate::capacity::mode::confined_agent_cpus(&agent_load::Host::from_env()).is_some()
}

pub fn host_with_room() -> Result<gate::Machine, String> {
    let here = crate::platform::host::name();
    let others: Vec<gate::Machine> = machines().into_iter().filter(|machine| machine.name != here).collect();
    let rooms = others.iter().map(|machine| (machine.name.clone(), room::read(&machine.ssh)));
    let host = room::first_with_room(rooms).map_err(|refused| {
        format!("no other host has room: {}", if refused.is_empty() { "none has an ssh destination".to_owned() } else { refused.join("; ") })
    })?;
    others.into_iter().find(|machine| machine.name == host).ok_or_else(|| format!("{host} left the host list"))
}

/// Prints each offload host's newest offloaded runs with their ship time and cache state, then its median ship time, read from the host's own records over ssh.
fn cost() -> u8 {
    let hosts = build_hosts();
    if hosts.is_empty() {
        eprintln!("north capacity offload: no build host is configured");
        return 1;
    }
    let remote = "\"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}\" capacity host offload-cost";
    let mut code = 0;
    for (name, ssh) in hosts {
        println!("{name}");
        let read = std::process::Command::new("ssh").args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &ssh, "--", remote]).status();
        if !read.is_ok_and(|status| status.success()) {
            eprintln!("north capacity offload: could not read {name}'s offload records");
            code = 1;
        }
    }
    code
}

fn reap(owner: &str) -> u8 {
    let mut code = 0;
    for (name, ssh) in build_hosts() {
        if ship::reap(&ssh, owner) != 0 {
            eprintln!("north capacity offload: could not reap {owner}'s runs on {name}");
            code = 1;
        }
    }
    code
}

/// What `choose --any` weighs besides each host's room: the land checks already on their way to a host, and the hosts a lane found unfit.
#[derive(Debug, Default, PartialEq, Eq)]
struct Placement {
    taken: Vec<String>,
    avoid: Vec<String>,
}

impl Placement {
    fn parse(words: &[&str]) -> Option<Self> {
        let mut placement = Self::default();
        for pair in words.chunks(2) {
            match pair {
                ["--taken", host] if !host.is_empty() => placement.taken.push((*host).to_owned()),
                ["--avoid", host] if !host.is_empty() => placement.avoid.push((*host).to_owned()),
                _ => return None,
            }
        }
        Some(placement)
    }

    fn admits(&self, host: &str) -> bool {
        !self.avoid.iter().any(|avoided| avoided == host)
    }
}

fn choose(any: bool, placement: &Placement) -> u8 {
    let in_reserve = in_game_reserve();
    if !any && !in_reserve {
        eprintln!("north capacity offload: this host is not in game-reserve, so the land check runs here");
        return 1;
    }
    if !any && checks_confined_here() {
        eprintln!("north capacity offload: this host keeps its game CPUs to game-reserve.slice and confines agent slices to the rest, so the land check runs here");
        return 1;
    }
    let here = crate::platform::host::name();
    let reserved = in_reserve || checks_confined_here() || !placement.admits(&here);
    let others: Vec<gate::Machine> = machines().into_iter().filter(|machine| machine.name != here && placement.admits(&machine.name)).collect();
    let remotes = std::thread::scope(|scope| {
        let reads: Vec<_> = others.iter().map(|machine| scope.spawn(|| (machine.name.clone(), room::read(&machine.ssh)))).collect();
        reads.into_iter().filter_map(|read| read.join().ok()).collect::<Vec<_>>()
    });
    match room::check_host(reserved, &here, room::read_local, remotes, &placement.taken, crate::capacity::host::land_check_cpus()) {
        Ok(name) if name == here => {
            eprintln!("north capacity offload: {here} has the most room, so the land check runs here");
            0
        }
        Ok(name) => {
            println!("{name}");
            0
        }
        Err(refused) => {
            eprintln!("north capacity offload: no host has room: {}", if refused.is_empty() { "none has an ssh destination".to_owned() } else { refused.join("; ") });
            1
        }
    }
}

fn free(host: &str) -> u8 {
    let Some(machine) = machines().into_iter().find(|machine| machine.name == host) else {
        eprintln!("north capacity offload: no [hosts.{host}] with an ssh destination");
        return 1;
    };
    match std::env::current_dir().map_err(|error| format!("current directory: {error}")).and_then(|directory| ship::free_slots(&machine.ssh, &directory)) {
        Ok(free) => {
            println!("{free}");
            0
        }
        Err(why) => {
            eprintln!("north capacity offload: {why}");
            1
        }
    }
}

fn offload(host: &str, land: ship::Land) -> u8 {
    let Some(machine) = machines().into_iter().find(|machine| machine.name == host) else {
        eprintln!("north capacity offload: no [hosts.{host}] with an ssh destination");
        return ship::TRANSPORT_FAILED;
    };
    ship::run(&ship::Land { ssh: &machine.ssh, ..land })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn a_placement_counts_each_taken_host_and_leaves_out_each_avoided_one() {
        let placement = super::Placement::parse(&["--taken", "gex45", "--avoid", "cx53", "--taken", "gex45"]).unwrap();
        assert_eq!(placement.taken, ["gex45", "gex45"]);
        assert!(!placement.admits("cx53"));
        assert!(placement.admits("gex45"));
        assert_eq!(super::Placement::parse(&[]), Some(super::Placement::default()));
        assert_eq!(super::Placement::parse(&["--avoid"]), None);
        assert_eq!(super::Placement::parse(&["--avoid", ""]), None);
        assert_eq!(super::Placement::parse(&["--skip", "cx53"]), None);
    }

    #[test]
    fn a_symbolic_head_ships_as_the_commit_it_names_an_empty_base_ships_everything_and_an_unknown_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| Command::new("git").current_dir(dir.path()).args(args).output().unwrap();
        git(&["init", "-q"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "x"]);
        let sha = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout).unwrap().trim().to_owned();
        let (base, head) = super::commits_in(dir.path(), "", "HEAD").unwrap();
        assert_eq!((base, head.as_str()), (None, sha.as_str()));
        assert!(super::commits_in(dir.path(), "no-such-ref", "HEAD").is_err());
        assert!(super::commits_in(dir.path(), "", "no-such-ref").is_err());
    }
}
