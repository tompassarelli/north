use crate::capacity::agent_load;
use crate::ops::pacing::placement::{config, gate};

mod destinations;
pub mod room;
pub mod ship;

const USAGE: &str = "usage: north capacity offload choose | run --host NAME --base BASE --head HEAD --timeout SECONDS -- COMMAND";

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["choose"] => choose(),
        ["run", "--host", host, "--base", base, "--head", head, "--timeout", seconds, "--", command] => match seconds.parse() {
            Ok(timeout_seconds) => offload(host, ship::Land { ssh: "", base, head, timeout_seconds, command }),
            Err(_) => usage(),
        },
        _ => usage(),
    }
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

pub fn other_destinations() -> Vec<String> {
    config().map(|cfg| destinations::deploy(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

pub fn builds_runtime_here() -> bool {
    config().map(|cfg| destinations::builds_runtime(&cfg, &crate::platform::host::name())).unwrap_or(true)
}

pub fn settles_fleet_here() -> bool {
    config().is_ok_and(|cfg| destinations::settles_fleet(&cfg, &crate::platform::host::name()))
}

pub fn in_game_reserve() -> bool {
    let local = agent_load::Host::from_env();
    agent_load::gaming_declared(&local)
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

fn choose() -> u8 {
    if !in_game_reserve() {
        eprintln!("north capacity offload: this host is not in game-reserve, so the land check runs here");
        return 1;
    }
    match host_with_room() {
        Ok(machine) => {
            println!("{}", machine.name);
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
