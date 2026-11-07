use serde::{Deserialize, Serialize};

pub const RESULTS: &str = "docs/rationale/frontier/results.jsonl";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arm {
    North,
    Plain,
}

/// One arm's run of one frozen task, as the grader scored it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub task: String,
    pub arm: Arm,
    pub correct_first_pass: bool,
    pub correct: bool,
    pub cost_usd: f64,
    pub wall_seconds: f64,
    pub interventions: u32,
    #[serde(default)]
    pub contaminated: bool,
}

pub fn parse(text: &str) -> Result<Vec<Run>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| serde_json::from_str(line).map_err(|why| format!("{RESULTS}:{}: {why}", index + 1)))
        .collect()
}
