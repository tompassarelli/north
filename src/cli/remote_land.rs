use toml_edit::{DocumentMut, Item};

use crate::pacing::placement;
use north_safe_push::entry::{self, HostFacts};

pub fn safe_push(arguments: &[String]) -> u8 {
    if let Some(refusal) = north_runtime::worker_sandbox::lane_only_safe_push_refusal() {
        eprintln!("{refusal}");
        return 1;
    }
    let request: Vec<String> = std::iter::once("safe-push".to_owned()).chain(arguments.iter().cloned()).collect();
    if let Some(code) = north_runtime::worker_sandbox::reenter_outside(&request) {
        return code;
    }
    entry::safe_push(arguments, &host_facts())
}

pub fn hand_in(arguments: &[String]) -> u8 {
    north_lander::land::hand_in::run(arguments, |lane| road(lane, "hand-in", &["--wait"]))
}

pub fn finish(arguments: &[String]) -> u8 {
    north_lander::land::hand_in::finish(arguments, |lane| road(lane, "finish", &[]))
}

fn road(lane: &north_lander::land::hand_in::Lane, verb: &str, flags: &[&str]) -> u8 {
    match std::env::set_current_dir(&lane.root) {
        Ok(()) => safe_push(&["--to", lane.destination.as_str()].into_iter().chain(flags.iter().copied()).map(str::to_owned).collect::<Vec<_>>()),
        Err(error) => {
            eprintln!("{verb}: {}: {error}", lane.root.display());
            1
        }
    }
}

pub fn ledger_client(config: &DocumentMut, here: &str) -> bool {
    config
        .get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("ledger_client"))
        .and_then(Item::as_bool)
        .unwrap_or(false)
}

fn host_facts() -> HostFacts {
    HostFacts { ledger_client: placement::config().is_ok_and(|config| ledger_client(&config, &crate::platform::host::name())), abandon }
}

fn abandon(arguments: &[String]) -> u8 {
    north_landing::land_abandon::abandon(arguments, &super::abandon_witness::FleetWitness)
}
