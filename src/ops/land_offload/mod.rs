use crate::capacity::agent_load;
use crate::ops::pacing::placement::{config, gate};

mod room;
mod ship;

const USAGE: &str = "usage: north capacity offload choose | run --host NAME --base BASE --head HEAD --timeout SECONDS -- COMMAND";

pub(crate) fn run(arguments: &[String]) -> u8 {
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

fn choose() -> u8 {
    let local = agent_load::Host::from_env();
    if !(agent_load::gaming_marked(&local.capacity) || agent_load::game_reserve_on(&local)) {
        eprintln!("north capacity offload: this host is not in game-reserve, so the land check runs here");
        return 1;
    }
    let here = crate::platform::host::name();
    let others = machines().into_iter().filter(|machine| machine.name != here);
    match room::first_with_room(others.map(|machine| (machine.name, room::read(&machine.ssh)))) {
        Ok(host) => {
            println!("{host}");
            0
        }
        Err(refused) => {
            eprintln!("north capacity offload: no other host has room: {}", if refused.is_empty() { "none has an ssh destination".to_owned() } else { refused.join("; ") });
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
