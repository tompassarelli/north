mod cli;
#[cfg(test)]
mod remote_serve_tests;

use north_capacity_host::capacity;
use north_core::{
    agent_catalog, agent_config_check, agent_policy_contract, autoland_status, build_time, contract_probe, error, fleet, foundation,
    grounding, health, ledger, platform, providers, proxy, router, split, threads, usage, work_items,
};
use north_guard::{code_health, guard, hook, lanes};
use north_landing::{build_cache_gate, dep_gate, follows_release, issue_box_lint, land, land_abandon, landing, language_gate, provenance, runtime_floor_gate, watch};
use north_tui::tui;
use north_ops::ops;
use north_runtime::{agent_contain, build_offload, land_offload, laptop_ntfy, laptop_runner, model_watch, runtime, runtime_access, timers};

fn main() -> std::process::ExitCode {
    ops::pacing::spawn_gate::install(ops::pacing::spawn_gate::SpawnGate {
        summary: || guard::spawn_capacity::gate_summary(&guard::spawn_capacity::Live { pace: ops::pacing::answer }),
        launch_refusal: |brief| guard::spawn_capacity::launch_refusal(&guard::spawn_capacity::Live { pace: ops::pacing::answer }, brief),
    });
    platform::stdio::with_blocking_outputs(|| {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        arguments
            .first()
            .filter(|argument| *argument == "private-desktop")
            .map_or_else(cli::main, |_| ops::private_desktop::run(&arguments[1..]))
    })
}
