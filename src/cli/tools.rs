use std::process::ExitCode;

use crate::landing::{after_landing, bin_references, denylist, merge_set, landed_tree, stale_revert};
use self::graph::graph;
use self::subverbs::{Run, code_health, home, lane, land, ledger_verb, ops, runner, runtime};
pub(super) use self::subverbs::ledger_runner;
use crate::timers::{self, Timer};
use crate::{
    agent_config_check, agent_policy_contract, autoland_status, capacity, dep_gate, follows_release, issue_box_lint,
    provenance,
};

mod graph;
mod subverbs;

struct HostLedger;

impl north_landing::issue_body::Ledger for HostLedger {
    fn record(&self, item: &str, body: &str) -> Result<(), String> {
        crate::threads::client::ingest(crate::threads::issue_boxes::row(item, body).as_bytes())
    }
}

const UNIFORM: &[(&str, Run)] = &[
    ("agent-policy-contract", agent_policy_contract::run),
    ("dep-gate", dep_gate::run),
    ("evidence", crate::ops::reclaim::evidence_start),
    ("denylist", denylist::run),
    ("autoland-status", autoland_status::run),
    ("check", check),
    ("land", land),
    ("cause", |arguments| ledger_verb("cause", arguments)),
    ("landed-tree", landed_tree::run),
    ("stale-revert", stale_revert::run),
    ("merge-driver", merge_set::run),
    ("after-landing", |arguments| after_landing::run(arguments, &HostLedger)),
    ("bin-references", bin_references::run),
    ("safe-push", super::remote_land::safe_push),
    ("hand-in", super::remote_land::hand_in),
    ("finish", super::remote_land::finish),
    ("follows-release", follows_release::run),
    ("job-run", crate::ops::job_run::run),
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
    ("proxy", |arguments| match arguments.first().map(String::as_str) {
        Some("calls") => {
            eprintln!("did you mean: north config proxy {}", arguments.join(" "));
            2
        }
        _ => super::fleet::proxy(arguments),
    }),
    ("ops", ops),
    ("private-check", crate::ops::private_check::run),
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
    ("gh-broker", crate::ops::gh_broker::run),
    ("broker", north_broker::run),
    ("issue", |arguments| match arguments {
        [word, rest @ ..] if word == "child" => crate::ops::issue_child::run(rest),
        _ => crate::ops::issue_convert::run(arguments),
    }),
    ("ci-diagnose", crate::ops::ci_watch::diagnose::run),
    ("red-main-notify", crate::ops::ci_watch::notify::run),
    ("main-red", crate::ops::ci_watch::sheriff::run),
];

const DIRECT: &[(&str, Run)] = &[
    ("nexus-ram", crate::ops::nexus_ram::run),
    ("laptop-runner", crate::laptop_runner::run),
    ("agent-contain", crate::agent_contain::run),
    ("retain", crate::ops::retention::run),
    ("buck", |arguments| north_buck::run(arguments, &north_buck::Cargo { manifests: north_build_system::manifest_graph, features: north_build_system::feature_graph }, &north_buck::Sides { buck2: north_build_system::cases_buck2_runs, nextest: north_build_system::cases_nextest_lists }, &crate::platform::records_mirror::state_home())),
    ("lane", lane),
    ("home", home),
    ("runtime", runtime),
    ("tune", crate::tune::run),
    ("loops", crate::loops::run),
    ("desktop", desktop),
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
            Self::Dispatch => ExitCode::from(crate::dispatch::run(arguments, super::fleet::roster)),
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
                Some((verb, rest)) if verb == "bounded" => ExitCode::from(crate::ops::run_bounded::run(rest)),
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

fn citation_check(arguments: &[String]) -> u8 {
    crate::ops::citation_check::run(arguments, defines)
}

const CHECK: &[(&str, Run)] = &[
    ("calls", call_check),
    ("citations", citation_check),
    ("concerns", concerns),
    ("harness", agent_config_check::run),
    ("privacy", |arguments| crate::privacy::run(arguments, &super::agents::claude_access())),
    ("oneshot", |arguments| crate::timers::ask::run(arguments, &super::agents::claude_access())),
    ("experiments", crate::experiments::run),
    ("issue-boxes", issue_box_lint::run),
    ("machines", super::machines::run),
    ("nix", super::nix::doctor),
    ("provenance", provenance::run),
];

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

fn review_items(arguments: &[String]) -> u8 {
    crate::ops::review_items::run(arguments, defines)
}

pub(super) const BUILT_IN: &[&str] = &["config", "desktop-idle-state", "guard", "help"];

pub(super) fn defines(command: &str) -> bool {
    BUILT_IN.contains(&command) || super::forwarded(command, Vec::new()).is_some()
}


fn reclaim_disk() -> Option<f64> {
    north_offload::offload_cache::reclaim(crate::ops::reclaim::before_disk_defer_here)
}

fn config(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((noun, rest)) if noun == "context" => crate::context_audit::run(rest, agent_config_check::startup_tokens),
        Some((noun, rest)) if noun == "proxy" => crate::proxy::run(rest, &super::verbs::VERB_KINDS, crate::guard::proxy_role::streak_limit(), crate::guard::proxy_table::run),
        _ => 2,
    }
}

const TEST: &[(&str, Run)] = &[
    ("coverage", north_coverage::run),
    ("mutant", |arguments| north_mutants::cli::run(arguments, &super::mutant_issues::MirrorIssues, crate::land::scans::test_admission::contract_tests)),
];

fn work(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "brief" => match rest.split_first() {
            Some((noun, paths)) if noun == "pack" => north_context_pack::run(paths),
            _ => crate::guard::brief_skeleton::run(rest),
        },
        Some((_, rest)) => review_items(rest),
        None => 2,
    }
}

fn desktop(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "capture" => crate::ops::agent_capture::run(rest),
        Some((verb, rest)) if verb == "warcraft" => match rest.split_first() {
            Some((action, tail)) if action == "close" => crate::ops::warcraft_close::run(tail),
            Some((action, tail)) if action == "login-field" => crate::ops::wc3_login_field::run(tail),
            _ => {
                eprintln!("usage: north desktop warcraft (close|login-field) ...");
                2
            }
        },
        _ => {
            eprintln!("usage: north desktop (capture|private) ...; north desktop --help lists each");
            2
        }
    }
}

fn capacity_host() -> capacity::effects::Host {
    capacity::effects::Host { serving: crate::pacing::placement::serving_host, warcraft_clients: crate::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::pacing::placement::local_bar1_margin_mib, gpu_slots: crate::pacing::placement::local_gpu_slots, offload: north_offload::land_offload::run, stage_runtime: north_runtime::runtime::stage_on, reenter_outside: crate::worker_sandbox::reenter_outside, build_offload: north_offload::build_offload::offload, hand_over: north_offload::build_offload::hand_over, lease_builds: crate::pacing::placement::local_lease_builds, build_host: north_offload::land_offload::builds_for_peers, own_build_refusal: north_offload::build_offload::take::own_build_refusal, session_builds: crate::pacing::placement::local_session_builds, reclaim_disk, tend_disk: north_offload::offload_cache::tend, keep_disk: north_offload::offload_cache::keep, tend_slots: north_offload::land_offload::warm::tend }
}
