//! The verdicts the broker's review calls keep by patch id in the verdict store (north#1610), which the broker writes and the lander reads to refuse a held ticket whose patch a call refused (north#1632).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The verdict store's directory of the review calls.
pub const REVIEW_CALLS: &str = "review-calls";

/// One review call's verdict on a patch: whether it accepted, and its findings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallVerdict {
    pub session: String,
    pub accept: bool,
    pub findings: String,
}

/// The file stem the review-call store keeps `word` under.
pub fn store_key(word: &str) -> String {
    word.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// The review calls' verdicts kept in a verdict store.
pub struct ReviewCalls(PathBuf);

impl ReviewCalls {
    pub fn of(store: &Path) -> Self {
        Self(store.join(REVIEW_CALLS))
    }

    /// The findings of the review calls that refused `patch_id` when none accepted it; `None` while no call refused it or one accepted.
    pub fn refused(&self, patch_id: &str) -> Option<String> {
        let text = fs::read_to_string(self.0.join("patches").join(format!("{}.json", store_key(patch_id)))).ok()?;
        let verdicts: Vec<CallVerdict> = serde_json::from_str(&text).ok()?;
        let refusals: Vec<String> = verdicts.iter().filter(|verdict| !verdict.accept).map(|verdict| format!("{} REFUSE patch {patch_id}\n{}", verdict.session, verdict.findings)).collect();
        (!refusals.is_empty() && !verdicts.iter().any(|verdict| verdict.accept)).then(|| refusals.join("\n\n"))
    }
}
