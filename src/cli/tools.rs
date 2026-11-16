use std::process::ExitCode;

use crate::landing::{after_landing, bin_references, denylist, merge_set, landed_tree, stale_revert};
use self::graph::graph;
use self::work_split::work_split;
use self::subverbs::{Run, code_health, home, lane, land, ledger_verb, ops, runner, runtime};
pub(super) use self::subverbs::ledger_runner;
use crate::timers::{self, Timer};
use crate::{
    agent_config_check, agent_policy_contract, autoland_status, capacity, dep_gate, follows_release, issue_box_lint,
    provenance,
};

mod graph;
mod subverbs;
mod work_split;

const UNIFORM: &[(&str, Run)] = &[
    ("agent-policy-contract", agent_policy_contract::run),
    ("dep-gate", dep_gate::run),
    ("evidence", north_reclaim::reclaim::evidence_start),
    ("denylist", denylist::run),
    ("autoland-status", autoland_status::run),
    ("check", check),
    ("land", land),
    ("cause", |arguments| ledger_verb("cause", arguments)),
    ("landed-tree", landed_tree::run),
    ("stale-revert", stale_revert::run),
    ("merge-driver", merge_set::run),
    ("after-landing", after_landing::run),
    ("bin-references", bin_references::run),
    ("safe-push", super::remote_land::safe_push),
    ("hand-in", super::remote_land::hand_in),
    ("finish", super::remote_land::finish),
    ("follows-release", follows_release::run),
    ("job-run", north_timers::job_run::run),
    ("cloud", super::cloud::run),
    ("hardcoded-repo-path-check", crate::lanes::hardcoded_paths),
    ("pin-retire", crate::lanes::pin_retire),
    ("graph", graph),
    ("resident-cap", crate::resident_cap::run),
    ("docs", north_docs::docs::run),
    ("convo", crate::convo::run),
    ("incident-mine", north_convo::incident_mine::run),
    ("tool-mine", crate::tool_mine::run),
    ("policy-replay", crate::policy_replay::run),
    ("defaults-measure", north_tune::defaults_measure::run),
    ("spend", super::spend::run),
    ("fetch-fleet", super::fetch_fleet::run),
    ("fleet-for", super::fleet_for::run),
    ("fleet-issue", super::fleet_issue::run),
    ("proof-run", super::proof_run::run),
    ("control-plane", super::control_plane::run),
    ("coordinator", |arguments| match arguments.first().map(String::as_str) {
        Some("calls") => {
            eprintln!("did you mean: north config coordinator {}", arguments.join(" "));
            2
        }
        _ => super::fleet::coordinator(arguments),
    }),
    ("ops", ops),
    ("private-check", crate::private_check::run),
    ("ssh-argv", crate::platform::ssh::run),
    ("provider-capacity", crate::capacity::provider_capacity::run),
    ("github-mirror", crate::github_mirror::sync::run),
    ("gh-session", |arguments| crate::platform::gh_session::run(arguments, crate::guard::gh_screen::screen)),
    ("ssh-session", crate::platform::ssh_session::run),
    ("ssh-master", crate::platform::ssh_master::run),
    ("placement-survey", |arguments| crate::pacing::placement::keep_surveys(arguments, crate::dispatch::refresh_fleet_listings)),
    ("ssh-spare", crate::pacing::placement::hold_spares),
    ("spare-fill", super::fleet::fill_spares),
    ("spare-retire", super::fleet::retire_spares),
    ("git-credential", crate::platform::git_credential::run),
    ("gh-broker", crate::gh_broker::run),
    ("broker", north_broker::run),
    ("issue", |arguments| match arguments {
        [word, rest @ ..] if word == "child" => file_child(rest),
        _ => crate::issue_convert::run(arguments),
    }),
];

