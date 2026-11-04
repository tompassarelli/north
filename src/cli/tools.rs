use std::process::ExitCode;

use crate::landing::{after_landing, bin_references, denylist, fails_first, land_actions, land_cache, merge_set, red_check, land_test_runner, landed_tree, lane_squash, lane_target, mutant_cli, safe_push_tag, stale_revert};
use crate::ops::timers::{self, Timer};
use crate::{
    agent_config_check, agent_policy_contract, autoland_status, capacity, dep_gate, follows_release, issue_box_lint,
    land_abandon, provenance,
};

type Run = fn(&[String]) -> u8;

const UNIFORM: &[(&str, Run)] = &[
    ("agent-config-check", agent_config_check::run),
    ("agent-policy-contract", agent_policy_contract::run),
    ("dep-gate", dep_gate::run),
    ("evidence", crate::ops::reclaim::evidence_start),
    ("denylist", denylist::run),
    ("autoland-status", autoland_status::run),
    ("autoland-wait", autoland_status::wait),
    ("land-abandon", |arguments| land_abandon::abandon(arguments, &super::abandon_witness::FleetWitness)),
    ("land", crate::land::run),
    ("land-clean", lane_target::run),
    ("land-train", |arguments| ledger_verb("land-train", arguments)),
    ("incident", |arguments| ledger_verb("incident", arguments)),
    ("landed-tree", landed_tree::run),
    ("stale-revert", stale_revert::run),
    ("lane-squash", lane_squash::run),
    ("merge-driver", merge_set::run),
    ("after-landing", after_landing::run),
    ("bin-references", bin_references::run),
    ("land-tests", land_test_runner::run),
    ("land-actions", land_actions::run),
    ("land-red-check", red_check::run),
    ("land-red-wait", red_check::wait),
    ("land-cache", land_cache::run),
    ("safe-push-result", super::safe_push_result::run),
    ("watch", crate::ops::watch::run),
    ("safe-push", safe_push_tag::run),
    ("provenance", provenance::run),
    ("issue-box-lint", issue_box_lint::run),
    ("follows-release", follows_release::run),
    ("citation-check", citation_check),
    ("cloud", super::cloud::run),
    ("worktree-layout-check", crate::lanes::layout_check),
    ("hardcoded-repo-path-check", crate::lanes::hardcoded_paths),
    ("lane-gc", crate::lanes::lane_gc),
    ("target-seed", crate::lanes::target_seed),
    ("pin-retire", crate::lanes::pin_retire),
    ("wt-reap", crate::lanes::wt_reap),
    ("wt-rescue", crate::lanes::wt_rescue),
    ("mutant", mutant_cli::run),
    ("land-fails-first", fails_first::run),
    ("flow", crate::ops::flow::run),
    ("experiments", crate::ops::experiments::run),
    ("report", crate::ops::report::run),
    ("agents", super::agents::run),
    ("resident-cap", crate::ops::resident_cap::run),
    ("docs", crate::ops::docs::run),
    ("usage", super::agents::usage),
    ("convo", crate::ops::convo::run),
    ("incident-mine", crate::ops::incident_mine::run),
    ("policy-replay", crate::ops::policy_replay::run),
    ("defaults-measure", crate::ops::defaults_measure::run),
    ("context", |arguments| crate::ops::context_audit::run(arguments, agent_config_check::startup_tokens)),
    ("worker-ledger", super::worker_ledger::run),
    ("brief", crate::guard::brief_skeleton::run),
    ("proxy", |arguments| crate::proxy::run(arguments, &super::verbs::VERB_KINDS)),
    ("ops", crate::ops::run),
    ("private-check", crate::ops::private_check::run),
    ("watch", crate::watch::run),
    ("ssh-argv", crate::platform::ssh::run),
    ("provider-capacity", crate::ops::provider_capacity::run),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Uniform(&'static str),
    Wisp,
    Pace,
    Capacity,
    NexusRam,
    Laptop,
    LaptopRunner,
    LaptopNtfy,
    AgentContain,
    Reclaim,
    Retain,
    SessionUnit,
    Runtime,
    Dispatch,
    Tune,
    Archives,
    CodeHealth,
    SecretsProxyExec,
    NexusRun,
    Timer(Timer),
}

const OPERATIONS: &[(&str, Tool)] = &[
    ("wisp", Tool::Wisp),
    ("pace", Tool::Pace),
    ("capacity", Tool::Capacity),
    ("nexus-ram", Tool::NexusRam),
    ("laptop", Tool::Laptop),
    ("laptop-runner", Tool::LaptopRunner),
    ("laptop-ntfy", Tool::LaptopNtfy),
    ("agent-contain", Tool::AgentContain),
    ("reclaim", Tool::Reclaim),
    ("retain", Tool::Retain),
    ("session-unit", Tool::SessionUnit),
    ("runtime", Tool::Runtime),
    ("dispatch", Tool::Dispatch),
    ("tune", Tool::Tune),
    ("archives", Tool::Archives),
    ("code-health", Tool::CodeHealth),
    ("secrets-proxy-exec", Tool::SecretsProxyExec),
    ("nexus-run", Tool::NexusRun),
];

impl Tool {
    pub(super) fn named(command: &str) -> Option<Self> {
        UNIFORM.iter().find(|(name, _)| *name == command).map(|(name, _)| Self::Uniform(name)).or_else(|| Self::operations(command))
    }

    fn operations(command: &str) -> Option<Self> {
        OPERATIONS.iter().find(|(name, _)| *name == command).map(|(_, tool)| *tool).or_else(|| Timer::named(command).map(Self::Timer))
    }

    pub(super) fn run(self, arguments: &[String]) -> ExitCode {
        match self {
            Self::Uniform(name) => {
                let run = UNIFORM.iter().find(|(known, _)| *known == name).map(|(_, run)| *run);
                super::in_runtime(async move { Ok(ExitCode::from(run.map_or(2, |run| run(arguments)))) })
            }
            Self::Wisp => super::in_runtime(async move {
                crate::platform::wisp_payoff::run(arguments).map(|()| ExitCode::SUCCESS)
            }),
            Self::Dispatch => ExitCode::from(crate::ops::dispatch::run(arguments, super::fleet::roster)),
            Self::Pace => ExitCode::from(crate::ops::pacing::run(arguments, crate::ops::pacing::FleetFacts { roster: super::fleet::roster, machine_load: super::fleet::fleet_load }, super::token_usage::read)),
            Self::Capacity => ExitCode::from(capacity::run(arguments, capacity::Host { serving: crate::ops::pacing::placement::serving_host, warcraft_clients: crate::ops::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::ops::pacing::placement::local_bar1_margin_mib, offload: crate::ops::land_offload::run, build_offload: crate::ops::build_offload::offload, lease_builds: crate::ops::pacing::placement::local_lease_builds, reclaim_disk: crate::ops::reclaim::before_disk_defer_here })),
            Self::NexusRam => ExitCode::from(crate::ops::nexus_ram::run(arguments)),
            Self::Laptop => ExitCode::from(crate::ops::laptop::run(arguments)),
            Self::LaptopRunner => ExitCode::from(crate::ops::laptop_runner::run(arguments)),
            Self::LaptopNtfy => ExitCode::from(crate::ops::laptop_ntfy::run(arguments)),
            Self::AgentContain => ExitCode::from(crate::ops::agent_contain::run(arguments)),
            Self::Reclaim => ExitCode::from(crate::ops::reclaim::run(arguments)),
            Self::Retain => ExitCode::from(crate::ops::retention::run(arguments)),
            Self::SessionUnit => super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::Runtime => ExitCode::from(crate::ops::runtime::run(arguments)),
            Self::Tune => ExitCode::from(crate::ops::tune::run(arguments)),
            Self::Archives => ExitCode::from(crate::platform::archive_tags::run(arguments)),
            Self::CodeHealth => ExitCode::from(match arguments.first().map(String::as_str) {
                Some("build-time") => crate::build_time::run(&arguments[1..]),
                _ => crate::code_health::run(arguments),
            }),
            Self::SecretsProxyExec => super::in_runtime(async move { Ok(ExitCode::from(crate::platform::secrets_proxy::run(arguments).await)) }),
            Self::NexusRun => super::in_runtime(async move { Ok(ExitCode::from(timers::nexus_run(arguments).await)) }),
            Self::Timer(timer) => ExitCode::from(timer.run(arguments, &super::agents::claude_access())),
        }
    }
}

fn ledger_verb(verb: &str, arguments: &[String]) -> u8 {
    let words: Vec<String> = std::iter::once(verb.to_owned()).chain(arguments.iter().cloned()).collect();
    crate::threads::client::run(&words, std::sync::Arc::new(crate::threads::outside::Unread))
}

fn citation_check(arguments: &[String]) -> u8 {
    crate::ops::citation_check::run(arguments, defines)
}

pub(super) const BUILT_IN: &[&str] = &["config", "desktop-idle-state", "guard", "help", "private-desktop"];

pub(super) fn defines(command: &str) -> bool {
    BUILT_IN.contains(&command) || super::forwarded(command, Vec::new()).is_some()
}

#[cfg(test)]
pub(super) fn verbs() -> impl Iterator<Item = &'static str> {
    UNIFORM.iter().map(|(name, _)| *name)
        .chain(OPERATIONS.iter().map(|(name, _)| *name))
        .chain(Timer::NAMES.iter().map(|(name, _)| *name))
        .chain(super::session::NAMES.iter().map(|(name, _)| *name))
        .chain(super::FORWARDED.iter().map(|(name, _)| *name))
        .chain(BUILT_IN.iter().copied())
}
