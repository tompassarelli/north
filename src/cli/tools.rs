use std::process::ExitCode;

use crate::landing::{denylist, land_test_runner, lane_target, mutant_cli, safe_push_tag};
use crate::{
    autoland_status, capacity, dep_gate, follows_release, issue_box_lint, land_abandon, provenance,
    review_gate,
};

type Run = fn(&[String]) -> u8;

const UNIFORM: &[(&str, Run)] = &[
    ("dep-gate", dep_gate::run),
    ("review-gate", review_gate::run),
    ("denylist", denylist::run),
    ("autoland-status", autoland_status::run),
    ("autoland-wait", autoland_status::wait),
    ("land-abandon", land_abandon::abandon),
    ("land-clean", lane_target::run),
    ("land-tests", land_test_runner::run),
    ("safe-push-result", super::safe_push_result::run),
    ("safe-push", safe_push_tag::run),
    ("provenance", provenance::run),
    ("issue-box-lint", issue_box_lint::run),
    ("follows-release", follows_release::run),
    ("cloud", super::cloud::run),
    ("worktree-layout-check", crate::lanes::layout_check),
    ("hardcoded-repo-path-check", crate::lanes::hardcoded_paths),
    ("mutant", mutant_cli::run),
    ("flow", crate::ops::flow::run),
    ("report", crate::ops::report::run),
    ("agents", super::agents::run),
    ("resident-cap", crate::ops::resident_cap::run),
    ("usage", super::agents::usage),
    ("convo", crate::ops::convo::run),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Uniform(&'static str),
    Wisp,
    Pace,
    Capacity,
    NexusRam,
    Reclaim,
    SessionUnit,
    Runtime,
    Dispatch,
    Tune,
    IncidentMine,
    Archives,
    CodeHealth,
}

impl Tool {
    pub(super) fn named(command: &str) -> Option<Self> {
        UNIFORM.iter().find(|(name, _)| *name == command).map(|(name, _)| Self::Uniform(name)).or_else(|| Self::operations(command))
    }

    fn operations(command: &str) -> Option<Self> {
        Some(match command {
            "wisp" => Self::Wisp,
            "pace" => Self::Pace,
            "capacity" => Self::Capacity,
            "nexus-ram" => Self::NexusRam,
            "reclaim" => Self::Reclaim,
            "session-unit" => Self::SessionUnit,
            "runtime" => Self::Runtime,
            "dispatch" => Self::Dispatch,
            "tune" => Self::Tune,
            "incident-mine" => Self::IncidentMine,
            "archives" => Self::Archives,
            "code-health" => Self::CodeHealth,
            _ => return None,
        })
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
            Self::Pace => ExitCode::from(crate::ops::pacing::run(arguments, crate::ops::pacing::FleetFacts { roster: super::fleet::roster, machine_load: super::fleet_placement::fleet_load }, super::token_usage::read)),
            Self::Capacity => ExitCode::from(capacity::run(arguments, capacity::Host { serving: crate::ops::pacing::placement::serving_host, warcraft_clients: crate::ops::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::ops::pacing::placement::local_bar1_margin_mib, offload: crate::ops::land_offload::run })),
            Self::NexusRam => ExitCode::from(crate::ops::nexus_ram::run(arguments)),
            Self::Reclaim => ExitCode::from(crate::ops::reclaim::run(arguments)),
            Self::SessionUnit => super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::Runtime => ExitCode::from(crate::ops::runtime::run(arguments)),
            Self::Tune => ExitCode::from(crate::ops::tune::run(arguments)),
            Self::IncidentMine => ExitCode::from(crate::ops::incident_mine::run(arguments)),
            Self::Archives => ExitCode::from(crate::platform::archive_tags::run(arguments)),
            Self::CodeHealth => ExitCode::from(crate::code_health::run(arguments)),
        }
    }
}
