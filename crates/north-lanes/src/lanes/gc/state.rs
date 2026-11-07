use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct LocalStale {
    pub(super) path: String,
    pub(super) branch: String,
    pub(super) age_h: i64,
    pub(super) dirty: bool,
    pub(super) owner: String,
    pub(super) flagged_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Owned {
    #[serde(rename = "where")]
    pub(super) place: String,
    pub(super) branch: String,
    pub(super) age_h: i64,
    pub(super) refs: Vec<String>,
    #[serde(default)]
    pub(super) issues: Vec<String>,
    pub(super) owner: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct RemoteStale {
    pub(super) branch: String,
    pub(super) age_h: i64,
    #[serde(default)]
    pub(super) issues: Vec<String>,
    pub(super) owner: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct RepoReport {
    #[serde(default)]
    pub(super) remote_landed: Vec<String>,
    #[serde(default)]
    pub(super) remote_stale: Vec<RemoteStale>,
    #[serde(default)]
    pub(super) local_stale: Vec<LocalStale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) owned: Option<Vec<Owned>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct StaleReport {
    #[serde(default)]
    pub(super) at: String,
    #[serde(default)]
    pub(super) repos: BTreeMap<String, RepoReport>,
    #[serde(default)]
    pub(super) notices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) alerts: Option<BTreeMap<String, String>>,
}

impl StaleReport {
    pub(super) fn load(path: &Path) -> Self {
        fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub(super) fn repo(&mut self, name: &str) -> &mut RepoReport {
        self.repos.entry(name.to_string()).or_insert_with(|| RepoReport { owned: Some(Vec::new()), ..RepoReport::default() })
    }
}

pub(super) fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}
