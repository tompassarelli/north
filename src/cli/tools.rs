use std::process::ExitCode;

use crate::landing::{after_landing, bin_references, denylist, merge_set, landed_tree, stale_revert};
use self::subverbs::{Run, code_health, graph, lane, land, ledger_verb, runner, runtime};
pub(super) use self::subverbs::ledger_runner;
use crate::timers::{self, Timer};
use crate::{
    agent_config_check, agent_policy_contract, autoland_status, capacity, dep_gate, follows_release, issue_box_lint,
    provenance,
};

mod subverbs;

const UNIFORM: &[(&str, Run)] = &[
    ("agent-config-check", agent_config_check::run),
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
    ("after-landing", after_landing::run),
    ("bin-references", bin_references::run),
    ("watch", crate::ops::watch::run),
    ("safe-push", super::remote_land::safe_push),
    ("release", north_safe_push::entry::release),
    ("follows-release", follows_release::run),
    ("job-run", crate::ops::job_run::run),
    ("run-bounded", crate::ops::run_bounded::run),
    ("review-items", review_items),
    ("cloud", super::cloud::run),
    ("hardcoded-repo-path-check", crate::lanes::hardcoded_paths),
    ("strategy", super::strategy::run),
    ("repo", crate::lanes::repo_verb),
    ("pin-retire", crate::lanes::pin_retire),
    ("mutant", |arguments| north_mutants::cli::run(arguments, &super::mutant_issues::MirrorIssues, crate::land::scans::test_admission::contract_tests)),
    ("flow", crate::flow::run),
    ("coverage", north_coverage::run),
    ("graph", graph),
    ("tempo", crate::tempo::run),
    ("report", crate::ops::report::run),
    ("agents", super::agents::run),
    ("resident-cap", crate::resident_cap::run),
    ("split", crate::split::run),
    ("docs", crate::ops::docs::run),
    ("usage", super::agents::usage),
    ("convo", crate::convo::run),
    ("warcraft-close", crate::ops::warcraft_close::run),
    ("wc3-login-field", crate::ops::wc3_login_field::run),
    ("credentials", crate::ops::credentials::run),
    ("agent-capture", crate::ops::agent_capture::run),
    ("incident-mine", crate::ops::incident_mine::run),
    ("tool-mine", crate::tool_mine::run),
    ("policy-replay", crate::policy_replay::run),
    ("defaults-measure", crate::ops::defaults_measure::run),
    ("context", |arguments| crate::context_audit::run(arguments, agent_config_check::startup_tokens)),
    ("worker-ledger", super::worker_ledger::run),
    ("spend", super::spend::run),
    ("brief", crate::guard::brief_skeleton::run),
    ("proxy", |arguments| match arguments.first().map(String::as_str) {
        Some("calls") => crate::proxy::run(arguments, &super::verbs::VERB_KINDS),
        _ => super::fleet::proxy(arguments),
    }),
    ("ops", crate::ops::command::run),
    ("private-check", crate::ops::private_check::run),
    ("privacy", |arguments| crate::privacy::run(arguments, &super::agents::claude_access())),
    ("watch", crate::watch::run),
    ("ssh-argv", crate::platform::ssh::run),
    ("provider-capacity", crate::capacity::provider_capacity::run),
    ("gh-app-token", crate::platform::github_app::run),
    ("github", crate::ops::gh_broker::github),
    ("github-mirror", crate::github_mirror::sync::run),
    ("gh-session", crate::platform::gh_session::run),
    ("ssh-session", crate::platform::ssh_session::run),
    ("ssh-master", crate::platform::ssh_master::run),
    ("git-credential", crate::platform::git_credential::run),
    ("gh-broker", crate::ops::gh_broker::run),
    ("ci-diagnose", crate::ops::ci_watch::diagnose::run),
    ("red-main-notify", crate::ops::ci_watch::notify::run),
    ("main-red", crate::ops::ci_watch::sheriff::run),
];

