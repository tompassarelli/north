use std::process::ExitCode;

use crate::landing::{denylist, lane_target};
use crate::{
    autoland_status, capacity, dep_gate, follows_release, issue_box_lint, land_abandon, provenance,
    review_gate,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Wisp,
    DepGate,
    ReviewGate,
    Denylist,
    AutolandStatus,
    AutolandWait,
    LandAbandon,
    SafePushResult,
    Provenance,
    IssueBoxLint,
    FollowsRelease,
    WorktreeLayoutCheck,
    HardcodedRepoPathCheck,
    Pace,
    Capacity,
    NexusRam,
    Reclaim,
    SessionUnit,
    Runtime,
    Cloud,
    Dispatch,
    Tune,
    LandClean,
    Archives,
}

impl Tool {
    pub(super) fn named(command: &str) -> Option<Self> {
        Some(match command {
            "wisp" => Self::Wisp,
            "dep-gate" => Self::DepGate,
            "review-gate" => Self::ReviewGate,
            "denylist" => Self::Denylist,
            "autoland-status" => Self::AutolandStatus,
            "autoland-wait" => Self::AutolandWait,
            "land-abandon" => Self::LandAbandon,
            "safe-push-result" => Self::SafePushResult,
            "provenance" => Self::Provenance,
            "issue-box-lint" => Self::IssueBoxLint,
            "follows-release" => Self::FollowsRelease,
            "hardcoded-repo-path-check" => Self::HardcodedRepoPathCheck,
            "worktree-layout-check" => Self::WorktreeLayoutCheck,
            "pace" => Self::Pace,
            "capacity" => Self::Capacity,
            "nexus-ram" => Self::NexusRam,
            "reclaim" => Self::Reclaim,
            "session-unit" => Self::SessionUnit,
            "runtime" => Self::Runtime,
            "cloud" => Self::Cloud,
            "dispatch" => Self::Dispatch,
            "tune" => Self::Tune,
            "land-clean" => Self::LandClean,
            "archives" => Self::Archives,
            _ => return None,
        })
    }

    pub(super) fn run(self, arguments: &[String]) -> ExitCode {
        let run = match self {
            Self::Wisp => return super::in_runtime(async move {
                crate::platform::wisp_payoff::run(arguments).map(|()| ExitCode::SUCCESS)
            }),
            Self::DepGate => dep_gate::run,
            Self::ReviewGate => review_gate::run,
            Self::Denylist => denylist::run,
            Self::AutolandStatus => autoland_status::run,
            Self::AutolandWait => autoland_status::wait,
            Self::LandAbandon => land_abandon::abandon,
            Self::LandClean => lane_target::run,
            Self::SafePushResult => super::safe_push_result::run,
            Self::Provenance => provenance::run,
            Self::IssueBoxLint => issue_box_lint::run,
            Self::FollowsRelease => follows_release::run,
            Self::Cloud => super::cloud::run,
            Self::WorktreeLayoutCheck => crate::lanes::layout_check,
            Self::HardcodedRepoPathCheck => crate::lanes::hardcoded_paths,
            Self::Archives => return ExitCode::from(crate::platform::archive_tags::run(arguments)),
            Self::Dispatch => return ExitCode::from(crate::ops::dispatch::run(arguments, super::fleet::roster)),
            Self::Pace => return ExitCode::from(crate::ops::pacing::run(arguments, super::fleet::roster, super::token_usage::read)),
            Self::Capacity => return ExitCode::from(capacity::run(arguments, capacity::Host { serving: crate::ops::pacing::placement::serving_host, warcraft_clients: crate::ops::pacing::placement::local_warcraft_clients, bar1_margin_mib: crate::ops::pacing::placement::local_bar1_margin_mib, offload: crate::ops::land_offload::run })),
            Self::NexusRam => return ExitCode::from(crate::ops::nexus_ram::run(arguments)),
            Self::Reclaim => return ExitCode::from(crate::ops::reclaim::run(arguments)),
            Self::SessionUnit => return super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::Runtime => return ExitCode::from(crate::ops::runtime::run(arguments)),
            Self::Tune => return ExitCode::from(crate::ops::tune::run(arguments)),
        };
        super::in_runtime(async move { Ok(ExitCode::from(run(arguments))) })
    }
}
