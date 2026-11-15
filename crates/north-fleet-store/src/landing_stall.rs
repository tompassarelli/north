//! The landing-stall episodes `north ledger flow --stall-alarm` keeps, one per repository whose lander queue holds
//! lanes while origin/main stays unmoved. The alarm writes them and the coordinator's break-glass reads them.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "lander-stalls.json";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Episode {
    pub slug: String,
    pub text: String,
    pub told: bool,
    pub filed: bool,
    #[serde(default)]
    pub opened_ms: u64,
}

pub type State = BTreeMap<String, Episode>;

pub fn path(flow_dir: &Path) -> PathBuf {
    flow_dir.join(FILE)
}

pub fn load(path: &Path) -> State {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}