const DIRECT: &[(&str, Run)] = &[
    ("nexus-ram", crate::ops::nexus_ram::run),
    ("laptop", crate::laptop::run),
    ("laptop-runner", crate::laptop_runner::run),
    ("laptop-ntfy", crate::laptop_ntfy::run),
    ("agent-contain", crate::agent_contain::run),
    ("retain", crate::ops::retention::run),
    ("lane", lane),
    ("runtime", runtime),
    ("tune", crate::tune::run),
    ("loops", crate::loops::run),
    ("climb", north_climb::run),
    ("frontier", north_frontier::run),
    ("code-health", code_health),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Uniform(&'static str),
    Direct(&'static str),
    Wisp,
    Pace,
    Capacity,
    SessionUnit,
    Dispatch,
    DevShell,
    NexusRun,
    Timer(Timer),
}

const OPERATIONS: &[(&str, Tool)] = &[
    ("wisp", Tool::Wisp),
    ("pace", Tool::Pace),
    ("capacity", Tool::Capacity),
    ("session-unit", Tool::SessionUnit),
    ("dispatch", Tool::Dispatch),
    ("dev-shell", Tool::DevShell),
    ("nexus-run", Tool::NexusRun),
];

impl Tool {
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
            Self::Wisp => super::in_runtime(async move {
                crate::platform::wisp_payoff::run(arguments).map(|()| ExitCode::SUCCESS)
            }),
            Self::Dispatch => ExitCode::from(crate::dispatch::run(arguments, super::fleet::roster)),
            Self::DevShell => ExitCode::from(crate::capacity::dev_shell::command(arguments)),
            Self::Pace => ExitCode::from(crate::pacing::run(arguments, crate::pacing::FleetFacts { roster: super::fleet::roster, machine_load: super::fleet::fleet_load }, super::token_usage::read)),
            Self::Capacity => ExitCode::from(capacity::command::run(arguments, capacity::effects::Host { serving: crate::pacing::placement::serving_host, warcraft_clients: crate::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::pacing::placement::local_bar1_margin_mib, offload: crate::land_offload::run, build_offload: crate::build_offload::offload, hand_over: crate::build_offload::hand_over, lease_builds: crate::pacing::placement::local_lease_builds, own_build_refusal: crate::build_offload::take::own_build_refusal, reclaim_disk, tend_disk: crate::offload_cache::tick, tend_slots: crate::land_offload::warm::tend })),
            Self::SessionUnit => super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::NexusRun => super::in_runtime(async move { Ok(ExitCode::from(timers::nexus_run(arguments).await)) }),
            Self::Timer(timer) => ExitCode::from(timer.run(arguments, &super::agents::claude_access())),
        }
    }
}

fn named(table: &'static [(&'static str, Run)], command: &str) -> Option<&'static str> {
    table.iter().find(|(name, _)| *name == command).map(|(name, _)| *name)
}


fn citation_check(arguments: &[String]) -> u8 {
    crate::ops::citation_check::run(arguments, defines)
}

const CHECK: &[(&str, Run)] = &[
    ("citations", citation_check),
    ("experiments", crate::experiments::run),
    ("goals", goals),
    ("issue-boxes", issue_box_lint::run),
    ("nix", super::nix::doctor),
    ("provenance", provenance::run),
];

fn goals(arguments: &[String]) -> u8 {
    north_goals::run(arguments, &super::concern_history::LedgerHistory)
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

pub(super) const BUILT_IN: &[&str] = &["config", "desktop-idle-state", "guard", "help", "private-desktop"];

pub(super) fn defines(command: &str) -> bool {
    BUILT_IN.contains(&command) || super::forwarded(command, Vec::new()).is_some()
}


fn reclaim_disk() -> Option<f64> {
    crate::offload_cache::reclaim(crate::ops::reclaim::before_disk_defer_here)
}
