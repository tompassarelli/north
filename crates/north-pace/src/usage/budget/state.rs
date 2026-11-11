use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::assess::{Assessment, Scope, Spend};

/// What budget notices remember between ticks: each scope's last assessments, since a condition must hold at two
/// ticks to change one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub scopes: BTreeMap<String, Remembered>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remembered {
    pub raw: Assessment,
    pub confirmed: Assessment,
    pub spent: f64,
    pub p50: f64,
    pub p90: f64,
}

pub fn key(scope: &Scope) -> String {
    match scope {
        Scope::Box(id) => format!("box:{id}"),
        Scope::Workstream(workstream) => format!("workstream:{workstream}"),
    }
}

fn scope_of(key: &str) -> Option<Scope> {
    match key.split_once(':')? {
        ("box", id) => Some(Scope::Box(id.to_owned())),
        ("workstream", name) => Some(Scope::Workstream(name.to_owned())),
        _ => None,
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
        path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&temporary, text)).and_then(|()| fs::rename(&temporary, path)).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn spends(&self) -> BTreeMap<Scope, Spend> {
        self.scopes.iter().filter_map(|(key, seen)| Some((scope_of(key)?, Spend { raw: seen.raw, confirmed: seen.confirmed, changed: false, spent: seen.spent, p50: seen.p50, p90: seen.p90 }))).collect()
    }

    pub fn remembering(spends: &BTreeMap<Scope, Spend>) -> Self {
        Self { scopes: spends.iter().map(|(scope, spend)| (key(scope), Remembered { raw: spend.raw, confirmed: spend.confirmed, spent: spend.spent, p50: spend.p50, p90: spend.p90 })).collect() }
    }
}
