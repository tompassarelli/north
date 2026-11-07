use super::snapshot::git_ok;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(super) struct State {
    pub(super) root: PathBuf,
    pub(super) home: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Marker {
    pub(super) sha: String,
    pub(super) at: String,
    pub(super) hand_lines: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct LoopRecord {
    pub(super) at: String,
    pub(super) sha: String,
    pub(super) since: Option<String>,
    pub(super) change_cost: super::change_cost::ChangeCost,
    pub(super) review: super::config::Review,
    pub(super) fired: Vec<String>,
}

impl State {
    pub(super) fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let root = std::env::var_os("XDG_STATE_HOME").filter(|root| !root.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from);
        Self { root, home }
    }

    pub(super) fn repo_name(&self, repo: &Path) -> String {
        let code = format!("{}/", self.home.join("code").display());
        let text = repo.to_string_lossy();
        if let Some(rest) = text.strip_prefix(&code) {
            return rest.split('/').next().unwrap_or_default().to_owned();
        }
        let base = |text: &str| text.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_owned();
        match git_ok(repo, &["remote", "get-url", "origin"]).map(|url| url.trim().to_owned()).filter(|url| !url.is_empty()) {
            Some(url) => base(&url).trim_end_matches(".git").to_owned(),
            None => base(&text),
        }
    }

    pub(super) fn directory(&self, repo: &Path) -> PathBuf {
        self.root.join("code-health").join(self.repo_name(repo))
    }

    pub(super) fn baseline(&self, repo: &Path) -> Option<serde_json::Value> {
        std::fs::read_to_string(self.directory(repo).join("baseline.json")).ok().and_then(|text| serde_json::from_str(&text).ok())
    }

    pub(super) fn marker(&self, repo: &Path) -> Result<Option<Marker>, String> {
        let path = self.directory(repo).join("review.json");
        let Ok(text) = std::fs::read_to_string(&path) else { return Ok(None) };
        serde_json::from_str(&text).map(Some).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub(super) fn write_marker(&self, repo: &Path, marker: &Marker) -> Result<(), String> {
        write_json(&self.directory(repo).join("review.json"), marker)
    }

    pub(super) fn write_loop(&self, repo: &Path, record: &LoopRecord, score: &str) -> Result<(), String> {
        let directory = self.directory(repo);
        write_json(&directory.join("loop.json"), record)?;
        let path = directory.join("loop.txt");
        std::fs::write(&path, format!("{score}\n")).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub(super) fn save(&self, repo: &Path, baseline: &impl Serialize, trend: &impl Serialize) -> Result<(), String> {
        let directory = self.directory(repo);
        write_json(&directory.join("baseline.json"), baseline)?;
        let row = serde_json::to_string(trend).map_err(|error| error.to_string())?;
        let path = directory.join("trend.jsonl");
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        writeln!(file, "{row}").map_err(|error| format!("{}: {error}", path.display()))
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|error| format!("{}: {error}", path.display()))
}
