use std::io::{Read, Write};

use serde_json::json;

use super::admission::{self, Facts};
use super::effects::Host;
use super::enrich::admit_enriched;
use super::{agent_load, carry, deadline, dev_shell, game_reserve, gaming, gate, host, laptop_auto, mode, native_hold, seat_idle, unleased, warcraft, watchdog};

fn route(arguments: &[String], host: &Host) -> Option<u8> {
    let status = match arguments.first().map(String::as_str) {
        Some("admission") if arguments.len() == 1 => return None,
        Some("laptop-auto") => laptop_auto::run(&arguments[1..]),
        Some("seat-idle-hint") => seat_idle::run(&arguments[1..]),
        Some("offload") => (host.offload)(&arguments[1..]),
        Some("pull") => host::pull(&arguments[1..]),
        Some("actions-pull") => host::actions_pull(&arguments[1..]),
        Some("reap-orphans") if arguments.len() == 1 => {
            let status = agent_load::reap_orphans_run();
            (host.tend_disk)();
            status
        }
        Some("gaming") => gaming::run(&arguments[1..]),
        Some("mode") => mode::run(&arguments[1..]),
        Some("game-reserve") => game_reserve::run(&arguments[1..]),
        Some("deadline") => deadline::run(&arguments[1..]),
        Some("dev-shell") => dev_shell::run(&arguments[1..]),
        Some("exec") => dev_shell::exec(&arguments[1..]),
        Some("carry") => carry::run(&arguments[1..]),
        Some("warcraft-clients") if arguments.len() == 1 => warcraft::run(),
        Some("unleased") if arguments.len() == 1 => unleased::run(),
        Some("watchdog") => watchdog::run(&arguments[1..]),
        Some("model") => host::model_command(&arguments[1..]),
        Some("calibration") => host::calibration_command(&arguments[1..]),
        Some("quiet") if arguments.len() == 1 => native_hold::run(admission::quiet_idle_bound()),
        _ => {
            eprintln!("usage: north capacity admission < FACTS.json | laptop-auto [--dry-run] | seat-idle-hint [--seconds N] | reap-orphans | mode status|tick|hold [--minutes N] [PHRASE]|extend|release | gaming on|off|status | game-reserve on [--auto]|off|playing|status | offload choose|cost|free|run | deadline LEASE_FILE SECONDS -- COMMAND... | dev-shell -- COMMAND... | exec [--dev-shell DIRECTORY] -- COMMAND... | carry --list [--into sandbox|fetch|offload] | warcraft-clients | warcraft-limit | disk-share SHARE | unleased | watchdog sample|report|replay|rest-check|step | model [KEY] | calibration [--since 24h] | quiet < LEASES.json | host OPERATION ... | pull [--host NAME] | actions-pull | gate [--class] PROGRAM [ARG...]");
            2
        }
    };
    Some(status)
}

/// The host's configured Warcraft client cap, empty when it sets none.
pub fn limit_text(limit: Option<u32>) -> String {
    limit.map(|limit| limit.to_string()).unwrap_or_default()
}

pub fn run(arguments: &[String], host: Host) -> u8 {
    if arguments.first().map(String::as_str) == Some("host") {
        return host::run(&arguments[1..], &host);
    }
    if arguments.first().map(String::as_str) == Some("gate") {
        return gate::run(&arguments[1..], &host);
    }
    if arguments == ["warcraft-limit"] {
        println!("{}", limit_text((host.warcraft_clients)()));
        return 0;
    }
    if let [verb, share] = arguments && verb == "disk-share" {
        return disk_share(share);
    }
    if let Some(status) = route(arguments, &host) {
        return status;
    }
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Facts>(&input).map_err(|error| error.to_string()));
    match outcome {
        Ok(facts) => {
            let _ = writeln!(std::io::stdout(), "{}", json!(admit_enriched(facts, &host)));
            0
        }
        Err(message) => {
            eprintln!("north capacity: {message}");
            2
        }
    }
}

/// `north capacity disk-share SHARE`: the GiB this host's `[hosts.NAME.disk_budget_gib]` gives the store, or nothing when it names none.
fn disk_share(name: &str) -> u8 {
    let Some(share) = crate::platform::disk_budget::Share::parse(name) else {
        eprintln!("north capacity: disk-share is one of cargo_target, map_store, makgora_evidence, mutant_cargo, capture_batches");
        return 64;
    };
    if let Some(gib) = crate::platform::disk_budget::DiskBudget::local().gib(share) {
        println!("{gib}");
    }
    0
}
