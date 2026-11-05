use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::usage::decide::world::{BoxId, Usd, Workstream};
use crate::usage::decide::{Assessment, Scope, Spend};

/// What the breaker remembers between ticks: each scope's last assessments (a condition must hold at two ticks to
/// change one), the scopes whose sessions were told of a trip, when each workstream tripped, and the clears in force.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub scopes: BTreeMap<String, Remembered>,
    pub told: BTreeSet<String>,
    pub tripped_at_ms: BTreeMap<String, u64>,
    pub clears: Vec<Cleared>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remembered {
    pub raw: String,
    pub confirmed: String,
    pub spent: f64,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cleared {
    pub workstream: String,
    pub p90: f64,
    pub by: String,
}

pub fn key(scope: &Scope) -> String {
    match scope {
        Scope::Box(id) => format!("box:{}", id.0),
        Scope::Workstream(workstream) => format!("workstream:{}", workstream.0),
    }
}

pub fn workstream_key(workstream: &str) -> String {
    format!("workstream:{workstream}")
}

fn scope_of(key: &str) -> Option<Scope> {
    match key.split_once(':')? {
        ("box", id) => Some(Scope::Box(BoxId(id.to_owned()))),
        ("workstream", name) => Some(Scope::Workstream(Workstream(name.to_owned()))),
        _ => None,
    }
}

pub fn label(assessment: &Assessment) -> String {
    match assessment {
        Assessment::Unestimated => "unestimated".to_owned(),
        Assessment::Cleared { by } => format!("cleared:{by}"),
        Assessment::Tripped => "tripped".to_owned(),
        Assessment::Warn => "warn".to_owned(),
        Assessment::Velocity => "velocity".to_owned(),
        Assessment::Within => "within".to_owned(),
    }
}

fn assessment(label: &str) -> Assessment {
    match label {
        "unestimated" => Assessment::Unestimated,
        "tripped" => Assessment::Tripped,
        "warn" => Assessment::Warn,
        "velocity" => Assessment::Velocity,
        other => other.strip_prefix("cleared:").map_or(Assessment::Within, |by| Assessment::Cleared { by: by.to_owned() }),
    }
}

impl State {
    pub fn read(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, path)).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn spends(&self) -> BTreeMap<Scope, Spend> {
        self.scopes
            .iter()
            .filter_map(|(key, seen)| {
                let (raw, confirmed) = (assessment(&seen.raw), assessment(&seen.confirmed));
                Some((scope_of(key)?, Spend { raw, confirmed, changed: false, spent: Usd(seen.spent), p50: Usd(seen.p50), p90: Usd(seen.p90) }))
            })
            .collect()
    }

    pub fn remember(&mut self, spends: &BTreeMap<Scope, Spend>) {
        self.scopes = spends
            .iter()
            .map(|(scope, spend)| (key(scope), Remembered { raw: label(&spend.raw), confirmed: label(&spend.confirmed), spent: spend.spent.0, p50: spend.p50.0, p90: spend.p90.0 }))
            .collect();
    }

    pub fn tripped(&self, workstream: &str) -> Option<&Remembered> {
        self.scopes.get(&workstream_key(workstream)).filter(|seen| seen.confirmed == "tripped")
    }
}
