use std::path::Path;

use toml_edit::{DocumentMut, Item};

use crate::pacing::placement;
use north_lander::land::hand_in::{Lane, Road};
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
    north_lander::land::hand_in::run(arguments, SafePush { verb: "hand-in", flags: &["--wait"] })
}

pub fn finish(arguments: &[String]) -> u8 {
    north_lander::land::hand_in::finish(arguments, SafePush { verb: "finish", flags: &[] })
}

struct SafePush {
    verb: &'static str,
    flags: &'static [&'static str],
}

impl Road for SafePush {
    fn writable(&self, root: &Path) -> Result<(), String> {
        north_runtime::worker_sandbox::stranded_git_locks(root).map_or(Ok(()), Err)
    }

    fn take(self, lane: &Lane) -> u8 {
        match std::env::set_current_dir(&lane.root) {
            Ok(()) => safe_push(&["--to", lane.destination.as_str()].into_iter().chain(self.flags.iter().copied()).map(str::to_owned).collect::<Vec<_>>()),
            Err(error) => {
                eprintln!("{}: {}: {error}", self.verb, lane.root.display());
                1
            }
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
