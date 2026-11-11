use north_base::foundation::clock::Timestamp;
use serde::{Deserialize, Serialize};

use super::Row;
use super::trend::Point;
use super::verdict::Verdict;

pub const SAMPLER: &str = "concerns-tick";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Green,
    Drifting,
    Red,
    Repairing,
    Unmeasured,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Drifting => "drifting",
            Self::Red => "red",
            Self::Repairing => "repairing",
            Self::Unmeasured => "unmeasured",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sample {
    pub project: String,
    pub concern: String,
    pub sampler: &'static str,
    pub host: String,
    pub at: Timestamp,
    pub value: Option<f64>,
    pub why: String,
    pub detail: String,
    pub state: State,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recorded {
    pub project: String,
    pub concern: String,
    pub point: Point,
}

pub trait Store {
    fn record(&self, samples: &[Sample]) -> Result<(), String>;
    fn read(&self, host: &str, since: Timestamp) -> Result<Vec<Recorded>, String>;
}

pub fn points(recorded: &[Recorded], project: &str, concern: &str) -> Vec<Point> {
    recorded.iter().filter(|row| row.project == project && row.concern == concern).map(|row| row.point).collect()
}

pub fn sample(row: &Row, state: State, host: &str, at: Timestamp) -> Sample {
    let (value, why) = match &row.verdict {
        Verdict::Green(value) | Verdict::RedContract(value) | Verdict::RedDebt(value) | Verdict::RedOverCeiling(value) => (Some(*value), String::new()),
        Verdict::Unmeasured(why) => (None, why.clone()),
    };
    Sample { project: row.project.clone(), concern: row.concern.name.clone(), sampler: SAMPLER, host: host.to_owned(), at, value, why, detail: row.verdict.word().to_owned(), state }
}
