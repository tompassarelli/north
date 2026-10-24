use std::process::ExitCode;

use crate::{
    autoland_status, capacity, dep_gate, follows_release, issue_box_lint, provenance, review_gate,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    Wisp,
    DepGate,
    ReviewGate,
    AutolandStatus,
    AutolandWait,
    LandAbandon,
    SafePushResult,
    Provenance,
    IssueBoxLint,
    FollowsRelease,
    Pace,
    Capacity,
    SessionUnit,
    Runtime,
}

impl Tool {
    pub(super) fn named(command: &str) -> Option<Self> {
        Some(match command {
            "wisp" => Self::Wisp,
            "dep-gate" => Self::DepGate,
            "review-gate" => Self::ReviewGate,
            "autoland-status" => Self::AutolandStatus,
            "autoland-wait" => Self::AutolandWait,
            "land-abandon" => Self::LandAbandon,
            "safe-push-result" => Self::SafePushResult,
            "provenance" => Self::Provenance,
            "issue-box-lint" => Self::IssueBoxLint,
            "follows-release" => Self::FollowsRelease,
            "pace" => Self::Pace,
            "capacity" => Self::Capacity,
            "session-unit" => Self::SessionUnit,
            "runtime" => Self::Runtime,
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
            Self::AutolandStatus => autoland_status::run,
            Self::AutolandWait => autoland_status::wait,
            Self::LandAbandon => autoland_status::abandon,
            Self::SafePushResult => super::safe_push_result::run,
            Self::Provenance => provenance::run,
            Self::IssueBoxLint => issue_box_lint::run,
            Self::FollowsRelease => follows_release::run,
            Self::Pace => return ExitCode::from(crate::ops::pacing::run(arguments, super::fleet::roster)),
            Self::Capacity => return ExitCode::from(capacity::run(arguments)),
            Self::SessionUnit => return super::failure(crate::platform::session_scope::run(arguments).into()),
            Self::Runtime => return ExitCode::from(crate::ops::runtime::run(arguments)),
        };
        super::in_runtime(async move { Ok(ExitCode::from(run(arguments))) })
    }
}
