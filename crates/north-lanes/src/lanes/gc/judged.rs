use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::content::cache_path;
use super::exec::verified;
use super::state::write_atomically;

const CACHE: &str = "north/lane-gc-verdicts.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Landed {
    pub(super) landed: bool,
    pub(super) ahead: u64,
    pub(super) how: String,
}

#[derive(Default, Deserialize, Serialize)]
struct Kept {
    base: String,
    verdicts: HashMap<String, Landed>,
}

pub(super) struct Judged {
    path: Option<PathBuf>,
    kept: Kept,
}

impl Judged {
    pub(super) fn read(repo: &Path, base: &str) -> Self {
        let base = verified(repo, &format!("{base}^{{commit}}")).unwrap_or_default();
        let path = cache_path(repo, CACHE).filter(|_| !base.is_empty());
        let kept = path.as_deref().and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| serde_json::from_str::<Kept>(&text).ok()).filter(|kept| kept.base == base);
        Self { path, kept: kept.unwrap_or(Kept { base, verdicts: HashMap::new() }) }
    }

    pub(super) fn unjudged(&self, tips: &[String]) -> Vec<String> {
        tips.iter().filter(|tip| !self.kept.verdicts.contains_key(*tip)).cloned().collect()
    }

    pub(super) fn keep(mut self, tips: &[String], fresh: HashMap<String, Landed>) -> HashMap<String, Landed> {
        self.kept.verdicts.extend(fresh);
        self.kept.verdicts.retain(|tip, _| tips.contains(tip));
        if let (Some(path), Ok(text)) = (self.path.as_deref(), serde_json::to_string(&self.kept)) {
            let _ = write_atomically(path, &text);
        }
        self.kept.verdicts
    }
}
