use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct LocalStale {
    pub(super) path: String,
    pub(super) branch: String,
    pub(super) age_h: i64,
    pub(super) dirty: bool,
    pub(super) owner: String,
    pub(super) flagged_at: String,
    pub(super) retire_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Owned {
    #[serde(rename = "where")]
    pub(super) place: String,
    pub(super) branch: String,
    pub(super) age_h: i64,
    pub(super) refs: Vec<String>,
    pub(super) owner: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct RemoteStale {
    pub(super) branch: String,
    pub(super) age_h: i64,
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

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub(super) struct LandingPing {
    pub(super) tip: String,
    pub(super) pushed_at: i64,
    pub(super) pinged_at: String,
    pub(super) owner: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct StaleReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) landing_pings: Option<BTreeMap<String, LandingPing>>,
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

    pub(super) fn forget_ping(&mut self, key: &str) {
        if let Some(pings) = self.landing_pings.as_mut() {
            pings.remove(key);
        }
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

pub(super) fn mirror_path(home: &Path, container: &str) -> PathBuf {
    let root = std::env::var_os("LANE_GC_MIRRORS")
        .filter(|value| !value.is_empty())
        .map_or_else(|| home.join(".local/state/agents/lane-gc/mirrors"), PathBuf::from);
    root.join(format!("{container}.git"))
}

pub(super) fn restore_command(home: &Path, repo: &str, branch: &str, tag: &str) -> String {
    let dated = tag
        .strip_prefix("refs/archive/")
        .and_then(|rest| rest.get(..11))
        .is_some_and(|day| day.bytes().enumerate().all(|(i, b)| if matches!(i, 4 | 7) { b == b'-' } else if i == 10 { b == b'/' } else { b.is_ascii_digit() }));
    if dated {
        let main = home.join("code").join(repo).join("main");
        format!("git -C {} push $(git -C {} remote get-url origin) {tag}:refs/heads/{branch}", mirror_path(home, repo).display(), main.display())
    } else {
        format!("git push origin {tag}:refs/heads/{branch}")
    }
}
