use serde::{Deserialize, Serialize};

pub const RESULTS: &str = "docs/rationale/frontier/results.jsonl";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arm {
    North,
    Plain,
    #[serde(rename = "t3code")]
    T3Code,
}

/// One arm's run of one frozen task, as the grader scored it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub task: String,
    pub arm: Arm,
    pub model: String,
    pub effort: String,
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

impl Arm {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "north" => Some(Self::North),
            "plain" => Some(Self::Plain),
            "t3code" => Some(Self::T3Code),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::Plain => "plain",
            Self::T3Code => "t3code",
        }
    }
}
