use std::process::ExitCode;

use crate::{autoland_status, capacity, dep_gate, follows_release, issue_box_lint, pace, provenance};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tool {
    DepGate,
    AutolandStatus,
    Provenance,
    IssueBoxLint,
    FollowsRelease,
    Pace,
    Capacity,
}

impl Tool {
    pub(super) fn named(command: &str) -> Option<Self> {
        Some(match command {
            "dep-gate" => Self::DepGate,
            "autoland-status" => Self::AutolandStatus,
            "provenance" => Self::Provenance,
            "issue-box-lint" => Self::IssueBoxLint,
            "follows-release" => Self::FollowsRelease,
            "pace" => Self::Pace,
            "capacity" => Self::Capacity,
            _ => return None,
        })
    }

    pub(super) fn run(self, arguments: &[String]) -> ExitCode {
        let run = match self {
            Self::DepGate => dep_gate::run,
            Self::AutolandStatus => autoland_status::run,
            Self::Provenance => provenance::run,
            Self::IssueBoxLint => issue_box_lint::run,
            Self::FollowsRelease => follows_release::run,
            Self::Pace => return ExitCode::from(pace::run(arguments)),
            Self::Capacity => return ExitCode::from(capacity::run(arguments)),
        };
        super::in_runtime(async move { Ok(ExitCode::from(run(arguments))) })
    }
}
