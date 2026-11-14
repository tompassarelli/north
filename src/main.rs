mod cli;
#[cfg(test)]
mod run_time_readers_tests;

use north_calibration::threads;
use north_capacity_host::capacity;
use north_core::{
    agent_catalog, agent_config_check, agent_policy_contract, autoland_status, contract_probe, error, fleet, foundation, land_ticket,
    grounding, platform::{self, entry_points::{self, EntryPoint}}, proxy, router, run_time_data, usage,
};
use north_health::health;
use north_providers::{ledger, providers};
use north_code_health::code_health;
use north_guard::{guard, hook, hook_input::Evidence, nix_explain};
use north_lanes::lanes;
use north_lander::land;
use north_land_watch::watch;
use north_landing::{build_cache_gate, dep_gate, follows_release, issue_box_lint, land_abandon, landing, language_gate, provenance};
use north_github_mirror as github_mirror;
use north_tui::tui;
use north_ci::{ci_watch, private_check};
use north_github_broker::gh_broker;
use north_desktop::{agent_capture, private_desktop, warcraft_close, wc3_login_field};
use north_runtime::{agent_contain, laptop_runner, runtime, worker_sandbox};
use north_dispatch::{dispatch, laptop, pacing, resident_cap, spend};
use north_tune::{loops, tune};
use north_flow::{agents, experiments, flow, tempo, tool_mine};
use north_convo::{context_audit, convo, policy_replay};
use north_timers::{privacy, runtime_access, timers};
use north_issue_boxes::{cargo_boxes, done_when_boxes, issue_child, issue_convert, observation_boxes, stale_issues};
use north_claims::{citation_check, review_items};

static RUN_TIME_READERS: [run_time_data::Consumer; 6] = [
    run_time_data::Consumer { path: "harness/defaults.toml", validate: router::policy::validate_defaults },
    run_time_data::Consumer { path: "harness/loops.toml", validate: loops::validate },
    run_time_data::Consumer { path: "north-data/expectations.toml", validate: north_expectations::check::validate },
    run_time_data::Consumer { path: "north-data/private-checks.toml", validate: private_check::validate },
    run_time_data::Consumer { path: "harness/nix/cards.toml", validate: guard::nix_activity::validate },
    run_time_data::Consumer { path: "harness/file-skills.toml", validate: guard::file_skill::validate },
];

fn main() -> std::process::ExitCode {
    run_time_data::install(&RUN_TIME_READERS);
    pacing::spawn_gate::install(pacing::spawn_gate::SpawnGate {
        summary: || guard::spawn_capacity::gate_summary(&guard::spawn_capacity::Live),
        launch_capacity: || guard::spawn_capacity::launch_capacity(&guard::spawn_capacity::Live),
        launch_verdict: guard::spawn_capacity::launch_verdict,
    });
    platform::stdio::with_blocking_outputs(|| {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        if let Some(point) = std::env::args_os().next().and_then(|argv0| entry_points::named(&argv0)) {
            return enter(point, &arguments);
        }
        if let Some(code) = worker_sandbox::tool_server::enter(&arguments, command_refusal) {
            return code;
        }
        arguments
            .first()
            .filter(|argument| *argument == "desktop" && arguments.get(1).is_some_and(|verb| verb == "private") && !cli::answers_help(&arguments))
            .map_or_else(cli::main, |_| private_desktop::run(&arguments[2..]))
    })
}

fn enter(point: EntryPoint, arguments: &[String]) -> std::process::ExitCode {
    if point.scrubbed_variables().iter().any(|name| std::env::var_os(name).is_some()) {
        return relaunch_scrubbed(point, arguments);
    }
    match point {
        EntryPoint::SafePush => std::process::ExitCode::from(cli::remote_land::safe_push(arguments)),
        EntryPoint::WorkerSandbox => worker_sandbox::run(arguments, command_refusal),
    }
}

fn command_refusal(command: &str, cwd: &std::path::Path) -> Option<String> {
    let (refusal, evidence) = hook::command_refusal(command, cwd);
    evidence.iter().for_each(record_evidence);
    refusal
}

fn record_evidence(evidence: &Evidence) {
    match evidence {
        Evidence::NativeSolo { session, reason, command } => {
            let _ = ledger::Ledger::open_default().and_then(|ledger| ledger.record_native_solo(session, reason, command));
        }
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
