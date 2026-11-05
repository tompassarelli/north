use std::process::ExitCode;

use crate::landing::{after_landing, bin_references, denylist, fails_first, land_actions, land_cache, merge_set, red_check, land_test_runner, landed_tree, lane_squash, lane_target, mutant_cli, stale_revert};
use crate::timers::{self, Timer};
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
    ("safe-push", super::remote_land::safe_push),
    ("release", crate::landing::release::run),
    ("remote-land", super::remote_land::run),
    ("provenance", provenance::run),
    ("issue-box-lint", issue_box_lint::run),
    ("follows-release", follows_release::run),
    ("citation-check", citation_check),
    ("job-run", crate::ops::job_run::run),
    ("run-bounded", crate::ops::run_bounded::run),
    ("review-items", review_items),
    ("cleanup-sweep", crate::ops::cleanup_sweep::run),
    ("cloud", super::cloud::run),
    ("worktree-layout-check", crate::lanes::layout_check),
    ("hardcoded-repo-path-check", crate::lanes::hardcoded_paths),
    ("lane-gc", crate::lanes::lane_gc),
    ("repo", crate::lanes::repo_verb),
    ("target-seed", crate::lanes::target_seed),
    ("pin-retire", crate::lanes::pin_retire),
    ("wt-reap", crate::lanes::wt_reap),
    ("wt-rescue", crate::lanes::wt_rescue),
    ("mutant", |arguments| mutant_cli::run(arguments, &super::mutant_issues::MirrorIssues)),
    ("land-fails-first", fails_first::run),
    ("flow", crate::ops::flow::run),
    ("experiments", crate::ops::experiments::run),
    ("tempo", crate::ops::tempo::run),
    ("report", crate::ops::report::run),
    ("agents", super::agents::run),
    ("resident-cap", crate::resident_cap::run),
    ("split", crate::split::run),
    ("docs", crate::ops::docs::run),
    ("usage", super::agents::usage),
    ("convo", crate::ops::convo::run),
    ("warcraft-close", crate::ops::warcraft_close::run),
    ("wc3-login-field", crate::ops::wc3_login_field::run),
    ("agent-capture", crate::ops::agent_capture::run),
    ("claude-runtime-update", crate::ops::vendor_runtime::run_claude),
    ("agy-runtime-update", crate::ops::vendor_runtime::run_agy),
    ("incident-mine", crate::ops::incident_mine::run),
    ("tool-mine", crate::ops::tool_mine::run),
    ("policy-replay", crate::ops::policy_replay::run),
    ("defaults-measure", crate::ops::defaults_measure::run),
    ("context", |arguments| crate::ops::context_audit::run(arguments, agent_config_check::startup_tokens)),
    ("worker-ledger", super::worker_ledger::run),
    ("brief", crate::guard::brief_skeleton::run),
    ("proxy", |arguments| match arguments.first().map(String::as_str) {
        Some("calls") => crate::proxy::run(arguments, &super::verbs::VERB_KINDS),
        _ => super::fleet::proxy(arguments),
    }),
    ("ops", crate::ops::command::run),
    ("private-check", crate::ops::private_check::run),
    ("watch", crate::watch::run),
    ("ssh-argv", crate::platform::ssh::run),
    ("provider-capacity", crate::capacity::provider_capacity::run),
    ("gh-app-token", crate::platform::github_app::run),
    ("github-mirror", crate::github_mirror::sync::run),
    ("gh-session", crate::platform::gh_session::run),
    ("git-credential", crate::platform::git_credential::run),
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
    ("reclaim", crate::ops::reclaim::run),
    ("retain", crate::ops::retention::run),
    ("runtime", crate::runtime::run),
    ("tune", crate::tune::run),
    ("loops", crate::loops::run),
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
            Self::Capacity => ExitCode::from(capacity::command::run(arguments, capacity::effects::Host { serving: crate::pacing::placement::serving_host, warcraft_clients: crate::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::pacing::placement::local_bar1_margin_mib, offload: crate::land_offload::run, build_offload: crate::build_offload::offload, lease_builds: crate::pacing::placement::local_lease_builds, reclaim_disk: crate::ops::reclaim::before_disk_defer_here })),
            Self::SessionUnit => super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::NexusRun => super::in_runtime(async move { Ok(ExitCode::from(timers::nexus_run(arguments).await)) }),
            Self::Timer(timer) => ExitCode::from(timer.run(arguments, &super::agents::claude_access())),
        }
    }
}

fn named(table: &'static [(&'static str, Run)], command: &str) -> Option<&'static str> {
    table.iter().find(|(name, _)| *name == command).map(|(name, _)| *name)
}

fn runner(table: &[(&str, Run)], name: &str) -> Option<Run> {
    table.iter().find(|(known, _)| *known == name).map(|(_, run)| *run)
}

fn code_health(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("build-time") => crate::build_time::run(&arguments[1..]),
        _ => crate::code_health::run(arguments),
    }
}

fn ledger_verb(verb: &str, arguments: &[String]) -> u8 {
    let words: Vec<String> = std::iter::once(verb.to_owned()).chain(arguments.iter().cloned()).collect();
    crate::threads::client::run(&words, std::sync::Arc::new(crate::threads::outside::Unread))
}

fn citation_check(arguments: &[String]) -> u8 {
    crate::ops::citation_check::run(arguments, defines)
}

fn review_items(arguments: &[String]) -> u8 {
    crate::ops::review_items::run(arguments, defines)
}

pub(super) const BUILT_IN: &[&str] = &["config", "desktop-idle-state", "guard", "help", "private-desktop"];

pub(super) fn defines(command: &str) -> bool {
    BUILT_IN.contains(&command) || super::forwarded(command, Vec::new()).is_some()
}

#[cfg(test)]
pub(super) fn verbs() -> impl Iterator<Item = &'static str> {
    UNIFORM.iter().map(|(name, _)| *name)
        .chain(DIRECT.iter().map(|(name, _)| *name))
        .chain(OPERATIONS.iter().map(|(name, _)| *name))
        .chain(Timer::NAMES.iter().map(|(name, _)| *name))
        .chain(super::session::NAMES.iter().map(|(name, _)| *name))
        .chain(super::FORWARDED.iter().map(|(name, _)| *name))
        .chain(BUILT_IN.iter().copied())
}
