use north_base::foundation::clock::Timestamp;
use serde::Serialize;

use super::Row;
use super::verdict::Verdict;

pub const SAMPLER: &str = "concerns-tick";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Green,
    Red,
    Unmeasured,
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

pub trait Store {
    fn record(&self, samples: &[Sample]) -> Result<(), String>;
}

pub fn samples(rows: &[Row], host: &str, at: Timestamp) -> Vec<Sample> {
    rows.iter()
        .map(|row| {
            let (state, value, why) = match &row.verdict {
                Verdict::Green(value) => (State::Green, Some(*value), String::new()),
                Verdict::RedContract(value) | Verdict::RedDebt(value) | Verdict::RedOverCeiling(value) => (State::Red, Some(*value), String::new()),
                Verdict::Unmeasured(why) => (State::Unmeasured, None, why.clone()),
            };
            Sample { project: row.project.clone(), concern: row.goal.name.clone(), sampler: SAMPLER, host: host.to_owned(), at, value, why, detail: row.verdict.word().to_owned(), state }
        })
        .collect()
}
