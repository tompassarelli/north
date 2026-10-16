use serde::{Deserialize, Serialize};
use serde_json::Value;

mod gather;
pub(crate) mod parse;

pub(crate) use gather::{Sources, gather};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Place {
    Laptop,
    Nexus,
    HostedFarm,
    AnthropicCloud,
}

impl Place {
    pub(crate) const ALL: [Place; 4] = [Place::Laptop, Place::Nexus, Place::HostedFarm, Place::AnthropicCloud];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Place::Laptop => "laptop",
            Place::Nexus => "nexus",
            Place::HostedFarm => "hosted farm",
            Place::AnthropicCloud => "Anthropic cloud",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct SessionFact {
    pub name: String,
    pub account: String,
    pub live: bool,
    pub working: bool,
    pub created_at: i64,
    #[serde(default)]
    pub host_dir: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct CapacityRun {
    pub place: Place,
    pub at: i64,
    pub owner: String,
    pub cpu_seconds: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct CloudSession {
    pub id: String,
    pub status: String,
    pub placed_at: i64,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct FarmRun {
    pub repo: String,
    pub id: u64,
    pub created_at: i64,
    pub job_seconds: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct LockHolder {
    pub pid: u32,
    pub dir: String,
    pub session: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Waiter {
    pub session: Option<String>,
    pub branch: String,
    pub since: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct LandingLock {
    pub repo: String,
    pub holder: Option<LockHolder>,
    pub held_since: i64,
    pub waiters: Vec<Waiter>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Landing {
    pub at: i64,
    pub repo: String,
    pub sha: String,
    pub committed_at: Option<i64>,
    pub issue_refs: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum IssueKind {
    Work,
    Epic,
    TomOnly,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct ReleaseIssue {
    pub number: u32,
    pub open: bool,
    pub closed_at: Option<i64>,
    pub kind: IssueKind,
    #[serde(default)]
    pub size: Option<Size>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct BetIssue {
    pub reference: String,
    pub title: String,
    pub open: bool,
    #[serde(default)]
    pub closed_at: Option<i64>,
    #[serde(default)]
    pub size: Option<Size>,
    pub dividends: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    pub cores: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Facts {
    pub now: i64,
    pub pace: Option<Value>,
    pub sessions: Vec<SessionFact>,
    pub runs: Vec<CapacityRun>,
    pub cloud: Option<Vec<CloudSession>>,
    pub farm: Option<Vec<FarmRun>>,
    pub locks: Vec<LandingLock>,
    pub landings: Vec<Landing>,
    pub release: Option<Vec<ReleaseIssue>>,
    pub builds: Vec<Build>,
    pub load: Option<Load>,
    pub bet_issues: Option<Vec<BetIssue>>,
    pub limit_stops: Vec<LimitStop>,
    pub checks: Vec<i64>,
    pub nexus: bool,
    pub gaps: Vec<String>,
}

pub(crate) const DAY: i64 = 86_400;
pub(crate) const WEEK: i64 = 7 * DAY;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Size {
    S,
    M,
    L,
}

impl Size {
    pub(crate) fn points(self) -> f64 {
        match self {
            Size::S => 1.0,
            Size::M => 2.0,
            Size::L => 4.0,
        }
    }

    pub(crate) fn for_eta_minutes(minutes: u32) -> Self {
        match minutes {
            0..=60 => Size::S,
            61..=180 => Size::M,
            _ => Size::L,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Size::S => "size:S",
            Size::M => "size:M",
            Size::L => "size:L",
        }
    }

    pub(crate) fn from_labels<'a>(mut labels: impl Iterator<Item = &'a str>) -> Option<Self> {
        labels.find_map(|label| match label {
            "size:S" => Some(Size::S),
            "size:M" => Some(Size::M),
            "size:L" => Some(Size::L),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct LimitStop {
    pub session: String,
    pub account: String,
    pub resets_at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Build {
    pub at: i64,
    pub land_to_playable_seconds: f64,
}
