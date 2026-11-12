//! A draft lane's mark (north#1240): `north lane new --draft NAME --check CMD` writes it under the repository's
//! `north/lane-drafts/SLUG.json`, and the finish call in that lane reads it to record the draft rather than queue it.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::repo::git_out;

pub const RECORDS: &str = "north/lane-drafts";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Mark {
    pub name: String,
    pub item: String,
    pub base: String,
    pub check: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_exit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

fn path(lane: &Path) -> Option<PathBuf> {
    let common = git_out(lane, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let slug = git_out(lane, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    (!slug.is_empty() && slug != "HEAD").then(|| PathBuf::from(common).join(RECORDS).join(format!("{slug}.json")))
}

pub fn read(lane: &Path) -> Option<Mark> {
    serde_json::from_str(&std::fs::read_to_string(path(lane)?).ok()?).ok()
}

pub fn write(lane: &Path, mark: &Mark) -> Result<(), String> {
    let path = path(lane).ok_or_else(|| format!("{} is no lane on a branch", lane.display()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string(mark).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}
