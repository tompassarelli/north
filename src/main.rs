mod cli;

use north_core::{
    agent_catalog, agent_config_check, agent_policy_contract, autoland_status, capacity, code_health, contract_probe, error, fleet,
    foundation, grounding, guard, health, hook, lanes, ledger, ops, platform, providers, router, threads, tui, usage, work_items,
};
use north_landing::{dep_gate, follows_release, issue_box_lint, land_abandon, landing, language_gate, provenance, review_gate};

fn main() -> std::process::ExitCode {
    platform::stdio::with_blocking_outputs(|| {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        arguments
            .first()
            .filter(|argument| *argument == "private-desktop")
            .map_or_else(cli::main, |_| ops::private_desktop::run(&arguments[1..]))
    })
}
