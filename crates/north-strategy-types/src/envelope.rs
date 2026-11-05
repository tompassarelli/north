use alloc::string::String;
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

/// One line of the decision log: who decided what, when, from which case, and what recording cost.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope<C, D> {
    pub v: u32,
    pub kind: Kind,
    pub id: String,
    pub at: f64,
    pub host: String,
    pub strategy: String,
    pub follows: Option<String>,
    pub case: C,
    pub decision: D,
    pub decide_us: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u64,
}

fn is_zero(count: &u64) -> bool {
    *count == 0
}

/// One line of an admission snapshot file: the host part of the facts, keyed by its hash.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot<S> {
    pub v: u32,
    pub hash: String,
    pub at: f64,
    pub host: String,
    pub snapshot: S,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Kind {
    #[serde(rename = "placement")]
    Placement,
    #[serde(rename = "admission")]
    Admission,
    #[serde(rename = "land.pick")]
    LandPick,
    #[serde(rename = "land.batch")]
    LandBatch,
    #[serde(rename = "land.judge")]
    LandJudge,
    #[serde(rename = "land.wave")]
    LandWave,
    #[serde(rename = "land.sheriff")]
    LandSheriff,
}

impl Kind {
    pub const ALL: [Self; 7] = [Self::Placement, Self::Admission, Self::LandPick, Self::LandBatch, Self::LandJudge, Self::LandWave, Self::LandSheriff];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Admission => "admission",
            Self::LandPick => "land.pick",
            Self::LandBatch => "land.batch",
            Self::LandJudge => "land.judge",
            Self::LandWave => "land.wave",
            Self::LandSheriff => "land.sheriff",
        }
    }
}
