use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item};

use crate::foundation::clock::Timestamp;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Target {
    pub by: String,
    pub spend_to_pct: f64,
}

impl Target {
    pub(super) fn deadline(&self) -> Option<f64> {
        seconds(&self.by)
    }
}

pub(crate) fn seconds(text: &str) -> Option<f64> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds_f64)
}

pub(crate) fn policy_path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTS_ORCHESTRATION").filter(|path| !path.is_empty()) {
        return path.into();
    }
    match env::var_os("AGENTS_OPERATOR_DIR") {
        Some(operator) => PathBuf::from(operator).join("orchestration.toml"),
        None => home_policy_path(Path::new(&env::var_os("HOME").unwrap_or_default())),
    }
}

pub(crate) fn home_policy_path(home: &Path) -> PathBuf {
    home.join("code/south/main/agents/orchestration.toml")
}

fn entry(item: &Item) -> Option<Target> {
    let by = match item.get("by")? {
        Item::Value(value) => value
            .as_datetime()
            .map(|stamp| stamp.to_string())
            .or_else(|| value.as_str().map(str::to_owned))?,
        _ => return None,
    };
    let goal = item.get("spend_to_pct")?;
    let goal = goal.as_float().or_else(|| goal.as_integer().map(|whole| whole as f64))?;
    let utc = by.ends_with('Z') || by.ends_with("+00:00") || by.ends_with("-00:00");
    seconds(&by)?;
    (utc && goal > 0.0 && goal <= 100.0).then_some(Target { by, spend_to_pct: goal })
}

pub fn target_for(account: &str, at: f64) -> Result<Option<Target>, String> {
    target_in(&policy_path(), account, at)
}

pub fn target_in(policy: &Path, account: &str, at: f64) -> Result<Option<Target>, String> {
    let invalid = || format!("invalid or unreadable pace target policy for {account}");
    let text = fs::read_to_string(policy).map_err(|_| invalid())?;
    let document: DocumentMut = text.parse().map_err(|_| invalid())?;
    let Some(item) = document.get("pace").and_then(|pace| pace.get(account)) else {
        return Ok(None);
    };
    let target = entry(item).ok_or_else(invalid)?;
    Ok(target.deadline().filter(|deadline| *deadline > at).map(|_| target))
}
