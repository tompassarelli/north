use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::decide::world::Label;
use super::super::decide::world::{
    EstimateSource, Expected, ModelFamily, Name, Owner, Session, Usd, Workstream,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Launch {
    pub tool_use_id: String,
    pub name: String,
    pub account: String,
    pub family: String,
    pub workstream: Option<String>,
    pub p50: f64,
    pub p90: f64,
    pub minutes: f64,
}

impl Launch {
    pub fn session(&self) -> Option<Session> {
        Some(Session {
            name: Name(self.name.clone()),
            owner: Owner::AgentTool {
                tool_use_id: self.tool_use_id.clone(),
            },
            account: Label(self.account.clone()),
            family: family(&self.family)?,
            workstream: self.workstream.clone().map(Workstream),
            box_id: None,
            lead: false,
            spent: None,
            active_minutes: 0.0,
            expected: Expected {
                p50: Usd(self.p50),
                p90: Usd(self.p90),
                minutes: self.minutes,
                source: EstimateSource::None,
            },
            parked_since: None,
        })
    }
}

pub fn family(text: &str) -> Option<ModelFamily> {
    let text = text.to_ascii_lowercase();
    [
        ("fable", ModelFamily::Fable),
        ("opus", ModelFamily::Opus),
        ("sonnet", ModelFamily::Sonnet),
        ("haiku", ModelFamily::Haiku),
    ]
    .into_iter()
    .find_map(|(name, family)| text.contains(name).then_some(family))
}

pub struct LaunchLog {
    path: PathBuf,
}

impl LaunchLog {
    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }

    pub fn read(&self) -> Vec<Launch> {
        fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub fn add(&self, launch: &Launch) -> Result<(), String> {
        let line = serde_json::to_string(launch).map_err(|error| error.to_string())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        writeln!(file, "{line}").map_err(|error| format!("{}: {error}", self.path.display()))
    }

    pub fn remove(&self, tool_use_id: &str) -> Result<(), String> {
        let kept: Vec<String> = self
            .read()
            .into_iter()
            .filter(|launch| launch.tool_use_id != tool_use_id)
            .filter_map(|launch| serde_json::to_string(&launch).ok())
            .collect();
        let text = if kept.is_empty() {
            String::new()
        } else {
            kept.join("\n") + "\n"
        };
        fs::write(&self.path, text).map_err(|error| format!("{}: {error}", self.path.display()))
    }
}
