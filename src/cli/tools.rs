use std::process::ExitCode;

use crate::landing::{after_landing, bin_references, denylist, merge_set, landed_tree, stale_revert};
use self::graph::graph;
use self::work_split::work_split;
use self::subverbs::{Run, code_health, home, lane, land, ops, runner, runtime};
use crate::{
    agent_config_check, agent_policy_contract, autoland_status, capacity, dep_gate, issue_box_lint,
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
    ("landed-tree", landed_tree::run),
    ("stale-revert", stale_revert::run),
    ("merge-driver", merge_set::run),
    ("after-landing", after_landing::run),
    ("bin-references", bin_references::run),
    ("safe-push", super::remote_land::safe_push),
    ("hand-in", super::remote_land::hand_in),
    ("finish", super::remote_land::finish),
    ("job-run", super::job_run::run),
    ("cloud", super::cloud::run),
    ("hardcoded-repo-path-check", crate::lanes::hardcoded_paths),
    ("pin-retire", crate::lanes::pin_retire),
    ("graph", graph),
    ("docs", north_docs::docs::run),
    ("proof-run", super::proof_run::run),
    ("ops", ops),
    ("private-check", crate::private_check::run),
    ("ssh-argv", crate::platform::ssh::run),
    ("provider-capacity", crate::capacity::provider_capacity::run),
    ("gh-session", |arguments| crate::platform::gh_session::run(arguments, crate::guard::gh_screen::screen)),
    ("ssh-session", crate::platform::ssh_session::run),
    ("ssh-master", crate::platform::ssh_master::run),
    ("git-credential", crate::platform::git_credential::run),
    ("gh-broker", crate::gh_broker::run),
    ("broker", north_broker::run),
];

const DIRECT: &[(&str, Run)] = &[
    ("nexus-ram", north_capacity_host::nexus_ram::run),
    ("agent-contain", crate::agent_contain::run),
    ("retain", north_reclaim::retention::run),
    ("buck", |arguments| north_buck::run(arguments, &north_buck::Cargo { manifests: north_build_system::manifest_graph, features: north_build_system::feature_graph }, &north_buck::Sides { buck2: north_build_system::cases_buck2_runs, nextest: north_build_system::cases_nextest_lists }, &crate::platform::records_mirror::state_home())),
    ("lane", lane),
    ("home", home),
    ("runtime", runtime),
    ("code-health", code_health),
    ("infra", super::views::infra::run),
    ("depbox", |arguments| capacity::depbox::run(arguments, &capacity_host())),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Uniform(&'static str),
    Direct(&'static str),
    Capacity,
    SessionUnit,
    Config,
    Work,
    Test,
}

const OPERATIONS: &[(&str, Tool)] = &[
    ("test", Tool::Test),
    ("capacity", Tool::Capacity),
    ("session-unit", Tool::SessionUnit),
];

impl Tool {
    pub(super) fn named(command: &str) -> Option<Self> {
        named(UNIFORM, command)
            .map(Self::Uniform)
            .or_else(|| named(DIRECT, command).map(Self::Direct))
            .or_else(|| OPERATIONS.iter().find(|(name, _)| *name == command).map(|(_, tool)| *tool))
    }

    pub(super) fn run(self, arguments: &[String]) -> ExitCode {
        match self {
            Self::Uniform(name) => {
                let run = runner(UNIFORM, name);
                super::in_runtime(async move { Ok(ExitCode::from(run.map_or(2, |run| run(arguments)))) })
            }
            Self::Direct(name) => ExitCode::from(runner(DIRECT, name).map_or(2, |run| run(arguments))),
            Self::Config => super::in_runtime(async move { Ok(ExitCode::from(config(arguments))) }),
            Self::Work => super::in_runtime(async move { Ok(ExitCode::from(work(arguments))) }),
            Self::Test => match arguments.split_first() {
                Some((verb, rest)) if let Some(run) = runner(TEST, verb) => super::in_runtime(async move { Ok(ExitCode::from(run(rest))) }),
                _ => {
                    eprintln!("usage: north test (coverage|mutant) ...; north test --help lists each");
                    ExitCode::from(2)
                }
            },
            Self::Capacity => match arguments.split_first() {
                Some((verb, rest)) if verb == "bounded" => ExitCode::from(north_capacity::run_bounded::run(rest)),
                Some((verb, rest)) if verb == "dev-shell" && rest.first().is_some_and(|run| run == "run") => ExitCode::from(crate::capacity::dev_shell::command(rest, &|command| capacity::gate::lease_class(command, &capacity_host()).is_some())),
                _ => ExitCode::from(capacity::command::run(arguments, capacity_host())),
            },
            Self::SessionUnit => super::failure(crate::platform::session_scope::run(arguments).into()),
        }
    }
}

fn named(table: &'static [(&'static str, Run)], command: &str) -> Option<&'static str> {
    table.iter().find(|(name, _)| *name == command).map(|(name, _)| *name)
}


fn call_check(arguments: &[String]) -> u8 {
    super::call_check::run(arguments, defines)
}

const CHECK: &[(&str, Run)] = &[
    ("calls", call_check),
    ("concerns", concerns),
    ("harness", agent_config_check::run),
    ("disclosure", disclosure_judge),
    ("oneshot", super::oneshot::ask::run),
    ("issue-boxes", issue_box_lint::run),
    ("provenance", provenance::run),
];

fn disclosure_judge(arguments: &[String]) -> u8 {
    north_land_scans::disclosure_judge::run(arguments, &|tier, prompt| {
        super::oneshot::judge_model::ask_subscription(&super::oneshot::judge_model::Model { model: tier.model, effort: tier.effort, system: north_land_scans::disclosure_judge::SYSTEM }, prompt)
    })
}

fn concerns(arguments: &[String]) -> u8 {
    north_concerns::run(arguments, &super::concern_history::NoHistory)
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
        Some((noun, rest)) if noun == "coordinator" => crate::coordinator::run(rest, crate::guard::coordinator_table::run),
        _ => 2,
    }
}

const TEST: &[(&str, Run)] = &[
    ("coverage", north_coverage::run),
    ("mutant", |arguments| north_mutants::cli::run(arguments, &super::mutant_issues::GhIssues, crate::land::scans::test_admission::contract_tests)),
];

fn work(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "split" => work_split(rest),
        _ => 2,
    }
}

fn capacity_host() -> capacity::effects::Host {
    capacity::effects::Host { serving: |_| None, warcraft_clients: || None, bar1_margin_mib: || None, gpu_slots: || None, offload: north_offload::land_offload::run, stage_runtime: north_runtime::runtime::stage_on, reenter_outside: crate::worker_sandbox::reenter_outside, build_offload: north_offload::build_offload::offload, hand_over: north_offload::build_offload::hand_over, lease_builds: north_capacity_host::capacity::mode::leases_own_builds, build_host: north_offload::land_offload::builds_for_peers, session_builds: || crate::platform::orchestration::read().is_ok_and(|cfg| crate::platform::orchestration::builds_for_sessions(&cfg, &crate::platform::host::name())), reclaim_disk, tend_disk: north_offload::offload_cache::tend, keep_disk, tend_slots: north_offload::land_offload::warm::tend, run_lease: north_capacity_host::capacity::host::run }
}