const DIRECT: &[(&str, Run)] = &[
    ("nexus-ram", north_capacity_host::nexus_ram::run),
    ("laptop-runner", crate::laptop_runner::run),
    ("agent-contain", crate::agent_contain::run),
    ("retain", north_reclaim::retention::run),
    ("buck", |arguments| north_buck::run(arguments, &north_buck::Cargo { manifests: north_build_system::manifest_graph, features: north_build_system::feature_graph }, &north_buck::Sides { buck2: north_build_system::cases_buck2_runs, nextest: north_build_system::cases_nextest_lists }, &crate::platform::records_mirror::state_home())),
    ("lane", lane),
    ("home", home),
    ("runtime", runtime),
    ("tune", crate::tune::run),
    ("loops", crate::loops::run),
    ("code-health", code_health),
    ("dash", super::dash::run),
    ("infra", super::views::infra::run),
    ("depbox", |arguments| capacity::depbox::run(arguments, &capacity_host())),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Uniform(&'static str),
    Direct(&'static str),
    Pace,
    Capacity,
    SessionUnit,
    Dispatch,
    Fleet(&'static str),
    Laptop,
    Config,
    Work,
    Test,
    Timer(Timer),
}

const FLEET: &[(&str, Run)] = &[("report", retired_work), ("usage", retired_usage)];

fn retired_work(_: &[String]) -> u8 {
    eprintln!("north fleet report moved to `north work`; its sessions, gates, landings, report and lane flow print there");
    2
}

fn retired_usage(arguments: &[String]) -> u8 {
    let writer = arguments.iter().any(|argument| matches!(argument.as_str(), "--refresh" | "--local" | "--write-pace-gate" | "--fast" | "--cached" | "--gate" | "--rows" | "sample"));
    if writer {
        return super::agents::usage(arguments);
    }
    eprintln!("north fleet usage's account view moved to `north accounts`; `north fleet usage --refresh` and the provider flags still write readings");
    2
}

const OPERATIONS: &[(&str, Tool)] = &[
    ("test", Tool::Test),
    ("capacity", Tool::Capacity),
    ("session-unit", Tool::SessionUnit),
];

impl Tool {
    pub(super) fn fleet_verb(verb: &str) -> Option<Self> {
        match verb {
            "dispatch" => Some(Self::Dispatch),
            "pace" => Some(Self::Pace),
            "laptop" => Some(Self::Laptop),
            _ => named(FLEET, verb).map(Self::Fleet),
        }
    }

    pub(super) fn named(command: &str) -> Option<Self> {
        named(UNIFORM, command)
            .map(Self::Uniform)
            .or_else(|| named(DIRECT, command).map(Self::Direct))
            .or_else(|| OPERATIONS.iter().find(|(name, _)| *name == command).map(|(_, tool)| *tool))
            .or_else(|| Timer::named(command).map(Self::Timer))
    }

    pub(super) fn run(self, arguments: &[String]) -> ExitCode {
        match self {
            Self::Uniform(name) => {
                let run = runner(UNIFORM, name);
                super::in_runtime(async move { Ok(ExitCode::from(run.map_or(2, |run| run(arguments)))) })
            }
            Self::Direct(name) => ExitCode::from(runner(DIRECT, name).map_or(2, |run| run(arguments))),
            Self::Dispatch => ExitCode::from(super::fleet::dispatch(arguments)),
            Self::Fleet(name) => {
                let run = runner(FLEET, name);
                super::in_runtime(async move { Ok(ExitCode::from(run.map_or(2, |run| run(arguments)))) })
            }
            Self::Laptop => ExitCode::from(crate::laptop::run(arguments)),
            Self::Config => super::in_runtime(async move { Ok(ExitCode::from(config(arguments))) }),
            Self::Work => super::in_runtime(async move { Ok(ExitCode::from(work(arguments))) }),
            Self::Test => match arguments.split_first() {
                Some((verb, rest)) if let Some(run) = runner(TEST, verb) => super::in_runtime(async move { Ok(ExitCode::from(run(rest))) }),
                _ => {
                    eprintln!("usage: north test (coverage|mutant) ...; north test --help lists each");
                    ExitCode::from(2)
                }
            },
            Self::Pace => ExitCode::from(crate::pacing::run(arguments, crate::pacing::FleetFacts { roster: super::fleet::roster, machine_load: super::fleet::fleet_load, workers: crate::dispatch::fleet_workers }, super::token_usage::read)),
            Self::Capacity => match arguments.split_first() {
                Some((verb, rest)) if verb == "bounded" => ExitCode::from(north_capacity::run_bounded::run(rest)),
                Some((verb, rest)) if verb == "nexus" => super::in_runtime(async move { Ok(ExitCode::from(timers::nexus_run(rest).await)) }),
                Some((verb, rest)) if verb == "dev-shell" && rest.first().is_some_and(|run| run == "run") => ExitCode::from(crate::capacity::dev_shell::command(rest, &|command| capacity::gate::lease_class(command, &capacity_host()).is_some())),
                _ => ExitCode::from(capacity::command::run(arguments, capacity_host())),
            },
            Self::SessionUnit => super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::Timer(timer) => ExitCode::from(timer.run(arguments, &super::agents::claude_access())),
        }
    }
}

fn named(table: &'static [(&'static str, Run)], command: &str) -> Option<&'static str> {
    table.iter().find(|(name, _)| *name == command).map(|(name, _)| *name)
}


