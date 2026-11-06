use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::facts::LinkFacts;

pub const GOOD_KEPT: usize = 5;

/// What the doctor remembers between rounds: the lander builds that landed a batch green, oldest first, and each open episode.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub good: Vec<String>,
    #[serde(default)]
    pub episodes: BTreeMap<String, Episode>,
    #[serde(default)]
    pub checked_at: u64,
}

/// One bound held over from `since` until a round reads it within again; each remedy applied in it and the page sent about it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub since: u64,
    #[serde(default)]
    pub remedies: Vec<(u64, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paged: Option<u64>,
}

impl Book {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())? + "\n";
        north_landing::land::queue::write_atomic(path, &text).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn last_remedy(&self, key: &str, class: &str) -> Option<u64> {
        self.episodes.get(key)?.remedies.iter().filter(|(_, applied)| applied == class).map(|(at, _)| *at).max()
    }

    pub fn open(&mut self, key: &str, now: u64) -> &mut Episode {
        self.episodes.entry(key.to_owned()).or_insert_with(|| Episode { since: now, ..Episode::default() })
    }

    /// Marks the build the lander link names good once an after-landing run that started after the link last moved passed.
    pub fn learn(&mut self, link: &LinkFacts) {
        if !link.green_since_link || self.good.last() == Some(&link.lander) {
            return;
        }
        self.good.retain(|good| *good != link.lander);
        self.good.push(link.lander.clone());
        let surplus = self.good.len().saturating_sub(GOOD_KEPT);
        self.good.drain(..surplus);
    }
}
