mod cli;
#[cfg(test)]
mod remote_serve_tests;
#[cfg(test)]
mod run_time_readers_tests;

use north_capacity_host::capacity;
use north_core::{
    agent_catalog, agent_config_check, agent_policy_contract, autoland_status, build_time, contract_probe, error, fleet, foundation,
    grounding, health, ledger, platform::{self, entry_points::{self, EntryPoint}}, providers, proxy, router, run_time_data, split, threads, usage, work_items,
};
use north_guard::{code_health, guard, hook, lanes, nix_explain};
use north_landing::{build_cache_gate, dep_gate, follows_release, issue_box_lint, land, land_abandon, landing, language_gate, provenance, runtime_floor_gate, watch};
use north_github_mirror as github_mirror;
use north_tui::tui;
use north_ops::ops;
use north_runtime::{agent_contain, build_offload, land_offload, laptop_ntfy, laptop_runner, runtime, runtime_access, timers};
use north_dispatch::{dispatch, laptop, loops, pacing, resident_cap, tune};

static RUN_TIME_READERS: [run_time_data::Consumer; 5] = [
    run_time_data::Consumer { path: "harness/loops.toml", validate: loops::validate },
    run_time_data::Consumer { path: "north-data/expectations.toml", validate: north_dispatch::check::validate },
    run_time_data::Consumer { path: "north-data/private-checks.toml", validate: ops::private_check::validate },
    run_time_data::Consumer { path: "harness/nix/cards.toml", validate: guard::nix_activity::validate },
    run_time_data::Consumer { path: "agent-machinery/file-skills.toml", validate: guard::file_skill::validate },
];

fn main() -> std::process::ExitCode {
    run_time_data::install(&RUN_TIME_READERS);
    pacing::spawn_gate::install(pacing::spawn_gate::SpawnGate {
        summary: || guard::spawn_capacity::gate_summary(&guard::spawn_capacity::Live { pace: pacing::answer }),
        launch_capacity: || guard::spawn_capacity::launch_capacity(&guard::spawn_capacity::Live { pace: pacing::answer }),
        launch_verdict: guard::spawn_capacity::launch_verdict,
    });
    platform::stdio::with_blocking_outputs(|| {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        if let Some(point) = std::env::args_os().next().and_then(|argv0| entry_points::named(&argv0)) {
            return enter(point, &arguments);
        }
        arguments
            .first()
            .filter(|argument| *argument == "private-desktop" && !cli::answers_help(&arguments))
            .map_or_else(cli::main, |_| ops::private_desktop::run(&arguments[1..]))
    })
}

fn enter(point: EntryPoint, arguments: &[String]) -> std::process::ExitCode {
    if point.scrubbed_variables().iter().any(|name| std::env::var_os(name).is_some()) {
        return relaunch_scrubbed(point, arguments);
    }
    match point {
        EntryPoint::SafePush => std::process::ExitCode::from(cli::remote_land::safe_push(arguments)),
    }
}

fn relaunch_scrubbed(point: EntryPoint, arguments: &[String]) -> std::process::ExitCode {
    use std::os::unix::process::CommandExt;
    let argv0 = std::env::args_os().next().unwrap_or_default();
    let mut command = std::process::Command::new("/proc/self/exe");
    command.arg0(argv0).args(arguments);
    for name in point.scrubbed_variables() {
        command.env_remove(name);
    }
    eprintln!("north: cannot relaunch with a clean environment: {}", command.exec());
    std::process::ExitCode::FAILURE
}