fn call_check(arguments: &[String]) -> u8 {
    super::call_check::run(arguments, defines)
}

/// Whether one more worker session may start on the subscription account `account` now, by the spend view pace admission reads.
fn subscription_admitted(account: &str) -> Result<(), String> {
    let fleet = super::fleet::open_fleet().map_err(|error| error.user_message())?;
    let view = super::fleet::spend_now(&fleet, &fleet.roster())?;
    view.admit(&format!("claude:{account}"), north_pace::spend::Starter::Worker).map_err(|refusal| refusal.to_string())
}

const CHECK: &[(&str, Run)] = &[
    ("calls", call_check),
    ("concerns", concerns),
    ("harness", agent_config_check::run),
    ("disclosure", disclosure_judge),
    ("oneshot", |arguments| crate::timers::ask::run(arguments, &super::agents::claude_access(), &subscription_admitted)),
    ("experiments", crate::experiments::run),
    ("issue-boxes", issue_box_lint::run),
    ("nix", super::nix::doctor),
    ("provenance", provenance::run),
];

fn disclosure_judge(arguments: &[String]) -> u8 {
    let access = super::agents::claude_access();
    north_land_scans::disclosure_judge::run(arguments, &|tier, prompt| {
        north_timers::judge_model::ask_subscription(&north_timers::judge_model::Model { model: tier.model, effort: tier.effort, system: north_land_scans::disclosure_judge::SYSTEM }, prompt, &access)
    })
}

fn concerns(arguments: &[String]) -> u8 {
    north_concerns::run(arguments, &super::concern_history::LedgerHistory)
}

fn check(arguments: &[String]) -> u8 {
    match arguments.split_first().and_then(|(verb, rest)| runner(CHECK, verb).map(|run| run(rest))) {
        Some(code) => code,
        None => {
            let verbs: Vec<&str> = CHECK.iter().map(|(verb, _)| *verb).collect();
            eprintln!("usage: north check ({}) ...; north check --help lists each", verbs.join("|"));
            2
        }
    }
}

pub(super) const BUILT_IN: &[&str] = &["config", "desktop-idle-state", "guard", "help"];

pub(super) fn defines(command: &str) -> bool {
    BUILT_IN.contains(&command) || super::forwarded(command, Vec::new()).is_some()
}


fn reclaim_disk() -> Option<f64> {
    north_offload::offload_cache::reclaim(north_reclaim::reclaim::before_disk_defer_here)
}

fn keep_disk() {
    north_reclaim::reclaim::sweep_here();
    north_offload::offload_cache::keep();
}

fn config(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((noun, rest)) if noun == "context" => crate::context_audit::run(rest, agent_config_check::startup_tokens),
        Some((noun, rest)) if noun == "coordinator" => crate::coordinator::run(rest, &super::verbs::VERB_KINDS, crate::guard::coordinator_table::run),
        _ => 2,
    }
}

const TEST: &[(&str, Run)] = &[
    ("coverage", north_coverage::run),
    ("mutant", |arguments| north_mutants::cli::run(arguments, &super::mutant_issues::MirrorIssues, crate::land::scans::test_admission::contract_tests)),
];

fn work(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "split" => work_split(rest),
        _ => 2,
    }
}

fn capacity_host() -> capacity::effects::Host {
    capacity::effects::Host { serving: crate::pacing::placement::serving_host, warcraft_clients: crate::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::pacing::placement::local_bar1_margin_mib, gpu_slots: crate::pacing::placement::local_gpu_slots, offload: north_offload::land_offload::run, stage_runtime: north_runtime::runtime::stage_on, reenter_outside: crate::worker_sandbox::reenter_outside, build_offload: north_offload::build_offload::offload, hand_over: north_offload::build_offload::hand_over, lease_builds: crate::pacing::placement::local_lease_builds, build_host: north_offload::land_offload::builds_for_peers, session_builds: crate::pacing::placement::local_session_builds, reclaim_disk, tend_disk: north_offload::offload_cache::tend, keep_disk, tend_slots: north_offload::land_offload::warm::tend, run_lease: north_capacity_host::capacity::host::run }
}

fn file_child(arguments: &[String]) -> u8 {
    let request: Vec<String> = ["issue", "child"].into_iter().map(str::to_owned).chain(arguments.iter().cloned()).collect();
    let outside = crate::platform::session_scope::confined_session().then(|| crate::worker_sandbox::reenter_outside(&request)).flatten();
    outside.unwrap_or_else(|| crate::issue_child::run(arguments))
}
